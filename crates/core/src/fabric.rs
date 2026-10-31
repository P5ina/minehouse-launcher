//! Fabric loader profile from meta.fabricmc.net, merged over the vanilla version.

use anyhow::Result;

use crate::http;
use crate::minecraft::{library_key, VersionJson};
use crate::paths::Paths;

pub const META: &str = "https://meta.fabricmc.net/v2";

pub fn profile_id(minecraft: &str, loader: &str) -> String {
    format!("fabric-loader-{loader}-{minecraft}")
}

pub async fn fetch_profile(
    client: &reqwest::Client,
    paths: &Paths,
    minecraft: &str,
    loader: &str,
) -> Result<VersionJson> {
    let id = profile_id(minecraft, loader);
    let url = format!("{META}/versions/loader/{minecraft}/{loader}/profile/json");
    let cache = paths.version_dir(&id).join(format!("{id}.json"));
    http::get_json_cached(client, &url, &cache).await
}

/// Child (Fabric) over parent (vanilla): child's main class, arguments
/// appended, libraries first with duplicates of the same artifact dropped
/// from the parent (Fabric ships newer ASM etc.).
pub fn merge(parent: VersionJson, child: VersionJson) -> VersionJson {
    let mut libraries = child.libraries;
    let mut seen: std::collections::HashSet<String> =
        libraries.iter().map(|l| library_key(&l.name)).collect();
    for lib in parent.libraries {
        if seen.insert(library_key(&lib.name)) {
            libraries.push(lib);
        }
    }
    let mut arguments = parent.arguments;
    arguments.game.extend(child.arguments.game);
    arguments.jvm.extend(child.arguments.jvm);
    arguments
        .default_user_jvm
        .extend(child.arguments.default_user_jvm);
    VersionJson {
        id: child.id,
        inherits_from: None,
        main_class: child.main_class,
        arguments,
        libraries,
        asset_index: child.asset_index.or(parent.asset_index),
        downloads: parent.downloads,
        java_version: child.java_version.or(parent.java_version),
        logging: child.logging.or(parent.logging),
        kind: parent.kind,
    }
}
