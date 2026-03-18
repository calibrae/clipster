use clipster_server::{db, routes, setup, state, tls};

use clap::Parser;
use clipster_common::config::ServerConfig;
use std::path::PathBuf;
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

    let db = db::Database::open(&db_path)?;
    db.migrate()?;

    let app_state = state::AppState::new(db, image_dir, config.api_key.clone());
    let app = routes::router(app_state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;

    if use_tls {
        let acceptor = tls::setup(
            &data_dir,
            config.tls_cert.as_deref(),
            config.tls_key.as_deref(),
        )?;

        tracing::info!("Clipster server listening on https://{bind}");

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
        tracing::info!("Clipster server listening on http://{bind}");
        axum::serve(listener, app).await?;
    }

    Ok(())
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
