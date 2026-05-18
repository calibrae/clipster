//! Headless Clipster peer. Watches the local clipboard, runs mDNS discovery,
//! pulls from trusted peers — no central server required.
//!
//! This is the "fat-peer-without-a-GUI" mode, ideal for Linux servers or
//! always-on Mac minis.

#[cfg(target_os = "macos")]
mod launchd;
mod peer_listener;
mod watcher;

use clap::Parser;
use clipster_core::ClipsterCore;
use clipster_core::db::Database;
use clipster_core::db::peers::TrustStatus;
use clipster_core::discovery::{Announcer, Browser};
use clipster_core::identity::Identity;
use clipster_core::sync::SyncEngine;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "clipster-agent", about = "Clipster headless peer (clipboard watcher + LAN sync)")]
struct Cli {
    /// Data directory (default: platform-specific via `directories`)
    #[arg(short, long)]
    data_dir: Option<PathBuf>,

    /// Peer port (overrides config). Default: 38744.
    #[arg(short, long)]
    port: Option<u16>,

    /// Human-friendly device name shown to peers.
    #[arg(long)]
    name: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Run the agent (default)
    Run,
    /// Print this device's identity (device_id + name)
    Whoami,
    /// List known peers
    Peers,
    /// Trust a peer by device_id
    Trust { device_id: String },
    /// Reject (block) a peer by device_id
    Reject { device_id: String },
    /// Install as a launchd agent (macOS)
    #[cfg(target_os = "macos")]
    Install,
    /// Uninstall the launchd agent (macOS)
    #[cfg(target_os = "macos")]
    Uninstall,
    /// Check if the launchd agent is running (macOS)
    #[cfg(target_os = "macos")]
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let data_dir = cli.data_dir.clone().unwrap_or_else(default_data_dir);
    let port = cli.port.unwrap_or(38744);

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run(data_dir, port, cli.name).await,
        Command::Whoami => whoami(data_dir, cli.name),
        Command::Peers => peers_list(data_dir),
        Command::Trust { device_id } => peers_trust(data_dir, &device_id, TrustStatus::Trusted),
        Command::Reject { device_id } => peers_trust(data_dir, &device_id, TrustStatus::Rejected),
        #[cfg(target_os = "macos")]
        Command::Install => launchd::install(None),
        #[cfg(target_os = "macos")]
        Command::Uninstall => launchd::uninstall(),
        #[cfg(target_os = "macos")]
        Command::Status => launchd::status(),
    }
}

async fn run(data_dir: PathBuf, port: u16, name: Option<String>) -> anyhow::Result<()> {
    let core = init_core(&data_dir, name)?;

    tracing::info!(
        device_id = %core.identity.device_id,
        device_name = %core.identity.device_name,
        port,
        "clipster-agent peer starting"
    );

    // mDNS announce + browse
    let _announcer = Announcer::start(
        &core.identity.device_name,
        port,
        &core.identity.device_id,
        &core.identity.device_name,
        env!("CARGO_PKG_VERSION"),
        &["agent", "images"],
    )
    .ok();
    let browser = Browser::start(core.identity.device_id.clone()).ok();

    // Sync engine
    let sync_engine = SyncEngine::spawn(core.clone());
    if let Some(b) = &browser {
        sync_engine.attach_discovery(b.subscribe());
    }
    drop(sync_engine);
    std::mem::forget(browser);
    std::mem::forget(_announcer);

    // Peer-facing TLS listener
    let core2 = core.clone();
    tokio::spawn(async move {
        if let Err(e) = peer_listener::run(core2, port).await {
            tracing::error!(error = %e, "peer listener exited");
        }
    });

    // Clipboard watcher (foreground)
    watcher::run(core).await
}

fn init_core(data_dir: &std::path::Path, name: Option<String>) -> anyhow::Result<Arc<ClipsterCore>> {
    std::fs::create_dir_all(data_dir)?;
    let image_dir = data_dir.join("images");
    std::fs::create_dir_all(&image_dir)?;
    let db = Database::open(data_dir.join("clipster.db").to_str().unwrap())?;
    db.migrate()?;
    let identity = Identity::load_or_create(data_dir, name)?;
    Ok(Arc::new(ClipsterCore::new(db, identity, image_dir)))
}

fn whoami(data_dir: PathBuf, name: Option<String>) -> anyhow::Result<()> {
    let core = init_core(&data_dir, name)?;
    println!("device_id:   {}", core.identity.device_id);
    println!("device_name: {}", core.identity.device_name);
    Ok(())
}

fn peers_list(data_dir: PathBuf) -> anyhow::Result<()> {
    let core = init_core(&data_dir, None)?;
    let peers = core.db.list_peers()?;
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
    }
    Ok(())
}

fn peers_trust(data_dir: PathBuf, device_id: &str, status: TrustStatus) -> anyhow::Result<()> {
    let core = init_core(&data_dir, None)?;
    core.db.set_peer_trust(device_id, status)?;
    println!("Peer {device_id} -> {status}");
    Ok(())
}

fn default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "clipster", "clipster-agent")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("./clipster-agent-data"))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}
