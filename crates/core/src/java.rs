//! Java from Mojang's own runtime manifest (the same builds the official
//! launcher uses), so players never install Java themselves.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::http::{self, Download, Hash};
use crate::paths::Paths;
use crate::progress::Reporter;

pub const RUNTIMES: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

type AllRuntimes = HashMap<String, HashMap<String, Vec<RuntimeEntry>>>;

#[derive(Debug, Deserialize)]
struct RuntimeEntry {
    manifest: ManifestRef,
    version: RuntimeVersion,
}

#[derive(Debug, Deserialize)]
struct ManifestRef {
    url: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeVersion {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeFile>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum RuntimeFile {
    Directory,
    File {
        #[serde(default)]
        executable: bool,
        downloads: FileDownloads,
    },
    Link {
        target: String,
    },
}

#[derive(Debug, Deserialize)]
struct FileDownloads {
    raw: Raw,
}

#[derive(Debug, Deserialize)]
struct Raw {
    sha1: String,
    size: u64,
    url: String,
}

/// Mojang's platform key for this machine.
pub fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "windows-arm64",
        ("windows", _) => "windows-x64",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        (_, "x86") => "linux-i386",
        _ => "linux",
    }
}

/// Path of the `java` binary inside an installed runtime.
pub fn java_binary(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("jre.bundle/Contents/Home/bin/java")
    } else if cfg!(windows) {
        // javaw: no console window behind the game.
        home.join("bin").join("javaw.exe")
    } else {
        home.join("bin/java")
    }
}

/// Installs (or repairs) runtime `component`, returns the java binary.
pub async fn install(
    client: &reqwest::Client,
    paths: &Paths,
    component: &str,
    reporter: &Reporter,
) -> Result<PathBuf> {
    let home = paths.runtime(component);
    let all_cache = paths.runtimes().join("all.json");
    let all: AllRuntimes = http::get_json_cached(client, RUNTIMES, &all_cache).await?;
    let entry = all
        .get(platform())
        .and_then(|p| p.get(component))
        .and_then(|v| v.first())
        .with_context(|| format!("Mojang has no Java '{component}' for {}", platform()))?;
    reporter.log(format!(
        "Java {} ({component}, {})",
        entry.version.name,
        platform()
    ));
    let manifest: RuntimeManifest =
        http::get_json_cached(client, &entry.manifest.url, &home.join("manifest.json")).await?;

    let mut downloads = Vec::new();
    let mut links = Vec::new();
    for (rel, f) in &manifest.files {
        let path = crate::paths::safe_join(&home, rel)?;
        match f {
            RuntimeFile::Directory => tokio::fs::create_dir_all(&path).await?,
            RuntimeFile::File {
                executable,
                downloads: d,
            } => downloads.push(Download {
                url: d.raw.url.clone(),
                path,
                hash: Hash::Sha1(d.raw.sha1.clone()),
                size: Some(d.raw.size),
                executable: *executable,
            }),
            RuntimeFile::Link { target } => links.push((path, target.clone())),
        }
    }
    http::fetch_all(client, downloads, 16, reporter).await?;

    #[cfg(unix)]
    for (path, target) in links {
        if tokio::fs::symlink_metadata(&path).await.is_ok() {
            continue;
        }
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        tokio::fs::symlink(&target, &path).await?;
    }
    #[cfg(not(unix))]
    drop(links);

    let java = java_binary(&home);
    anyhow::ensure!(
        java.exists(),
        "Java installed but {} is missing",
        java.display()
    );
    Ok(java)
}

/// A Java already on this computer that fits the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemJava {
    pub java: PathBuf,
    pub version: String,
}

/// Looks for an installed Java with exactly `major` for this CPU (JAVA_HOME,
/// PATH, the usual install folders, the official launcher and Prism), so a
/// player who already has it doesn't download another ~150 MB.
pub fn find_system(major: u32) -> Option<SystemJava> {
    let mut seen = std::collections::HashSet::new();
    for home in candidate_homes() {
        let Ok(home) = std::fs::canonicalize(&home) else {
            continue;
        };
        if !seen.insert(home.clone()) {
            continue;
        }
        if let Some(found) = check_home(&home, major) {
            return Some(found);
        }
    }
    None
}

/// Checks one Java home by its `release` file, without starting a JVM.
pub fn check_home(home: &Path, major: u32) -> Option<SystemJava> {
    let release = std::fs::read_to_string(home.join("release")).ok()?;
    let (version, arch) = parse_release(&release)?;
    if version_major(&version)? != major || !arch_matches(&arch) {
        return None;
    }
    let bin = home.join("bin");
    let java = if cfg!(windows) {
        ["javaw.exe", "java.exe"]
            .iter()
            .map(|n| bin.join(n))
            .find(|p| p.is_file())?
    } else {
        let p = bin.join("java");
        p.is_file().then_some(p)?
    };
    Some(SystemJava { java, version })
}

/// `JAVA_VERSION` and `OS_ARCH` from a JDK/JRE `release` file.
fn parse_release(text: &str) -> Option<(String, String)> {
    let get = |key: &str| {
        text.lines().find_map(|l| {
            let v = l.strip_prefix(key)?.strip_prefix('=')?;
            Some(v.trim().trim_matches('"').to_string())
        })
    };
    Some((get("JAVA_VERSION")?, get("OS_ARCH").unwrap_or_default()))
}

/// "25.0.1" → 25, "1.8.0_402" → 8.
fn version_major(v: &str) -> Option<u32> {
    let mut parts = v.split(|c: char| !c.is_ascii_digit());
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// An x64 Java under Rosetta or on Windows ARM runs, but slowly; take only
/// the native one. A missing OS_ARCH is trusted.
fn arch_matches(arch: &str) -> bool {
    let arch = arch.to_ascii_lowercase();
    if arch.is_empty() {
        return true;
    }
    match std::env::consts::ARCH {
        "x86_64" => matches!(arch.as_str(), "x86_64" | "amd64" | "x64"),
        "aarch64" => matches!(arch.as_str(), "aarch64" | "arm64"),
        "x86" => matches!(arch.as_str(), "x86" | "i386" | "i586" | "i686"),
        other => arch == other,
    }
}

/// Java homes worth checking, most likely first.
fn candidate_homes() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);

    if let Some(h) = env("JAVA_HOME") {
        out.push(h);
    }
    // `java` on PATH → its home (bin/..), following symlinks like
    // /usr/bin/java → /usr/lib/jvm/.../bin/java.
    if let Some(path) = std::env::var_os("PATH") {
        let exe = if cfg!(windows) { "java.exe" } else { "java" };
        for dir in std::env::split_paths(&path) {
            let java = dir.join(exe);
            if let Ok(real) = std::fs::canonicalize(&java) {
                if let Some(home) = real.parent().and_then(Path::parent) {
                    out.push(home.to_path_buf());
                }
            }
        }
    }

    // Folders that hold several Java homes side by side.
    let mut parents: Vec<PathBuf> = Vec::new();
    let home = env(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    if cfg!(windows) {
        for pf in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(pf) = env(pf) {
                for vendor in [
                    "Java",
                    "Eclipse Adoptium",
                    "Eclipse Foundation",
                    "Microsoft",
                    "Zulu",
                    "BellSoft",
                    "Amazon Corretto",
                    "Semeru",
                    "GraalVM",
                ] {
                    parents.push(pf.join(vendor));
                }
            }
        }
        if let Some(h) = &home {
            parents.push(h.join(".jdks"));
        }
    } else if cfg!(target_os = "macos") {
        parents.push("/Library/Java/JavaVirtualMachines".into());
        if let Some(h) = &home {
            parents.push(h.join("Library/Java/JavaVirtualMachines"));
        }
        parents.push("/opt/homebrew/opt".into());
        parents.push("/usr/local/opt".into());
    } else {
        parents.push("/usr/lib/jvm".into());
        parents.push("/usr/lib64/jvm".into());
        parents.push("/usr/java".into());
        parents.push("/opt".into());
        if let Some(h) = &home {
            parents.push(h.join(".jdks"));
            parents.push(h.join(".sdkman/candidates/java"));
        }
    }
    for p in parents {
        out.extend(subdirs(&p).into_iter().flat_map(mac_homes));
    }

    // Runtimes of other launchers: <runtime>/<component>/<platform>/<component>.
    let mut runtimes: Vec<PathBuf> = Vec::new();
    let mut prism: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        if let Some(a) = env("APPDATA") {
            runtimes.push(a.join(".minecraft/runtime"));
            prism.push(a.join("PrismLauncher/java"));
        }
        if let Some(l) = env("LOCALAPPDATA") {
            for pkg in subdirs(&l.join("Packages")) {
                let name = pkg.file_name().unwrap_or_default().to_string_lossy();
                if name.starts_with("Microsoft.4297127D64EC6") {
                    runtimes.push(pkg.join("LocalCache/Local/runtime"));
                }
            }
        }
    } else if let Some(h) = &home {
        if cfg!(target_os = "macos") {
            runtimes.push(h.join("Library/Application Support/minecraft/runtime"));
            prism.push(h.join("Library/Application Support/PrismLauncher/java"));
        } else {
            runtimes.push(h.join(".minecraft/runtime"));
            prism.push(h.join(".local/share/PrismLauncher/java"));
            prism.push(h.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/java"));
        }
    }
    for r in runtimes {
        for component in subdirs(&r) {
            for platform in subdirs(&component) {
                for home in subdirs(&platform) {
                    out.extend(mac_homes(home));
                }
            }
        }
    }
    for p in prism {
        out.extend(subdirs(&p).into_iter().flat_map(mac_homes));
    }
    out
}

/// On macOS a JDK folder keeps its home in Contents/Home (Mojang's in
/// jre.bundle/Contents/Home); elsewhere the folder is the home.
fn mac_homes(dir: PathBuf) -> Vec<PathBuf> {
    vec![
        dir.join("Contents/Home"),
        dir.join("jre.bundle/Contents/Home"),
        dir.join("libexec/openjdk.jdk/Contents/Home"),
        dir,
    ]
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    // Newest-looking names first (jdk-25.0.2 before jdk-25.0.1).
    v.sort();
    v.reverse();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn majors() {
        assert_eq!(version_major("25.0.1"), Some(25));
        assert_eq!(version_major("25"), Some(25));
        assert_eq!(version_major("1.8.0_402"), Some(8));
        assert_eq!(version_major("21-ea"), Some(21));
    }

    #[test]
    fn release_file() {
        let t = "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"25.0.1\"\nOS_ARCH=\"x86_64\"\n";
        assert_eq!(parse_release(t), Some(("25.0.1".into(), "x86_64".into())));
    }

    #[test]
    fn finds_matching_home_only() {
        let dir = std::env::temp_dir().join(format!("mh-java-{}", std::process::id()));
        let arch = std::env::consts::ARCH;
        let make = |name: &str, ver: &str, arch: &str| {
            let home = dir.join(name);
            std::fs::create_dir_all(home.join("bin")).unwrap();
            std::fs::write(
                home.join("release"),
                format!("JAVA_VERSION=\"{ver}\"\nOS_ARCH=\"{arch}\"\n"),
            )
            .unwrap();
            let exe = if cfg!(windows) { "javaw.exe" } else { "java" };
            std::fs::write(home.join("bin").join(exe), "").unwrap();
            home
        };
        let good = make("good", "25.0.1", arch);
        let old = make("old", "21.0.5", arch);
        let other = if arch == "aarch64" {
            "x86_64"
        } else {
            "aarch64"
        };
        let foreign = make("foreign", "25.0.1", other);

        let found = check_home(&good, 25).unwrap();
        assert_eq!(found.version, "25.0.1");
        assert!(check_home(&old, 25).is_none());
        assert!(check_home(&foreign, 25).is_none());
        assert!(check_home(&dir.join("missing"), 25).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
