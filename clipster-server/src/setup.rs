use anyhow::{Context, Result};
use clipster_common::config::ServerConfig;
use std::path::{Path, PathBuf};

pub fn generate_api_key() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let a: u128 = rng.random();
    let b: u128 = rng.random();
    format!("clp_{}{}", base62::encode(a), base62::encode(b))
}

fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "clipster", "clipster")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "clipster", "clipster")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn local_ip() -> String {
    // Try to get a LAN IP
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

pub fn setup(bind: Option<&str>, tls: bool) -> Result<()> {
    #[cfg(unix)]
    let is_root = unsafe { libc::geteuid() } == 0;
    #[cfg(not(unix))]
    let is_root = false;

    let (config_path, data) = if is_root {
        (
            PathBuf::from("/etc/clipster/server.toml"),
            PathBuf::from("/var/lib/clipster"),
        )
    } else {
        (config_dir().join("server.toml"), data_dir())
    };

    // Generate API key
    let api_key = generate_api_key();
    let bind_addr = bind.unwrap_or("0.0.0.0:8743");

    let config = ServerConfig {
        bind: bind_addr.to_string(),
        db_path: Some(data.join("clipster.db").to_string_lossy().to_string()),
        image_dir: Some(data.join("images").to_string_lossy().to_string()),
        api_key: Some(api_key.clone()),
        tls,
        tls_cert: None,
        tls_key: None,
    };

    // Write config
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&data)?;
    std::fs::create_dir_all(data.join("images"))?;

    let toml_content = toml::to_string_pretty(&config)?;

    if config_path.exists() {
        println!("Config already exists at {}", config_path.display());
        println!("Overwrite? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    std::fs::write(&config_path, &toml_content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600))?;
    }

    let protocol = if tls { "https" } else { "http" };
    let ip = local_ip();
    let port = bind_addr.split(':').last().unwrap_or("8743");

    println!();
    println!("=== Clipster Server Setup Complete ===");
    println!();
    println!("Config:  {}", config_path.display());
    println!("Data:    {}", data.display());
    println!("Bind:    {bind_addr}");
    println!("TLS:     {tls}");
    println!();
    println!("--- Client Configuration ---");
    println!();
    println!("  server_url = \"{protocol}://{ip}:{port}\"");
    println!("  api_key = \"{api_key}\"");
    if tls {
        println!("  insecure = true");
    }
    println!();
    println!("--- Quick Start ---");
    println!();
    println!("  # Agent:");
    println!("  clipster-agent --server {protocol}://{ip}:{port} -k");
    println!();
    println!("  # CLI:");
    println!("  clipster --server {protocol}://{ip}:{port}");
    println!();
    println!("  # Tauri app: use Settings > Server URL");
    println!();

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn install(config_path: Option<&Path>) -> Result<()> {
    let plist_label = "com.clipster.server";
    let plist_filename = "com.clipster.server.plist";

    let exe = std::env::current_exe()?;
    let home = directories::BaseDirs::new()
        .context("could not determine home directory")?
        .home_dir()
        .to_path_buf();
    let plist_path = home.join("Library/LaunchAgents").join(plist_filename);
    let log_path = home.join("Library/Logs/clipster-server.log");

    let mut args = format!(
        "        <string>{}</string>\n",
        exe.display()
    );
    if let Some(cfg) = config_path {
        args.push_str(&format!(
            "        <string>--config</string>\n        <string>{}</string>\n",
            cfg.display()
        ));
    }

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{plist_label}</string>
    <key>ProgramArguments</key>
    <array>
{args}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
    <key>ProcessType</key>
    <string>Background</string>
</dict>
</plist>
"#,
        log = log_path.display()
    );

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&plist_path, &plist)?;

    let output = std::process::Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .output()
        .context("failed to run launchctl load")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("launchctl load failed: {stderr}");
    }

    println!("Installed and loaded {}", plist_path.display());
    println!("Logs: {}", log_path.display());
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn install(config_path: Option<&Path>) -> Result<()> {
    let exe = std::env::current_exe()?;
    let cfg = config_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| config_dir().join("server.toml"));
    let is_root = unsafe { libc::geteuid() } == 0;

    if is_root {
        install_linux_system(&exe, &cfg)
    } else {
        install_linux_user(&exe, &cfg)
    }
}

#[cfg(target_os = "linux")]
fn install_linux_system(exe: &Path, cfg: &Path) -> Result<()> {
    // Create clipster user if it doesn't exist
    let user_exists = std::process::Command::new("id")
        .args(["-u", "clipster"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !user_exists {
        let status = std::process::Command::new("useradd")
            .args(["--system", "--no-create-home", "--shell", "/usr/sbin/nologin", "clipster"])
            .status()
            .context("failed to create clipster user")?;
        if !status.success() {
            anyhow::bail!("useradd clipster failed");
        }
        println!("Created system user: clipster");
    }

    // Create data directories
    let data_dir = Path::new("/var/lib/clipster");
    std::fs::create_dir_all(data_dir.join("images"))?;
    // chown to clipster
    let _ = std::process::Command::new("chown")
        .args(["-R", "clipster:clipster", "/var/lib/clipster"])
        .status();

    // Write unit file
    let service_path = Path::new("/etc/systemd/system/clipster-server.service");
    let service = format!(
        "[Unit]\n\
         Description=Clipster Clipboard Sync Server\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         User=clipster\n\
         Group=clipster\n\
         ExecStart={exe} --config {cfg}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         WorkingDirectory=/var/lib/clipster\n\
         Environment=RUST_LOG=info\n\
         NoNewPrivileges=true\n\
         ProtectSystem=strict\n\
         ProtectHome=true\n\
         ReadWritePaths=/var/lib/clipster\n\
         PrivateTmp=true\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        exe = exe.display(),
        cfg = cfg.display(),
    );

    std::fs::write(service_path, &service)
        .with_context(|| format!("failed to write {}", service_path.display()))?;

    let status = std::process::Command::new("systemctl")
        .args(["daemon-reload"])
        .status()?;
    if !status.success() {
        anyhow::bail!("systemctl daemon-reload failed");
    }

    let status = std::process::Command::new("systemctl")
        .args(["enable", "--now", "clipster-server"])
        .status()?;
    if !status.success() {
        anyhow::bail!("systemctl enable --now failed");
    }

    println!("Installed system service: {}", service_path.display());
    println!("Status: systemctl status clipster-server");
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_linux_user(exe: &Path, cfg: &Path) -> Result<()> {
    let service_dir = directories::BaseDirs::new()
        .context("could not determine home directory")?
        .home_dir()
        .join(".config/systemd/user");
    std::fs::create_dir_all(&service_dir)?;

    let service_path = service_dir.join("clipster-server.service");
    let service = format!(
        "[Unit]\n\
         Description=Clipster Clipboard Sync Server\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exe} --config {cfg}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         Environment=RUST_LOG=info\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe = exe.display(),
        cfg = cfg.display(),
    );

    std::fs::write(&service_path, &service)
        .with_context(|| format!("failed to write {}", service_path.display()))?;

    let status = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()?;
    if !status.success() {
        anyhow::bail!("systemctl --user daemon-reload failed (is dbus-user-session installed?)");
    }

    let status = std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", "clipster-server"])
        .status()?;
    if !status.success() {
        anyhow::bail!("systemctl --user enable --now failed");
    }

    println!("Installed user service: {}", service_path.display());
    println!("Status: systemctl --user status clipster-server");
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn install(config_path: Option<&Path>) -> Result<()> {
    let exe = std::env::current_exe()?;
    let cfg = config_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| config_dir().join("server.toml"));

    let task_name = "ClipsterServer";
    let args = if cfg.exists() {
        format!("--config \"{}\"", cfg.display())
    } else {
        String::new()
    };

    let status = std::process::Command::new("schtasks")
        .args([
            "/Create",
            "/TN", task_name,
            "/TR", &format!("\"{}\" {}", exe.display(), args),
            "/SC", "ONLOGON",
            "/RL", "LIMITED",
            "/F",
        ])
        .status()
        .context("failed to run schtasks")?;

    if !status.success() {
        anyhow::bail!("schtasks /Create failed");
    }

    // Start it now
    let _ = std::process::Command::new("schtasks")
        .args(["/Run", "/TN", task_name])
        .status();

    println!("Installed scheduled task: {task_name}");
    println!("Runs at logon. To check: schtasks /Query /TN {task_name}");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let home = directories::BaseDirs::new()
            .context("could not determine home directory")?
            .home_dir()
            .to_path_buf();
        let plist_path = home.join("Library/LaunchAgents/com.clipster.server.plist");

        let _ = std::process::Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .output();

        if plist_path.exists() {
            std::fs::remove_file(&plist_path)?;
            println!("Uninstalled {}", plist_path.display());
        } else {
            println!("Not installed");
        }
    }

    #[cfg(target_os = "linux")]
    {
        let is_root = unsafe { libc::geteuid() } == 0;
        let system_path = std::path::Path::new("/etc/systemd/system/clipster-server.service");
        let user_path = directories::BaseDirs::new()
            .map(|d| d.home_dir().join(".config/systemd/user/clipster-server.service"))
            .unwrap_or_default();

        if is_root && system_path.exists() {
            let _ = std::process::Command::new("systemctl")
                .args(["disable", "--now", "clipster-server"])
                .status();
            std::fs::remove_file(system_path)?;
            let _ = std::process::Command::new("systemctl")
                .args(["daemon-reload"])
                .status();
            println!("Uninstalled system service");
        } else if user_path.exists() {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", "--now", "clipster-server"])
                .status();
            std::fs::remove_file(&user_path)?;
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status();
            println!("Uninstalled user service");
        } else {
            println!("Not installed");
        }
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("schtasks")
            .args(["/Delete", "/TN", "ClipsterServer", "/F"])
            .status();
        println!("Uninstalled ClipsterServer scheduled task");
    }

    Ok(())
}

pub fn status() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("launchctl")
            .args(["list", "com.clipster.server"])
            .output()
            .context("failed to run launchctl")?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            println!("Server is loaded:\n{stdout}");
        } else {
            println!("Server is not loaded");
        }
    }

    #[cfg(target_os = "linux")]
    {
        let is_root = unsafe { libc::geteuid() } == 0;
        let system_path = std::path::Path::new("/etc/systemd/system/clipster-server.service");

        if is_root || system_path.exists() {
            let status = std::process::Command::new("systemctl")
                .args(["status", "clipster-server", "--no-pager"])
                .status()?;
            if !status.success() {
                println!("System service is not running");
            }
        } else {
            let status = std::process::Command::new("systemctl")
                .args(["--user", "status", "clipster-server", "--no-pager"])
                .status()?;
            if !status.success() {
                println!("User service is not running");
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let status = std::process::Command::new("schtasks")
            .args(["/Query", "/TN", "ClipsterServer"])
            .status()
            .context("failed to run schtasks")?;
        if !status.success() {
            println!("Server is not installed");
        }
    }

    Ok(())
}
