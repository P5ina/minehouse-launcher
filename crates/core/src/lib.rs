//! Everything the MineHouse launcher does besides drawing the window:
//! Microsoft sign-in, installing Java + Minecraft + Fabric + the modpack,
//! and starting the game straight into the MineHouse server.
//!
//! The Tauri app (`src-tauri`) and the `mh-cli` test binary are thin
//! wrappers around [`install::install`], [`auth`] and [`launch::launch`].

pub mod auth;
pub mod fabric;
pub mod http;
pub mod install;
pub mod java;
pub mod launch;
pub mod minecraft;
pub mod modpack;
pub mod paths;
pub mod ping;
pub mod progress;
pub mod rules;
pub mod settings;

/// Where the launcher reads the modpack manifest from (built by
/// `modpack/scripts/build.mjs` in P5ina/minehouse-landing).
pub const SITE: &str = "https://www.minehouse.pro";
pub const MANIFEST_URL: &str = "https://www.minehouse.pro/modpack/manifest.json";
/// Address with an explicit port: on the Pyro node's IP, port 25565 belongs to
/// someone else's server, so a client whose DNS drops the SRV record would land
/// there. The CNAME play → node.minehouse.pro makes host:port work without SRV.
pub const SERVER: &str = "play.minehouse.pro:25524";
pub const SERVER_HOST: &str = "play.minehouse.pro";
pub const SERVER_PORT: u16 = 25524;
/// Used when the site's manifest does not say which game version it wants.
pub const DEFAULT_MINECRAFT: &str = "26.3";
pub const DEFAULT_FABRIC_LOADER: &str = "0.20.1";

pub const LAUNCHER_NAME: &str = "minehouse-launcher";
pub const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");
