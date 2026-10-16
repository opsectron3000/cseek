//! cseekd: headless Soulseek daemon.

mod api;
mod auth;
mod logs;
mod media;

use anyhow::{bail, Context, Result};
use cseek_core::{Client, Config};
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::prelude::*;

const USAGE: &str = "cseekd [--config PATH] [--data DIR] [--web DIR] [--port PORT]

  --config PATH   config file (env CSEEK_CONFIG, default <data>/config.toml)
  --data DIR      data directory: database, default downloads (env CSEEK_DATA, default ~/.cseek)
  --web DIR       built web UI to serve (env CSEEK_WEB, default ./web/dist if present)
  --port PORT     override web.port";

struct Args {
    config: Option<PathBuf>,
    data: Option<PathBuf>,
    web: Option<PathBuf>,
    port: Option<u16>,
}

fn parse_args() -> Result<Args> {
    let mut a = Args {
        config: std::env::var_os("CSEEK_CONFIG").map(PathBuf::from),
        data: std::env::var_os("CSEEK_DATA").map(PathBuf::from),
        web: std::env::var_os("CSEEK_WEB").map(PathBuf::from),
        port: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
            None => (arg.clone(), None),
        };
        let mut val = || inline.clone().or_else(|| it.next()).with_context(|| format!("{key} needs a value"));
        match key.as_str() {
            "--config" | "-c" => a.config = Some(val()?.into()),
            "--data" | "-d" => a.data = Some(val()?.into()),
            "--web" => a.web = Some(val()?.into()),
            "--port" | "-p" => a.port = Some(val()?.parse().context("--port")?),
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("cseekd {}", cseek_core::VERSION);
                std::process::exit(0);
            }
            _ => bail!("unknown argument {arg}\n\n{USAGE}"),
        }
    }
    Ok(a)
}

fn random_password() -> String {
    const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    (0..16).map(|_| CHARS[rand::random_range(0..CHARS.len())] as char).collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;
    let logs = logs::LogBuffer::new();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,cseek_core=info,cseekd=info,tower_http=warn".into());
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_target(false).with_filter(filter))
        .with(logs::BufferLayer { buffer: logs.clone() })
        .init();

    let data = args.data.clone().unwrap_or_else(|| cseek_core::config::expand_home("~/.cseek"));
    std::fs::create_dir_all(&data).with_context(|| format!("creating {}", data.display()))?;
    let config_path = args.config.clone().unwrap_or_else(|| data.join("config.toml"));
    let mut config = Config::load(&config_path).with_context(|| format!("loading {}", config_path.display()))?;
    let existed = config_path.exists();
    if !existed || (config.web.auth.password.is_empty() && !config.web.auth.disabled) {
        config.web.auth.password = random_password();
        config.save(&config_path)?;
        tracing::warn!(
            "{} {} — web login is {:?} / {:?}; change it in Options",
            if existed { "generated a web password in" } else { "created" },
            config_path.display(),
            config.web.auth.username,
            config.web.auth.password
        );
    }
    if let Some(p) = args.port {
        config.web.port = p;
    }
    let (host, port) = (config.web.host.clone(), config.web.port);
    let base = config_path.parent().map(PathBuf::from).unwrap_or_else(|| data.clone());

    let client = Client::new(config, config_path.clone(), base)?;
    client.start().await?;

    let ctx = api::Ctx { client: client.clone(), logs };
    let mut app = api::router(ctx);
    let web = args.web.or_else(|| Some(PathBuf::from("web/dist")).filter(|p| p.join("index.html").exists()));
    match web {
        Some(dir) if dir.join("index.html").exists() => {
            tracing::info!("serving web UI from {}", dir.display());
            app = app.fallback_service(ServeDir::new(&dir).fallback(ServeFile::new(dir.join("index.html"))));
        }
        Some(dir) => tracing::warn!("no index.html in {}; web UI disabled", dir.display()),
        None => tracing::info!("no web UI directory; API only"),
    }

    let listener = tokio::net::TcpListener::bind((host.as_str(), port))
        .await
        .with_context(|| format!("binding web port {host}:{port}"))?;
    tracing::info!("web/API listening on http://{host}:{port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    tracing::info!("shutting down");
    client.persist_active_transfers();
    client.disconnect();
    Ok(())
}
