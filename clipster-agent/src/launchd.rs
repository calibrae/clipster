use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const PLIST_LABEL: &str = "com.clipster.agent";
const PLIST_FILENAME: &str = "com.clipster.agent.plist";

pub fn plist_path() -> PathBuf {
    directories::BaseDirs::new()
        .expect("could not determine home directory")
        .home_dir()
        .join("Library/LaunchAgents")
        .join(PLIST_FILENAME)
}

pub fn generate_plist(config_path: Option<&Path>) -> String {
    let exe = std::env::current_exe()
        .expect("could not determine current executable path")
        .display()
        .to_string();

    let home = directories::BaseDirs::new()
        .expect("could not determine home directory")
        .home_dir()
        .to_path_buf();
    let log_path = home.join("Library/Logs/clipster-agent.log");

    let mut args = String::new();
    args.push_str(&format!(
        "        <string>{exe}</string>\n        <string>run</string>\n"
    ));
    if let Some(cfg) = config_path {
        args.push_str(&format!(
            "        <string>--config</string>\n        <string>{}</string>\n",
            cfg.display()
        ));
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{PLIST_LABEL}</string>
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
    )
}

pub fn install(config_path: Option<&Path>) -> Result<()> {
    let path = plist_path();
    let content = generate_plist(config_path);

    // Ensure LaunchAgents dir exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    std::fs::write(&path, &content)
        .with_context(|| format!("failed to write plist to {}", path.display()))?;

    let output = std::process::Command::new("launchctl")
        .args(["load", &path.to_string_lossy()])
        .output()
        .context("failed to run launchctl load")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("launchctl load failed: {stderr}");
    }

    println!("Installed and loaded {}", path.display());
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let path = plist_path();

    // Unload (ignore errors — may not be loaded)
    let _ = std::process::Command::new("launchctl")
        .args(["unload", &path.to_string_lossy()])
        .output();

    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
        println!("Uninstalled {}", path.display());
    } else {
        println!("Plist not found at {}, nothing to remove", path.display());
    }

    Ok(())
}

pub fn status() -> Result<()> {
    let output = std::process::Command::new("launchctl")
        .args(["list", PLIST_LABEL])
        .output()
        .context("failed to run launchctl list")?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("Agent is loaded:\n{stdout}");
    } else {
        println!("Agent is not loaded");
    }

    Ok(())
}
