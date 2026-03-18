#[cfg(target_os = "macos")]
mod launchd;
mod sync;
mod watcher;

use clap::Parser;
use clipster_common::config::ClientConfig;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "clipster-agent", about = "Clipster clipboard watching daemon")]
struct Cli {
    /// Path to config file
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Server URL (overrides config)
    #[arg(short, long)]
    server: Option<String>,

    /// Accept self-signed TLS certificates
    #[arg(short = 'k', long)]
    insecure: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Run the agent (default)
    Run,
    /// Show current config
    ShowConfig,
    /// Install as a launchd agent (macOS)
    Install,
    /// Uninstall the launchd agent (macOS)
    Uninstall,
    /// Check if the launchd agent is running (macOS)
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let mut config = load_config(cli.config.as_deref())?;
    if let Some(url) = cli.server {
        config.server_url = url;
    }
    if cli.insecure {
        config.insecure = true;
    }

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => {
            tracing::info!(
                device = %config.device_name,
                server = %config.server_url,
                "starting clipster agent"
            );
            let client = sync::SyncClient::new(&config);
            watcher::run(client).await?;
        }
        Command::ShowConfig => {
            println!("{}", toml::to_string_pretty(&config)?);
        }
        #[cfg(target_os = "macos")]
        Command::Install => launchd::install(cli.config.as_deref())?,
        #[cfg(target_os = "macos")]
        Command::Uninstall => launchd::uninstall()?,
        #[cfg(target_os = "macos")]
        Command::Status => launchd::status()?,
        #[cfg(not(target_os = "macos"))]
        Command::Install | Command::Uninstall | Command::Status => {
            anyhow::bail!("launchd commands are only supported on macOS");
        }
    }

    Ok(())
}

fn load_config(path: Option<&std::path::Path>) -> anyhow::Result<ClientConfig> {
    if let Some(p) = path {
        let content = std::fs::read_to_string(p)?;
        Ok(toml::from_str(&content)?)
    } else {
        let default_path = config_dir().join("client.toml");
        if default_path.exists() {
            let content = std::fs::read_to_string(&default_path)?;
            Ok(toml::from_str(&content)?)
        } else {
            tracing::warn!("no config file found at {}, using defaults", default_path.display());
            Ok(ClientConfig::default())
        }
    }
}

fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "clipster", "clipster")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}
