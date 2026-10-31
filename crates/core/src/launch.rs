//! Builds the java command line from the merged version JSON and starts the game.

use std::collections::HashMap;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::auth::Session;
use crate::install::Installed;
use crate::paths::Paths;
use crate::progress::Reporter;
use crate::rules::Env;
use crate::settings::Settings;

fn substitute(arg: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        match rest[start..].find('}') {
            Some(end) => {
                let key = &rest[start + 2..start + end];
                match vars.get(key) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + end + 1]),
                }
                rest = &rest[start + end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The full argument list after `java`.
pub fn build_args(
    paths: &Paths,
    inst: &Installed,
    session: &Session,
    settings: &Settings,
) -> Vec<String> {
    let env = Env::current().with_feature("is_quick_play_multiplayer", true);
    let sep = if cfg!(windows) { ";" } else { ":" };
    let classpath = inst
        .classpath
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(sep);
    let natives = paths.natives(&inst.version.id);
    let s = |p: std::path::PathBuf| p.to_string_lossy().into_owned();

    let vars: HashMap<&str, String> = HashMap::from([
        ("auth_player_name", session.name.clone()),
        ("version_name", inst.version.id.clone()),
        ("game_directory", s(paths.game_dir())),
        ("assets_root", s(paths.assets())),
        (
            "assets_index_name",
            inst.version
                .asset_index
                .as_ref()
                .map(|a| a.id.clone())
                .unwrap_or_default(),
        ),
        ("auth_uuid", session.uuid.clone()),
        ("auth_access_token", session.access_token.clone()),
        ("clientid", String::new()),
        ("auth_xuid", session.xuid.clone()),
        ("user_type", "msa".into()),
        ("user_properties", "{}".into()),
        ("version_type", "MineHouse".into()),
        ("natives_directory", s(natives)),
        ("launcher_name", crate::LAUNCHER_NAME.into()),
        ("launcher_version", crate::LAUNCHER_VERSION.into()),
        ("classpath", classpath),
        ("classpath_separator", sep.into()),
        ("library_directory", s(paths.libraries())),
        ("quickPlayMultiplayer", inst.server.clone()),
        (
            "quickPlayPath",
            s(paths.game_dir().join("quickPlay").join("log.json")),
        ),
    ]);

    let mut args = Vec::new();
    // Mojang's suggested GC flags, our memory instead of their -Xms/-Xmx.
    for a in &inst.version.arguments.default_user_jvm {
        for v in a.resolve(&env) {
            if !v.starts_with("-Xmx") && !v.starts_with("-Xms") {
                args.push(v);
            }
        }
    }
    args.push(format!("-Xms{}M", settings.memory_mb.min(2048)));
    args.push(format!("-Xmx{}M", settings.memory_mb));
    args.extend(
        settings
            .extra_jvm_args
            .split_whitespace()
            .map(str::to_owned),
    );
    if let Some(log) = &inst.log_arg {
        args.push(log.clone());
    }
    for a in &inst.version.arguments.jvm {
        args.extend(a.resolve(&env).iter().map(|v| substitute(v, &vars)));
    }
    args.push(inst.version.main_class.clone());
    for a in &inst.version.arguments.game {
        args.extend(a.resolve(&env).iter().map(|v| substitute(v, &vars)));
    }
    args
}

/// Starts the game and streams its output to the reporter. Resolves when the game exits.
pub async fn launch(
    paths: &Paths,
    inst: &Installed,
    session: &Session,
    settings: &Settings,
    reporter: &Reporter,
) -> Result<std::process::ExitStatus> {
    let args = build_args(paths, inst, session, settings);
    tokio::fs::create_dir_all(paths.game_dir()).await?;
    tokio::fs::create_dir_all(paths.natives(&inst.version.id)).await?;

    let mut cmd = tokio::process::Command::new(&inst.java);
    cmd.args(&args)
        .current_dir(paths.game_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    reporter.log(format!("Запуск: {}", inst.java.display()));
    let mut child = cmd
        .spawn()
        .with_context(|| format!("start {}", inst.java.display()))?;

    let token = session.access_token.clone();
    let pipe = |r: Box<dyn tokio::io::AsyncRead + Unpin + Send>| {
        let reporter = reporter.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(r).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                reporter.log(line.replace(&token, "<token>"));
            }
        })
    };
    let out = pipe(Box::new(child.stdout.take().unwrap()));
    let err = pipe(Box::new(child.stderr.take().unwrap()));
    let status = child.wait().await?;
    let _ = tokio::join!(out, err);
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_known_vars_only() {
        let vars = HashMap::from([("a", "1".to_string()), ("b", "2".to_string())]);
        assert_eq!(substitute("x${a}y${b}z", &vars), "x1y2z");
        assert_eq!(substitute("${nope}", &vars), "${nope}");
        assert_eq!(substitute("-Dp=${a}/java", &vars), "-Dp=1/java");
        assert_eq!(substitute("tail ${", &vars), "tail ${");
    }
}
