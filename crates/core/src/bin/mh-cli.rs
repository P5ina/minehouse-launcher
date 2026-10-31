//! Headless driver for CI and debugging:
//!   mh-cli install         download/repair everything, print the java command
//!   mh-cli ping            server status
//!   mh-cli login           Microsoft sign-in in the terminal, then play
use anyhow::Result;
use minehouse_core::{
    auth, http, install, launch, paths::Paths, ping, progress::*, settings::Settings,
};

#[tokio::main]
async fn main() -> Result<()> {
    let paths = Paths::default_location()?;
    let reporter = Reporter::new(|e| match e {
        Event::Stage { name } => eprintln!("== {name}"),
        Event::Log { line } => eprintln!("{line}"),
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
        _ => eprintln!("usage: mh-cli install | ping | login"),
    }
    Ok(())
}
