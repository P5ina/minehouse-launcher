//! Headless driver for CI and debugging:
//!   mh-cli install         download/repair everything, print the java command
//!   mh-cli horizon         download/prepare Horizon, print its command line
//!   mh-cli ping            server status
//!   mh-cli login           Microsoft sign-in in the terminal, then play
//!   mh-cli mods [list | search <q> | add <project> | jar <path> | rm <file> | update | sync]
//!   mh-cli instances [list | new <name> | use <id> | rm <id> | dir <path>]
//!   mh-cli java [major]   which installed Java the launcher would use
//!   mh-cli packs <resource|shader> [list | search <q> | add <project> | zip <path> | on|off <file> | rm <file> | update]
use anyhow::Result;
use minehouse_core::{
    auth, http, install, launch, modpack, modrinth, packs, paths::Paths, ping, progress::*,
    settings::Settings, user_mods,
};

#[tokio::main]
async fn main() -> Result<()> {
    let paths = Paths::default_location()?;
    let reporter = Reporter::new(|e| match e {
        Event::Stage { name } => eprintln!("== {name}"),
        Event::Log { line } => eprintln!("{line}"),
        Event::Game(g) => eprintln!("[{}] [{}] {}", g.level, g.logger, g.message),
        Event::Progress { .. } => {}
    });
    match std::env::args().nth(1).as_deref() {
        Some("install") => {
            let inst = install::install(&paths, &reporter).await?;
            let out = std::process::Command::new(&inst.java)
                .arg("-version")
                .output()?;
            eprintln!("{}", String::from_utf8_lossy(&out.stderr));
            anyhow::ensure!(out.status.success(), "java -version failed");
            for p in &inst.classpath {
                anyhow::ensure!(p.exists(), "missing {}", p.display());
            }
            let fake = auth::Session {
                name: "Player".into(),
                uuid: "00000000000000000000000000000000".into(),
                xuid: "0".into(),
                access_token: "<token>".into(),
                expires_at: 0,
            };
            let args = launch::build_args(&paths, &inst, &fake, &Settings::load(&paths));
            println!("{} {}", inst.java.display(), args.join(" "));
            eprintln!(
                "OK: {} files on classpath, pack {}",
                inst.classpath.len(),
                inst.pack_version
            );
        }
        Some("horizon") => {
            let inst = minehouse_core::horizon::install(&paths, &reporter).await?;
            let args = minehouse_core::horizon::build_args(&inst, &paths.game_dir(), None);
            println!("{} {}", inst.client.display(), args.join(" "));
        }
        Some("ping") => println!(
            "{:?}",
            ping::ping(minehouse_core::SERVER_HOST, minehouse_core::SERVER_PORT).await?
        ),
        Some("login") => {
            let client = http::client();
            let code = auth::start_device_code(&client).await?;
            eprintln!(
                "Open {} and enter {}",
                code.verification_uri, code.user_code
            );
            let account = auth::finish_device_code(&client, &code).await?;
            eprintln!("Signed in as {}", account.session.name);
            let inst = install::install(&paths, &reporter).await?;
            launch::launch(
                &paths,
                &inst,
                &account.session,
                &Settings::load(&paths),
                &reporter,
            )
            .await?;
        }
        Some("mods") => mods(&paths).await?,
        Some("packs") => content_packs(&paths).await?,
        Some("instances") => instances_cmd()?,
        Some("java") => {
            let major: u32 = std::env::args().nth(2).as_deref().unwrap_or("25").parse()?;
            match minehouse_core::java::find_system(major) {
                Some(j) => println!("Java {} → {}", j.version, j.java.display()),
                None => println!("Java {major} на компьютере не найдена, лаунчер скачает свою"),
            }
        }
        _ => {
            eprintln!("usage: mh-cli install | horizon | ping | login | mods ... | packs ... | instances ...")
        }
    }
    Ok(())
}

fn instances_cmd() -> Result<()> {
    let mut c = minehouse_core::instances::Config::load(&Paths::default_root()?);
    let args: Vec<String> = std::env::args().skip(2).collect();
    let arg = || args.get(1).cloned().unwrap_or_default();
    match args.first().map(String::as_str).unwrap_or("list") {
        "list" => {}
        "new" => drop(c.create(
            &args[1..].join(" "),
            minehouse_core::instances::Kind::Fabric,
        )?),
        "use" => c.select(&arg())?,
        "rm" => c.remove(&arg())?,
        "dir" => drop(c.set_data_dir(std::path::Path::new(&arg()), &|s| eprintln!("== {s}"))?),
        other => anyhow::bail!("unknown: instances {other}"),
    }
    let c = minehouse_core::instances::Config::load(c.root());
    eprintln!("data: {}", c.data_dir().display());
    for i in &c.instances {
        let mark = if i.id == c.active { "*" } else { " " };
        println!("{mark} {:<20} {:<24} {}", i.id, i.name, i.dir.display());
    }
    Ok(())
}

async fn content_packs(paths: &Paths) -> Result<()> {
    let client = http::client();
    let args: Vec<String> = std::env::args().skip(2).collect();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let kind = packs::Kind::parse(&arg(0))?;
    let mc = modpack::fetch_manifest(&client, paths)
        .await?
        .minecraft
        .unwrap_or_else(|| minehouse_core::DEFAULT_MINECRAFT.into());
    match arg(1).as_str() {
        "" | "list" => {
            for p in packs::list(paths, kind)? {
                println!(
                    "{} {} — {} {} [{}]",
                    if p.enabled { "+" } else { "-" },
                    p.file,
                    p.title,
                    p.version,
                    p.project.unwrap_or_else(|| "zip".into())
                );
            }
        }
        "search" => {
            let (hits, total) = modrinth::search_content(
                &client,
                kind.project_type(),
                kind.category(),
                &arg(2),
                &mc,
                0,
            )
            .await?;
            println!("{total} found");
            for h in hits {
                println!("{} {}", h.project_id, h.title);
            }
        }
        "add" => println!(
            "{}",
            packs::install_modrinth(&client, paths, kind, &arg(2), None, &mc).await?
        ),
        "zip" => {
            let (k, t) = packs::add_local(&client, paths, std::path::Path::new(&arg(2))).await?;
            println!("{k:?}: {t}");
        }
        "on" | "off" => packs::set_enabled(paths, kind, &arg(2), arg(1) == "on")?,
        "rm" => packs::remove(paths, kind, &arg(2))?,
        "update" => println!("{:?}", packs::update_all(&client, paths, kind, &mc).await?),
        other => anyhow::bail!("unknown packs command {other}"),
    }
    Ok(())
}

async fn mods(paths: &Paths) -> Result<()> {
    let client = http::client();
    let manifest = modpack::fetch_manifest(&client, paths).await?;
    let pack = user_mods::Pack::load(&manifest, paths);
    let args: Vec<String> = std::env::args().skip(2).collect();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    match arg(0).as_str() {
        "" | "list" => {
            for m in user_mods::list(paths, &pack)?.mods {
                println!(
                    "{} {} {} [{}]{}",
                    if m.enabled { "+" } else { "-" },
                    m.file,
                    m.version,
                    m.project.unwrap_or_else(|| "jar".into()),
                    m.note.map(|n| format!(" ({n})")).unwrap_or_default()
                );
            }
        }
        "search" => {
            let (hits, total) = modrinth::search(&client, &arg(1), &pack.minecraft, 0, &[]).await?;
            println!("{total} found");
            for h in hits {
                let v = pack.verdict(&h.project_id, &h.client_side, &h.server_side);
                println!("{} {} {:?}", h.project_id, h.title, v);
            }
        }
        "add" => println!(
            "installed: {:?}",
            user_mods::install_modrinth(&client, paths, &pack, &arg(1), None).await?
        ),
        "jar" => println!(
            "added: {}",
            user_mods::add_local(&client, paths, &pack, std::path::Path::new(&arg(1))).await?
        ),
        "rm" => user_mods::remove(paths, &pack, &arg(1))?,
        // What runs before every launch, without installing the game.
        "sync" => {
            let reporter = Reporter::new(|e| {
                if let Event::Log { line } | Event::Stage { name: line } = e {
                    eprintln!("{line}")
                }
            });
            user_mods::sync(&client, paths, &manifest, &reporter).await
        }
        // Optional pack mods: `mods pack` lists them, `mods pack <id> on|off` switches one.
        "pack" if arg(1).is_empty() => {
            for o in user_mods::list(paths, &pack)?.options {
                println!(
                    "{} {} {}{}",
                    if o.enabled { "+" } else { "-" },
                    o.id,
                    o.name,
                    if o.missing.is_empty() {
                        String::new()
                    } else {
                        format!(" (нужен {})", o.missing.join(", "))
                    }
                );
            }
        }
        "pack" => {
            let reporter = Reporter::new(|e| {
                if let Event::Log { line } = e {
                    eprintln!("{line}")
                }
            });
            user_mods::set_pack_option(
                &client,
                paths,
                &manifest,
                &arg(1),
                arg(2) != "off",
                &reporter,
            )
            .await?
        }
        "update" => println!(
            "updated: {:?}",
            user_mods::update_all(&client, paths, &pack).await?
        ),
        other => anyhow::bail!("unknown mods command {other}"),
    }
    Ok(())
}
