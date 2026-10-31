use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures::{StreamExt, TryStreamExt};
use serde::de::DeserializeOwned;
use sha1::Sha1;
use sha2::{Digest, Sha512};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::progress::Reporter;

pub type ReqwestClient = reqwest::Client;

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!(
            "MineHouseLauncher/{} (+https://www.minehouse.pro)",
            crate::LAUNCHER_VERSION
        ))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .expect("http client")
}

pub async fn get_json<T: DeserializeOwned>(client: &reqwest::Client, url: &str) -> Result<T> {
    let res = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let res = res
        .error_for_status()
        .with_context(|| format!("GET {url}"))?;
    res.json()
        .await
        .with_context(|| format!("bad JSON from {url}"))
}

/// Fetches JSON and keeps a copy at `cache`; when offline, the copy is used.
pub async fn get_json_cached<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    cache: &Path,
) -> Result<T> {
    let fresh = async {
        let res = client.get(url).send().await?.error_for_status()?;
        let bytes = res.bytes().await?;
        serde_json::from_slice::<serde_json::Value>(&bytes)?;
        anyhow::Ok(bytes)
    }
    .await;
    let bytes = match fresh {
        Ok(bytes) => {
            if let Some(dir) = cache.parent() {
                tokio::fs::create_dir_all(dir).await?;
            }
            tokio::fs::write(cache, &bytes).await?;
            bytes.to_vec()
        }
        Err(e) => match tokio::fs::read(cache).await {
            Ok(b) => {
                tracing::warn!("{url}: {e:#}, using cached copy");
                b
            }
            Err(_) => return Err(e.context(format!("GET {url}"))),
        },
    };
    serde_json::from_slice(&bytes).with_context(|| format!("bad JSON from {url}"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hash {
    Sha1(String),
    Sha512(String),
    None,
}

#[derive(Clone, Debug)]
pub struct Download {
    pub url: String,
    pub path: PathBuf,
    pub hash: Hash,
    pub size: Option<u64>,
    pub executable: bool,
}

enum Hasher {
    Sha1(Sha1),
    Sha512(Sha512),
    None,
}

impl Hasher {
    fn new(hash: &Hash) -> Self {
        match hash {
            Hash::Sha1(_) => Self::Sha1(Sha1::new()),
            Hash::Sha512(_) => Self::Sha512(Sha512::new()),
            Hash::None => Self::None,
        }
    }
    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha1(h) => h.update(data),
            Self::Sha512(h) => h.update(data),
            Self::None => {}
        }
    }
    fn matches(self, hash: &Hash) -> bool {
        match (self, hash) {
            (Self::Sha1(h), Hash::Sha1(want)) => {
                hex::encode(h.finalize()).eq_ignore_ascii_case(want)
            }
            (Self::Sha512(h), Hash::Sha512(want)) => {
                hex::encode(h.finalize()).eq_ignore_ascii_case(want)
            }
            (Self::None, Hash::None) => true,
            _ => false,
        }
    }
}

/// True when the file exists with the right size and hash.
pub async fn is_valid(path: &Path, hash: &Hash, size: Option<u64>) -> bool {
    let Ok(meta) = tokio::fs::metadata(path).await else {
        return false;
    };
    if !meta.is_file() || size.is_some_and(|s| s != meta.len()) {
        return false;
    }
    if *hash == Hash::None {
        return true;
    }
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return false;
    };
    let mut hasher = Hasher::new(hash);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return false,
        }
    }
    hasher.matches(hash)
}

async fn download_once(client: &reqwest::Client, d: &Download, done: &AtomicU64) -> Result<u64> {
    if let Some(dir) = d.path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let tmp = d.path.with_extension("mhpart");
    let res = client.get(&d.url).send().await?.error_for_status()?;
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut hasher = Hasher::new(&d.hash);
    let mut stream = res.bytes_stream();
    let mut written = 0u64;
    while let Some(chunk) = stream.try_next().await? {
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
        done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    file.flush().await?;
    drop(file);
    if d.size.is_some_and(|s| s != written) || !hasher.matches(&d.hash) {
        let _ = tokio::fs::remove_file(&tmp).await;
        bail!("{}: size or hash mismatch", d.url);
    }
    #[cfg(unix)]
    if d.executable {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).await?;
    }
    tokio::fs::rename(&tmp, &d.path).await?;
    Ok(written)
}

async fn download(client: &reqwest::Client, d: &Download, done: &AtomicU64) -> Result<()> {
    let mut last = None;
    for attempt in 0..4u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt))).await;
        }
        let before = done.load(Ordering::Relaxed);
        match download_once(client, d, done).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                // Undo the partial progress of the failed attempt.
                let now = done.load(Ordering::Relaxed);
                done.fetch_sub(now.saturating_sub(before), Ordering::Relaxed);
                last = Some(e);
            }
        }
    }
    Err(last.unwrap()).with_context(|| format!("download {}", d.url))
}

/// Downloads every file that is missing or broken, `parallel` at a time,
/// and reports byte progress. Valid files are skipped (that is the update check).
pub async fn fetch_all(
    client: &reqwest::Client,
    downloads: Vec<Download>,
    parallel: usize,
    reporter: &Reporter,
) -> Result<usize> {
    let total: u64 = downloads.iter().map(|d| d.size.unwrap_or(0)).sum();
    let done = Arc::new(AtomicU64::new(0));
    let fetched = Arc::new(AtomicU64::new(0));
    reporter.progress(0, total);

    let ticker = {
        let done = done.clone();
        let reporter = reporter.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(150)).await;
                reporter.progress(done.load(Ordering::Relaxed).min(total), total);
            }
        })
    };

    let result = futures::stream::iter(downloads)
        .map(|d| {
            let done = done.clone();
            let fetched = fetched.clone();
            async move {
                if is_valid(&d.path, &d.hash, d.size).await {
                    #[cfg(unix)]
                    if d.executable {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = tokio::fs::set_permissions(
                            &d.path,
                            std::fs::Permissions::from_mode(0o755),
                        )
                        .await;
                    }
                    done.fetch_add(d.size.unwrap_or(0), Ordering::Relaxed);
                    return anyhow::Ok(());
                }
                download(client, &d, &done).await?;
                fetched.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
        })
        .buffer_unordered(parallel)
        .try_collect::<Vec<()>>()
        .await;

    ticker.abort();
    result?;
    reporter.progress(total, total);
    Ok(fetched.load(Ordering::Relaxed) as usize)
}
