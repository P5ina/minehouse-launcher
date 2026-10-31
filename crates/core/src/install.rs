//! One call that makes the instance ready to play: modpack manifest →
//! Minecraft + Fabric versions → Java → client jar, libraries, assets → mods.
//! Re-running it is the update check: valid files are skipped.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::minecraft::{self, VersionJson};
use crate::paths::Paths;
use crate::progress::Reporter;
use crate::rules::Env;
use crate::{fabric, http, java, modpack};

/// Everything [`crate::launch`] needs.
#[derive(Clone, Debug)]
pub struct Installed {
    pub version: VersionJson,
    pub minecraft: String,
    pub java: PathBuf,
    pub classpath: Vec<PathBuf>,
    pub log_arg: Option<String>,
    pub pack_version: String,
    pub server: String,
}

pub async fn install(paths: &Paths, reporter: &Reporter) -> Result<Installed> {
    let client = http::client();
    let env = Env::current();

    reporter.stage("Проверяем обновления сборки");
    let manifest = modpack::fetch_manifest(&client, paths).await?;
    let mc = manifest
        .minecraft
        .clone()
        .unwrap_or_else(|| crate::DEFAULT_MINECRAFT.into());
    let loader = manifest
        .loader_version
        .clone()
        .unwrap_or_else(|| crate::DEFAULT_FABRIC_LOADER.into());
    reporter.log(format!(
        "Сборка {} · Minecraft {mc} · Fabric {loader}",
        manifest.version
    ));

    let vanilla = minecraft::fetch_version(&client, paths, &mc).await?;
    let profile = fabric::fetch_profile(&client, paths, &mc, &loader).await?;
    anyhow::ensure!(
        profile.inherits_from.as_deref() == Some(mc.as_str()),
        "Fabric profile is for {:?}, not {mc}",
        profile.inherits_from
    );
    let java_component = vanilla
        .java_version
        .as_ref()
        .map(|j| j.component.clone())
        .context("Minecraft version does not say which Java it needs")?;
    let client_jar = minecraft::client_jar(paths, &vanilla)?;
    let version = fabric::merge(vanilla, profile);

    reporter.stage("Скачиваем Java");
    let java = java::install(&client, paths, &java_component, reporter).await?;

    reporter.stage("Скачиваем Minecraft");
    let mut downloads = vec![client_jar.clone()];
    let mut classpath = Vec::new();
    for lib in &version.libraries {
        if let Some(d) = minecraft::library_download(lib, paths, &env)? {
            classpath.push(d.path.clone());
            downloads.push(d);
        }
    }
    let log = minecraft::log_config(paths, &version);
    if let Some((_, d)) = &log {
        downloads.push(d.clone());
    }
    http::fetch_all(&client, downloads, 16, reporter).await?;
    classpath.push(client_jar.path);

    reporter.stage("Скачиваем ресурсы игры");
    let assets = minecraft::asset_downloads(&client, paths, &version).await?;
    http::fetch_all(&client, assets, 32, reporter).await?;

    reporter.stage("Устанавливаем моды MineHouse");
    modpack::install(&client, paths, &manifest, reporter).await?;

    Ok(Installed {
        version,
        minecraft: mc,
        java,
        classpath,
        log_arg: log.map(|(arg, _)| arg),
        pack_version: manifest.version,
        server: manifest.server.unwrap_or_else(|| crate::SERVER.into()),
    })
}
