use clipster_server::{retention, routes, setup, state};

use clap::Parser;
use clipster_common::config::ServerConfig;
use clipster_core::ClipsterCore;
use clipster_core::db::Database;
use clipster_core::discovery::{Announcer, Browser};
use clipster_core::identity::Identity;
use clipster_core::peer::server::router as peer_router;
use clipster_core::sync::SyncEngine;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "clipster-server", about = "Clipster clipboard sync server")]
struct Cli {
    /// Path to config file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Bind address (overrides config)
    #[arg(short, long)]
    bind: Option<String>,

    /// Enable TLS (auto-generates self-signed cert if needed)
    #[arg(long)]
    tls: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Run the server (default)
    Run,
    /// Initial setup: generate API key, write config, print client params
    Setup {
        /// Enable TLS in generated config
        #[arg(long)]
        tls: bool,
    },
    /// Install as system daemon (launchd/systemd/Windows task)
    Install,
    /// Uninstall the system daemon
    Uninstall,
    /// Check daemon status
    Status,
    /// Manage peer trust
    Peers {
        #[command(subcommand)]
        action: PeersAction,
    },
}

#[derive(clap::Subcommand)]
enum PeersAction {
    /// List all known peers
    List,
    /// Trust a peer by its device_id (SHA-256 fingerprint hex)
    Trust { device_id: String },
    /// Reject (block) a peer
    Reject { device_id: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Run) {
        Command::Setup { tls } => {
            return setup::setup(cli.bind.as_deref(), tls);
        }
        Command::Install => {
            return setup::install(cli.config.as_deref());
        }
        Command::Uninstall => {
            return setup::uninstall();
        }
        Command::Status => {
            return setup::status();
        }
        Command::Peers { action } => {
            return peers_command(cli.config.as_deref(), action);
        }
        Command::Run => {}
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = load_config(cli.config.as_deref())?;
    let bind = cli.bind.unwrap_or(config.bind.clone());
    let use_tls = cli.tls || config.tls;

    let data_dir = data_dir(&config);
    std::fs::create_dir_all(&data_dir)?;

    let image_dir = config
        .image_dir
        .clone()
        .unwrap_or_else(|| data_dir.join("images").to_string_lossy().to_string());
    std::fs::create_dir_all(&image_dir)?;

    let db_path = config
        .db_path
        .clone()
        .unwrap_or_else(|| data_dir.join("clipster.db").to_string_lossy().to_string());

    let db = Database::open(&db_path)?;
    db.migrate()?;

    let identity = Identity::load_or_create(&data_dir, None)?;
    let core = Arc::new(ClipsterCore::new(
        db,
        identity,
        PathBuf::from(&image_dir),
    ));

    let app_state = state::AppState::new(core.clone(), config.api_key.clone());

    retention::spawn(
        core.db.clone(),
        PathBuf::from(&image_dir),
        config.retention_days,
    );

    // Start mDNS discovery + announce
    let port: u16 = bind
        .rsplit(':')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8743);
    let _announcer = match Announcer::start(
        &core.identity.device_name,
        port,
        &core.identity.device_id,
        &core.identity.device_name,
        env!("CARGO_PKG_VERSION"),
        &["web_ui", "images"],
    ) {
        Ok(a) => Some(a),
        Err(e) => {
            tracing::warn!(error = %e, "mDNS announce failed; continuing without LAN discovery");
            None
        }
    };
    let browser = match Browser::start(core.identity.device_id.clone()) {
        Ok(b) => Some(b),
        Err(e) => {
            tracing::warn!(error = %e, "mDNS browse failed; LAN peer discovery disabled");
            None
        }
    };

    // Sync engine
    let sync_engine = SyncEngine::spawn(core.clone());
    if let Some(b) = &browser {
        sync_engine.attach_discovery(b.subscribe());
    }

    // Combine web/admin app router + peer router (peer router has its own auth)
    let app = routes::router(app_state).merge(peer_router(core.clone()));

    let listener = tokio::net::TcpListener::bind(&bind).await?;

    if use_tls {
        let acceptor = core.identity.tls_acceptor()?;
        tracing::info!("Clipster server listening on https://{bind} (device {})", core.identity.device_id);
        loop {
            let (stream, _addr) = listener.accept().await?;
            let acceptor = acceptor.clone();
            let app = app.clone();
            tokio::spawn(async move {
                match acceptor.accept(stream).await {
                    Ok(tls_stream) => {
                        let io = hyper_util::rt::TokioIo::new(tls_stream);
                        let service = hyper_util::service::TowerToHyperService::new(app);
                        if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                            hyper_util::rt::TokioExecutor::new(),
                        )
                        .serve_connection(io, service)
                        .await
                        {
                            tracing::debug!("connection error: {e}");
                        }
                    }
                    Err(e) => {
                        tracing::debug!("TLS handshake failed: {e}");
                    }
                }
            });
        }
    } else {
        tracing::info!("Clipster server listening on http://{bind} (device {})", core.identity.device_id);
        axum::serve(listener, app).await?;
    }

    // Unreachable in current control flow but keeps types tidy
    #[allow(unreachable_code)]
    Ok(())
}

fn peers_command(config_path: Option<&std::path::Path>, action: PeersAction) -> anyhow::Result<()> {
    let config = load_config(config_path)?;
    let data_dir = data_dir(&config);
    let db_path = config
        .db_path
        .clone()
        .unwrap_or_else(|| data_dir.join("clipster.db").to_string_lossy().to_string());
    let db = Database::open(&db_path)?;
    db.migrate()?;

    match action {
        PeersAction::List => {
            let peers = db.list_peers()?;
            if peers.is_empty() {
                println!("No peers known yet.");
                return Ok(());
            }
            println!("{:<10} {:<24} {}", "STATUS", "NAME", "DEVICE_ID");
            println!("{:-<80}", "");
            for p in peers {
                println!(
                    "{:<10} {:<24} {}",
                    p.trust_status.to_string(),
                    truncate(&p.name, 24),
                    p.device_id
                );
                if let Some(addr) = &p.last_addr {
                    println!("           addr: {addr}");
                }
                if let Some(seen) = &p.last_seen {
                    println!("           last seen: {}", seen.to_rfc3339());
                }
            }
        }
        PeersAction::Trust { device_id } => {
            db.set_peer_trust(&device_id, clipster_core::db::peers::TrustStatus::Trusted)?;
            println!("Peer {device_id} trusted");
        }
        PeersAction::Reject { device_id } => {
            db.set_peer_trust(&device_id, clipster_core::db::peers::TrustStatus::Rejected)?;
            println!("Peer {device_id} rejected");
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

fn load_config(path: Option<&std::path::Path>) -> anyhow::Result<ServerConfig> {
    if let Some(p) = path {
        let content = std::fs::read_to_string(p)?;
        Ok(toml::from_str(&content)?)
    } else {
        let default_path = config_dir().join("server.toml");
        if default_path.exists() {
            let content = std::fs::read_to_string(&default_path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(ServerConfig::default())
        }
    }
}

fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "clipster", "clipster")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn data_dir(config: &ServerConfig) -> PathBuf {
    if config.db_path.is_some() {
        config
            .db_path
            .as_ref()
            .and_then(|p| std::path::Path::new(p).parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        directories::ProjectDirs::from("com", "clipster", "clipster")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }
}
