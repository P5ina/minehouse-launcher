//! «Horizon (лёгкая)»: Minecraft 26.3 rewritten in Rust
//! (<https://github.com/horizon-rust/horizon>) with MineHouse's native mods
//! (voice chat, ping wheel) compiled in by P5ina/minehouse-horizon's CI.
//!
//! Installing follows Horizon's launch guide (`docs/api/launch-guide.md`):
//!
//! 1. Our build: `minehouse-horizon-<platform>.zip` from the latest
//!    minehouse-horizon release, a `horizon-client` plus `minehouse-horizon.json`
//!    naming the Horizon commit and Nightly it was built from.
//! 2. That Nightly of Horizon for this platform, checked against its signed
//!    `SHA256SUMS`, unpacked into a folder of its own; its `horizon-client` is
//!    replaced by ours, as Horizon's launcher installs a native build.
//! 3. `horizon-prepare` turns the player's own Minecraft files (version JSON,
//!    client and server jar, libraries, assets: the launcher downloads them for
//!    the signed-in owner) into the resources the game reads. Once per build:
//!    later starts only run `--check`.
//! 4. `horizon-client --resources … --game-dir <instance> --session-stdin`.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::auth::Session;
use crate::http::{self, Download, Hash};
use crate::minecraft::{self, JavaVersion};
use crate::paths::Paths;
use crate::progress::Reporter;
use crate::rules::Env;

/// Where the MineHouse builds of Horizon are published (a `v*` tag there
/// makes a release). `MINEHOUSE_HORIZON_URL` points elsewhere, e.g. a mirror.
pub const BUILDS: &str = "https://github.com/P5ina/minehouse-horizon/releases/latest/download";
/// Horizon's own releases: `<RELEASES>/<tag>/<archive>`.
pub const RELEASES: &str = "https://github.com/horizon-rust/horizon/releases/download";
/// Its refs, to see which commit the moving `nightly` tag is on.
const REFS: &str = "https://github.com/horizon-rust/horizon.git/info/refs?service=git-upload-pack";
/// `crates/horizon-release-sign/release-keys.txt`: the keys Horizon's
/// `SHA256SUMS` are signed with.
const RELEASE_KEYS: &[&str] = &["MU4oiIQcCxkZvbwSjjXs7SySCuM6ywRQ3nGaJYZG0gw="];
/// Horizon's Java for 26.3, if the preparer doesn't say otherwise.
const JAVA: (&str, u32) = ("java-runtime-epsilon", 25);
const EXE: &str = std::env::consts::EXE_SUFFIX;
/// Written into a build folder once it is complete.
const MARKER: &str = ".minehouse-build.json";

/// The platform name used by both minehouse-horizon and Horizon, for the
/// platforms minehouse-horizon's CI builds.
pub fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("macos-arm64"),
        ("linux", "x86_64") => Some("linux-x86_64"),
        ("windows", "x86_64") => Some("windows-x64"),
        _ => None,
    }
}

pub fn supported() -> bool {
    platform().is_some()
}

/// Horizon's archive for a platform.
fn archive_name(platform: &str) -> String {
    if platform.starts_with("linux") {
        format!("horizon-{platform}.tar.gz")
    } else {
        format!("horizon-{platform}.zip")
    }
}

/// `minehouse-horizon.json` inside our zip.
#[derive(Clone, Debug, Deserialize, serde::Serialize, PartialEq)]
pub struct BuildInfo {
    pub horizon_revision: String,
    pub horizon_nightly: String,
    pub minehouse_commit: String,
    pub platform: String,
}

/// Everything [`launch`] needs.
#[derive(Clone, Debug)]
pub struct Installed {
    /// The unpacked Nightly with our `horizon-client`.
    pub dir: PathBuf,
    pub client: PathBuf,
    /// horizon-prepare's output.
    pub resources: PathBuf,
    pub build: BuildInfo,
    pub server: String,
}

fn short(rev: &str) -> &str {
    &rev[..rev.len().min(12)]
}

/// Downloads (or checks) everything and prepares the resources.
pub async fn install(paths: &Paths, reporter: &Reporter) -> Result<Installed> {
    let client = http::client();
    let platform = platform().context(
        "Horizon пока собирается только для Windows x64, Linux x86_64 и macOS на Apple silicon",
    )?;
    let root = paths.data.join("horizon");
    let downloads = root.join("downloads");

    reporter.stage("Проверяем обновления Horizon");
    let (zip, zip_sha) = our_build(&client, &downloads, platform, reporter).await?;
    let build = {
        let zip = zip.clone();
        tokio::task::spawn_blocking(move || read_build_info(&zip)).await??
    };
    anyhow::ensure!(
        build.platform == platform,
        "Сборка Horizon для {}, а не для {platform}",
        build.platform
    );
    reporter.log(format!(
        "Horizon {} · MineHouse {}",
        build.horizon_nightly,
        short(&build.minehouse_commit)
    ));

    let builds = root.join("builds");
    let dir = builds.join(format!(
        "{}-{}",
        build.horizon_nightly,
        short(&build.minehouse_commit)
    ));
    let client_exe = dir.join(format!("horizon-client{EXE}"));
    if marker_matches(&dir, &zip_sha) && client_exe.exists() {
        reporter.log("Horizon уже установлен");
    } else {
        reporter.stage("Скачиваем Horizon");
        let archive = fetch_nightly(&client, &build, platform, &downloads, reporter).await?;
        reporter.stage("Распаковываем Horizon");
        let (archive, zip, dir, builds, sha) = (
            archive,
            zip.clone(),
            dir.clone(),
            builds.clone(),
            zip_sha.clone(),
        );
        let platform = platform.to_string();
        tokio::task::spawn_blocking(move || {
            unpack_build(&archive, &zip, &builds, &dir, &platform, &sha)?;
            prune(&builds, &dir);
            anyhow::Ok(())
        })
        .await??;
    }

    let prepare = dir.join(format!("horizon-prepare{EXE}"));
    let resources = root.join("resources").join(short(&build.horizon_revision));
    if check(&prepare, &dir, &resources).await {
        reporter.log("Ресурсы Horizon готовы");
    } else {
        prepare_resources(&client, paths, &dir, &prepare, &resources, reporter).await?;
    }

    Ok(Installed {
        dir,
        client: client_exe,
        resources,
        build,
        server: crate::SERVER.into(),
    })
}

/// Our zip, downloaded (or still valid) in `downloads/`, and its SHA-256.
/// `MINEHOUSE_HORIZON_ZIP` uses a local zip instead (a CI artifact, for tests).
async fn our_build(
    client: &reqwest::Client,
    downloads: &Path,
    platform: &str,
    reporter: &Reporter,
) -> Result<(PathBuf, String)> {
    if let Some(local) = std::env::var_os("MINEHOUSE_HORIZON_ZIP") {
        let local = PathBuf::from(local);
        let bytes = tokio::fs::read(&local)
            .await
            .with_context(|| format!("MINEHOUSE_HORIZON_ZIP: {}", local.display()))?;
        reporter.log(format!("Сборка Horizon из {}", local.display()));
        return Ok((local, sha256_hex(&bytes)));
    }
    let base = std::env::var("MINEHOUSE_HORIZON_URL").unwrap_or_else(|_| BUILDS.into());
    let base = base.trim_end_matches('/');
    let name = format!("minehouse-horizon-{platform}.zip");
    let path = downloads.join(&name);
    let sums = match get_text(client, &format!("{base}/SHA256SUMS")).await {
        Ok(s) => s,
        Err(e) => {
            // Offline: the zip we have is as good as it was last time.
            let cached = downloads.join(format!("{name}.sha256"));
            if let (true, Ok(sha)) = (path.exists(), tokio::fs::read_to_string(&cached).await) {
                reporter.log(format!(
                    "Нет связи с GitHub ({e:#}), играем на скачанной сборке"
                ));
                return Ok((path, sha.trim().to_string()));
            }
            return Err(e).context("Не удалось скачать сборку Horizon для MineHouse");
        }
    };
    let sha = find_sum(&sums, &name)
        .with_context(|| format!("В выпуске MineHouse Horizon нет {name}"))?;
    http::fetch_all(
        client,
        vec![Download {
            url: format!("{base}/{name}"),
            path: path.clone(),
            hash: Hash::Sha256(sha.clone()),
            size: None,
            executable: false,
        }],
        1,
        reporter,
    )
    .await?;
    tokio::fs::write(downloads.join(format!("{name}.sha256")), &sha).await?;
    Ok((path, sha))
}

fn read_build_info(zip: &Path) -> Result<BuildInfo> {
    let file = std::fs::File::open(zip)?;
    let mut archive = zip::ZipArchive::new(file).context("Сборка Horizon повреждена")?;
    let entry = archive
        .by_name("minehouse-horizon.json")
        .context("В сборке Horizon нет minehouse-horizon.json")?;
    serde_json::from_reader(entry).context("minehouse-horizon.json не читается")
}

fn marker_matches(dir: &Path, sha: &str) -> bool {
    std::fs::read_to_string(dir.join(MARKER))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("zip_sha256")?.as_str().map(str::to_owned))
        .is_some_and(|s| s.eq_ignore_ascii_case(sha))
}

/// Horizon's archive for the Nightly our build was made from. Dated Nightlies
/// are not always published as their own release; then the moving `nightly`
/// release is used, but only while it is still on that commit.
async fn fetch_nightly(
    client: &reqwest::Client,
    build: &BuildInfo,
    platform: &str,
    downloads: &Path,
    reporter: &Reporter,
) -> Result<PathBuf> {
    let archive = archive_name(platform);
    let mut sums = None;
    let mut missing = None;
    for tag in [build.horizon_nightly.as_str(), "nightly"] {
        let base = format!("{RELEASES}/{tag}");
        let list = match get_bytes(client, &format!("{base}/SHA256SUMS")).await {
            Ok(list) => list,
            Err(e) => {
                missing = Some(e);
                continue;
            }
        };
        if tag == "nightly" {
            let head = nightly_revision(client).await?;
            anyhow::ensure!(
                head == build.horizon_revision,
                "Horizon {} уже удалён с GitHub, а сборки MineHouse под новую Nightly ещё нет. \
                 Попробуйте позже или выберите обычную сборку",
                build.horizon_nightly
            );
        }
        let sig = get_bytes(client, &format!("{base}/SHA256SUMS.sig")).await?;
        verify_sums(tag, &list, &sig)?;
        sums = Some((base, String::from_utf8(list)?));
        break;
    }
    let Some((base, sums)) = sums else {
        let e = missing.unwrap_or_else(|| anyhow::anyhow!("нет SHA256SUMS"));
        return Err(e.context(format!("Нет выпуска Horizon {}", build.horizon_nightly)));
    };
    let sha =
        find_sum(&sums, &archive).with_context(|| format!("В выпуске Horizon нет {archive}"))?;
    let path = downloads.join(format!("{}-{archive}", short(&build.horizon_revision)));
    reporter.log(format!("{base}/{archive}"));
    http::fetch_all(
        client,
        vec![Download {
            url: format!("{base}/{archive}"),
            path: path.clone(),
            hash: Hash::Sha256(sha),
            size: None,
            executable: false,
        }],
        1,
        reporter,
    )
    .await?;
    Ok(path)
}

/// The commit `refs/tags/nightly` points at, from the smart-HTTP ref list.
async fn nightly_revision(client: &reqwest::Client) -> Result<String> {
    let refs = get_text(client, REFS).await?;
    parse_ref(&refs, "refs/tags/nightly")
        .context("Не удалось узнать, на каком коммите Nightly Horizon")
}

fn parse_ref(refs: &str, name: &str) -> Option<String> {
    let peeled = format!(" {name}^{{}}");
    let plain = format!(" {name}");
    let mut found = None;
    for line in refs.split('\n') {
        let line = line.split('\0').next().unwrap_or(line);
        for (suffix, best) in [(&peeled, true), (&plain, false)] {
            if let Some(head) = line.strip_suffix(suffix.as_str()) {
                if head.len() >= 40 {
                    let sha = &head[head.len() - 40..];
                    if sha.chars().all(|c| c.is_ascii_hexdigit()) {
                        if best {
                            return Some(sha.to_string());
                        }
                        found = Some(sha.to_string());
                    }
                }
            }
        }
    }
    found
}

/// `SHA256SUMS.sig` signs `horizon release <tag>\n` + `SHA256SUMS`; every
/// Nightly is signed as `nightly`, whichever date it carries.
fn verify_sums(tag: &str, sums: &[u8], sig: &[u8]) -> Result<()> {
    use base64::Engine;
    use ed25519_dalek::{Signature, VerifyingKey};
    let b64 = base64::engine::general_purpose::STANDARD;
    let sig = b64
        .decode(String::from_utf8_lossy(sig).trim())
        .context("SHA256SUMS.sig не читается")?;
    let sig = Signature::from_slice(&sig).context("SHA256SUMS.sig не читается")?;
    let signed_tag = if tag.starts_with("nightly") {
        "nightly"
    } else {
        tag
    };
    let mut msg = format!("horizon release {signed_tag}\n").into_bytes();
    msg.extend_from_slice(sums);
    for key in RELEASE_KEYS {
        let Ok(bytes) = b64.decode(key) else { continue };
        let Ok(bytes) = <[u8; 32]>::try_from(bytes.as_slice()) else {
            continue;
        };
        let Ok(key) = VerifyingKey::from_bytes(&bytes) else {
            continue;
        };
        if key.verify_strict(&msg, &sig).is_ok() {
            return Ok(());
        }
    }
    bail!("Подпись выпуска Horizon {tag} не сходится: скачивание отменено")
}

/// The SHA-256 of `name` in a `sha256sum` list.
fn find_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let sha = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && sha.len() == 64).then(|| sha.to_lowercase())
    })
}

/// Unpacks the Nightly into `dir`, puts our `horizon-client` in place of its
/// own and writes the marker last, so a half-done folder is never used.
fn unpack_build(
    archive: &Path,
    ours: &Path,
    builds: &Path,
    dir: &Path,
    platform: &str,
    zip_sha: &str,
) -> Result<()> {
    std::fs::create_dir_all(builds)?;
    let tmp = builds.join(".unpacking");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let file = std::fs::File::open(archive)?;
    if archive.extension().is_some_and(|e| e == "zip") {
        zip::ZipArchive::new(file)?
            .extract(&tmp)
            .context("Не удалось распаковать Horizon")?;
    } else {
        tar::Archive::new(flate2::read::GzDecoder::new(file))
            .unpack(&tmp)
            .context("Не удалось распаковать Horizon")?;
    }
    // The archive holds one folder, `horizon-<platform>/`.
    let inner = tmp.join(format!("horizon-{platform}"));
    let unpacked = if inner.is_dir() { inner } else { tmp.clone() };

    let exe = format!("horizon-client{EXE}");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(ours)?)?;
    let mut entry = zip
        .by_name(&exe)
        .with_context(|| format!("В сборке MineHouse Horizon нет {exe}"))?;
    let target = unpacked.join(&exe);
    let _ = std::fs::remove_file(&target);
    let mut out = std::fs::File::create(&target)?;
    std::io::copy(&mut entry, &mut out)?;
    drop(out);
    // Should a macOS Nightly ship the game as an app bundle, its executable
    // is ours too (the launcher always starts the one at the folder's top).
    let bundled = unpacked.join("Horizon.app/Contents/MacOS").join(&exe);
    if bundled.exists() {
        std::fs::copy(&target, &bundled)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bundled, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in [
            "horizon-client",
            "horizon-prepare",
            "horizon-ctl",
            "horizon-server",
        ] {
            let p = unpacked.join(name);
            if p.exists() {
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Signed ad hoc: no quarantine flag, or Gatekeeper asks on first start.
        let _ = std::process::Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(&unpacked)
            .status();
    }

    if dir.exists() {
        std::fs::remove_dir_all(dir)
            .with_context(|| format!("Не удалось заменить {}", dir.display()))?;
    }
    std::fs::rename(&unpacked, dir)?;
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::write(
        dir.join(MARKER),
        serde_json::to_vec_pretty(&serde_json::json!({ "zip_sha256": zip_sha }))?,
    )?;
    Ok(())
}

/// Keeps the current build and the newest other one (to go back to while its
/// Nightly still exists), deletes older ones.
fn prune(builds: &Path, current: &Path) {
    let Ok(entries) = std::fs::read_dir(builds) else {
        return;
    };
    let mut others: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p != current && p.join(MARKER).exists())
        .filter_map(|p| Some((std::fs::metadata(p.join(MARKER)).ok()?.modified().ok()?, p)))
        .collect();
    others.sort_by_key(|o| std::cmp::Reverse(o.0));
    for (_, p) in others.into_iter().skip(1) {
        let _ = std::fs::remove_dir_all(p);
    }
}

fn command(exe: &Path, cwd: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.current_dir(cwd).stdin(Stdio::null());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `horizon-prepare --check`: the resources are complete and current.
async fn check(prepare: &Path, dir: &Path, resources: &Path) -> bool {
    if !resources.exists() {
        return false;
    }
    command(prepare, dir)
        .args(["--check", "--output"])
        .arg(resources)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success())
}

/// What `horizon-prepare --launch-api` says it prepares for.
#[derive(Debug, Default, Deserialize)]
struct PrepareApi {
    #[serde(default)]
    minecraft: Option<String>,
    #[serde(default)]
    java_major: Option<u32>,
}

async fn prepare_api(prepare: &Path, dir: &Path) -> PrepareApi {
    let out = command(prepare, dir)
        .arg("--launch-api")
        .stderr(Stdio::null())
        .output()
        .await;
    out.ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or_default()
}

/// Downloads the owner's vanilla files and Java, then runs the preparer.
async fn prepare_resources(
    client: &reqwest::Client,
    paths: &Paths,
    dir: &Path,
    prepare: &Path,
    resources: &Path,
    reporter: &Reporter,
) -> Result<()> {
    let api = prepare_api(prepare, dir).await;
    let mc = api
        .minecraft
        .clone()
        .unwrap_or_else(|| crate::DEFAULT_MINECRAFT.into());
    reporter.stage("Скачиваем Minecraft для Horizon");
    let version = minecraft::fetch_version(client, paths, &mc).await?;
    let env = Env::current();
    let mut downloads = vec![
        minecraft::client_jar(paths, &version)?,
        minecraft::server_jar(paths, &version)?,
    ];
    for lib in &version.libraries {
        if let Some(d) = minecraft::library_download(lib, paths, &env)? {
            downloads.push(d);
        }
    }
    http::fetch_all(client, downloads, 16, reporter).await?;
    reporter.stage("Скачиваем ресурсы игры");
    let assets = minecraft::asset_downloads(client, paths, &version).await?;
    http::fetch_all(client, assets, 32, reporter).await?;

    let java_version = match (&version.java_version, api.java_major) {
        (Some(v), Some(major)) if v.major_version != major => JavaVersion {
            component: v.component.clone(),
            major_version: major,
        },
        (Some(v), _) => v.clone(),
        (None, major) => JavaVersion {
            component: JAVA.0.into(),
            major_version: major.unwrap_or(JAVA.1),
        },
    };
    let java = crate::install::java_for(client, paths, &java_version, reporter).await?;
    // bin/java → the Java home (on macOS …/Contents/Home).
    let java_home = java
        .parent()
        .and_then(Path::parent)
        .context("Не нашли папку Java")?
        .to_path_buf();

    reporter.stage("Готовим ресурсы Horizon (первый раз несколько минут)");
    tokio::fs::create_dir_all(resources).await?;
    let mut child = command(prepare, dir)
        .arg("--prepare")
        .arg("--server-jar")
        .arg(minecraft::server_jar(paths, &version)?.path)
        .arg("--minecraft-dir")
        .arg(&paths.data)
        .arg("--java-home")
        .arg(&java_home)
        .arg("--output")
        .arg(resources)
        .args(["--complete", "--progress", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Не удалось запустить {}", prepare.display()))?;

    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let progress = {
        let reporter = reporter.clone();
        tokio::spawn(async move {
            let mut error = None;
            let mut lines = BufReader::new(out).lines();
            reporter.progress(0, 1000);
            while let Ok(Some(line)) = lines.next_line().await {
                match parse_progress(&line) {
                    Progress::Message(m) => reporter.log(m),
                    Progress::Fraction(f) => reporter.progress((f * 1000.0) as u64, 1000),
                    Progress::Error(e) => {
                        reporter.log(format!("Ошибка: {e}"));
                        error = Some(e);
                    }
                    Progress::Other => {}
                }
            }
            error
        })
    };
    let stderr = {
        let reporter = reporter.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            let mut last = None;
            while let Ok(Some(line)) = lines.next_line().await {
                reporter.log(line.clone());
                if !line.trim().is_empty() {
                    last = Some(line);
                }
            }
            last
        })
    };
    let status = child.wait().await?;
    let error = progress.await.ok().flatten();
    let last = stderr.await.ok().flatten();
    if !status.success() {
        let why = error.or(last).unwrap_or_else(|| status.to_string());
        bail!("Horizon не смог подготовить ресурсы: {why}");
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
enum Progress {
    Message(String),
    Fraction(f64),
    Error(String),
    Other,
}

/// One line of `--progress json`. Unknown keys and lines are ignored.
fn parse_progress(line: &str) -> Progress {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return if line.trim().is_empty() {
            Progress::Other
        } else {
            Progress::Message(line.to_string())
        };
    };
    if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
        Progress::Error(e.to_string())
    } else if let Some(f) = v.get("fraction").and_then(|f| f.as_f64()) {
        Progress::Fraction(f.clamp(0.0, 1.0))
    } else if let Some(m) = v.get("message").and_then(|m| m.as_str()) {
        Progress::Message(m.to_string())
    } else {
        Progress::Other
    }
}

/// The options this `horizon-client` accepts (`--launch-api`), or `None`
/// when it doesn't say.
async fn client_options(inst: &Installed) -> Option<Vec<String>> {
    let out = command(&inst.client, &inst.dir)
        .arg("--launch-api")
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(
        v.get("options")?
            .as_array()?
            .iter()
            .filter_map(|o| o.get("name")?.as_str().map(str::to_owned))
            .collect(),
    )
}

/// The session JSON on the game's stdin (Launch API, «The session JSON»).
pub fn session_json(session: &Session) -> serde_json::Value {
    let mut v = serde_json::json!({
        "name": session.name,
        "uuid": session.uuid,
        "access_token": session.access_token,
    });
    if !session.is_offline() {
        v["user_type"] = "msa".into();
        if !session.xuid.is_empty() && session.xuid != "0" {
            v["xuid"] = session.xuid.clone().into();
        }
    }
    v
}

pub fn build_args(inst: &Installed, game_dir: &Path, options: Option<&[String]>) -> Vec<String> {
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let mut args = vec![
        "--resources".into(),
        s(&inst.resources),
        "--game-dir".into(),
        s(game_dir),
        "--home".into(),
        s(game_dir),
        "--session-stdin".into(),
    ];
    let knows = |o: &str| options.is_none_or(|list| list.iter().any(|n| n == o));
    if knows("--quickPlayMultiplayer") {
        args.push("--quickPlayMultiplayer".into());
        args.push(inst.server.clone());
    }
    args
}

/// Starts the game with the session on stdin and streams its output to the
/// reporter. Resolves when it exits, with Horizon's exit codes as errors.
pub async fn launch(
    paths: &Paths,
    inst: &Installed,
    session: &Session,
    reporter: &Reporter,
) -> Result<()> {
    let game = paths.game_dir();
    tokio::fs::create_dir_all(&game).await?;
    let options = client_options(inst).await;
    let args = build_args(inst, &game, options.as_deref());
    let mut child = command(&inst.client, &game)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Не удалось запустить {}", inst.client.display()))?;
    reporter.log(format!("Запуск: {}", inst.client.display()));

    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(session_json(session).to_string().as_bytes())
        .await?;
    stdin.shutdown().await?;
    drop(stdin);

    let token = session.access_token.clone();
    let pipe = |r: Box<dyn tokio::io::AsyncRead + Unpin + Send>| {
        let reporter = reporter.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(r).lines();
            let mut last = None;
            while let Ok(Some(line)) = lines.next_line().await {
                let line = if token.len() > 8 {
                    line.replace(&token, "<token>")
                } else {
                    line
                };
                reporter.log(line.clone());
                if !line.trim().is_empty() {
                    last = Some(line);
                }
            }
            last
        })
    };
    let out = pipe(Box::new(child.stdout.take().unwrap()));
    let err = pipe(Box::new(child.stderr.take().unwrap()));
    let status = child.wait().await?;
    let _ = out.await;
    let last = err.await.ok().flatten().unwrap_or_default();
    match status.code() {
        Some(0) => Ok(()),
        Some(2) => bail!("Horizon не принял параметры запуска: {last}"),
        Some(1) | Some(101) => match last.split_once("crash report saved to ") {
            Some((_, path)) => bail!("Horizon упал. Отчёт об ошибке: {}", path.trim()),
            None => bail!("Horizon закрылся с ошибкой: {last}"),
        },
        _ => bail!("Horizon закрылся с ошибкой ({status}). Подробности в журнале."),
    }
}

async fn get_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let res = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("GET {url}"))?;
    Ok(res.bytes().await?.to_vec())
}

async fn get_text(client: &reqwest::Client, url: &str) -> Result<String> {
    Ok(String::from_utf8(get_bytes(client, url).await?)?)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `nightly` release of 2026-11-13 (commit 9f42064), signed.
    const SUMS: &str = "83d36c5a2f8b6690d4d77411dfc3f85753319184fffe738ee5f0a4c552a0eabe  horizon-linux-x86_64.tar.gz
f47f9f988b7c30b727dfb35aed2b74f01428925b2eccc9bafff2352f4847f7d6  horizon-windows-x64.zip
9cc4b7a72afba2df2a2c729f76a983d1e6cb82de44d1474fca04bc4d2a40d553  horizon-macos-arm64.zip
f7d2d17d488ab2111fa0e7ce36a033fe6eb6db3ee6b74bc86aaa26dbab7e0144  horizon-macos-x64.zip
f621e334c254d56bfeed16ed989500a36ef82a6a2e84ee6b861adcb774ced9b5  horizon-server-linux-x86_64.tar.gz
aa9bf9383ae4e926d5f9c2ca8be997e843a6cf590fc3c8c04162a26718adeabd  horizon-source.tar.gz
";
    const SIG: &str =
        "aKtoYPO/dznYhh78vFORQvWnW/vzUvi6mM7w2PSNZcVwmMkuFpJzRbu+YvqMOJ0krL8Qkec/wNPYLZ6wzPmoCg==";

    #[test]
    fn signed_sums() {
        verify_sums("nightly", SUMS.as_bytes(), SIG.as_bytes()).unwrap();
        verify_sums(
            "nightly-2026-11-13-9f42064",
            SUMS.as_bytes(),
            SIG.as_bytes(),
        )
        .unwrap();
        assert!(verify_sums("v1.0", SUMS.as_bytes(), SIG.as_bytes()).is_err());
        let tampered = SUMS.replace("83d36c", "00d36c");
        assert!(verify_sums("nightly", tampered.as_bytes(), SIG.as_bytes()).is_err());
        assert_eq!(
            find_sum(SUMS, "horizon-windows-x64.zip").as_deref(),
            Some("f47f9f988b7c30b727dfb35aed2b74f01428925b2eccc9bafff2352f4847f7d6")
        );
        assert_eq!(find_sum(SUMS, "horizon-windows-arm64.zip"), None);
    }

    #[test]
    fn refs() {
        let refs = "001e# service=git-upload-pack\n0000015b1111111111111111111111111111111111111111 HEAD\0multi_ack\n\
            003f9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0 refs/tags/nightly\n\
            0049aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa refs/tags/nightly-2026-11-08\n0000";
        assert_eq!(
            parse_ref(refs, "refs/tags/nightly").as_deref(),
            Some("9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0")
        );
        let annotated = "0040bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb refs/tags/nightly\n\
            0043cccccccccccccccccccccccccccccccccccccccc refs/tags/nightly^{}\n";
        assert_eq!(
            parse_ref(annotated, "refs/tags/nightly").as_deref(),
            Some("cccccccccccccccccccccccccccccccccccccccc")
        );
        assert_eq!(parse_ref(refs, "refs/tags/v1"), None);
    }

    #[test]
    fn progress_lines() {
        assert_eq!(
            parse_progress(r#"{"fraction": 0.5}"#),
            Progress::Fraction(0.5)
        );
        assert_eq!(
            parse_progress(r#"{"message": "Reading jars"}"#),
            Progress::Message("Reading jars".into())
        );
        assert_eq!(
            parse_progress(r#"{"error": "no server jar"}"#),
            Progress::Error("no server jar".into())
        );
        assert_eq!(parse_progress(r#"{"new": 1}"#), Progress::Other);
    }

    #[test]
    fn launch_args_and_session() {
        let inst = Installed {
            dir: "/h".into(),
            client: "/h/horizon-client".into(),
            resources: "/r".into(),
            build: BuildInfo {
                horizon_revision: "9f42064".into(),
                horizon_nightly: "nightly-2026-11-13-9f42064".into(),
                minehouse_commit: "abc".into(),
                platform: "linux-x86_64".into(),
            },
            server: "play.minehouse.pro".into(),
        };
        let args = build_args(&inst, Path::new("/g"), None);
        assert_eq!(
            args,
            [
                "--resources",
                "/r",
                "--game-dir",
                "/g",
                "--home",
                "/g",
                "--session-stdin",
                "--quickPlayMultiplayer",
                "play.minehouse.pro"
            ]
        );
        let old = vec!["--resources".to_string(), "--game-dir".into()];
        assert!(!build_args(&inst, Path::new("/g"), Some(&old))
            .contains(&"--quickPlayMultiplayer".into()));

        let s = Session {
            name: "Alex".into(),
            uuid: "069a79f444e94726a5befca90e38aaf5".into(),
            xuid: "123".into(),
            access_token: "tok".into(),
            expires_at: 0,
        };
        let j = session_json(&s);
        assert_eq!(j["user_type"], "msa");
        assert_eq!(j["xuid"], "123");
        let alt = session_json(&crate::auth::offline_session("Twink"));
        assert!(alt.get("user_type").is_none());
        assert_eq!(alt["name"], "Twink");
    }
}
