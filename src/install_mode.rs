// Generates and installs a launchd plist so `wip scan` runs automatically
// in the background on macOS without requiring manual cron setup.
//
// The plist is installed to ~/Library/LaunchAgents/ and loaded immediately.
// launchd will then run `wip scan` every 10 minutes, keeping the index fresh.

use std::path::PathBuf;
use std::process::Command;

const LABEL: &str = "com.kenliu.wip";
const INTERVAL_SECS: u32 = 600; // 10 minutes

fn plist_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", LABEL))
}

fn log_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".wip")
        .join("launchd.log")
}

fn generate_plist(wip_bin: &str, api_key: &str, log_file: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>

    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>scan</string>
    </array>

    <!-- Run every {interval} seconds -->
    <key>StartInterval</key>
    <integer>{interval}</integer>

    <!-- Embed API key since launchd agents don't inherit shell environment.
         Replace with keychain integration once issue #5 is implemented. -->
    <key>EnvironmentVariables</key>
    <dict>
        <key>ANTHROPIC_API_KEY</key>
        <string>{api_key}</string>
    </dict>

    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>

    <!-- Only run when the user is logged in -->
    <key>RunAtLoad</key>
    <false/>
</dict>
</plist>
"#,
        label = LABEL,
        bin = wip_bin,
        interval = INTERVAL_SECS,
        api_key = api_key,
        log = log_file,
    )
}

pub fn install() -> Result<(), Box<dyn std::error::Error>> {
    let plist_path = plist_path();
    let log_path = log_path();

    // Warn if already installed
    if plist_path.exists() {
        eprintln!("Already installed at {}", plist_path.display());
        eprintln!("Run 'wip uninstall' first to reinstall.");
        return Ok(());
    }

    // Prompt for the binary path, suggesting the current executable as a default.
    // The user may be running from a debug build or a non-standard location, so
    // we don't assume current_exe() is the right path for the plist.
    let suggested_bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/usr/local/bin/wip".to_string());

    print!("Path to wip binary [{}]: ", suggested_bin);
    std::io::Write::flush(&mut std::io::stdout())?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let wip_bin = input.trim().to_string();
    let wip_bin = if wip_bin.is_empty() { suggested_bin } else { wip_bin };

    // Embed the current API key — launchd agents don't inherit shell environment.
    // This will be replaced by keychain lookup once issue #5 is implemented.
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY is not set. Set it before running wip install.")?;

    // Ensure ~/.wip/ exists for the log file
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Ensure ~/Library/LaunchAgents/ exists
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let plist = generate_plist(&wip_bin, &api_key, &log_path.to_string_lossy());
    std::fs::write(&plist_path, &plist)?;
    println!("Wrote plist to {}", plist_path.display());

    // Load the plist into launchd
    let status = Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .status()?;

    if !status.success() {
        return Err("launchctl load failed — check the plist manually".into());
    }

    println!("Installed and loaded. wip scan will run every {} minutes.", INTERVAL_SECS / 60);
    println!("Logs: {}", log_path.display());
    println!();
    println!("Note: the API key is embedded in the plist in plaintext.");
    println!("This will be replaced by keychain storage in a future release (issue #5).");

    Ok(())
}

pub fn uninstall() -> Result<(), Box<dyn std::error::Error>> {
    let plist_path = plist_path();

    if !plist_path.exists() {
        eprintln!("Not installed (no plist found at {}).", plist_path.display());
        return Ok(());
    }

    // Unload from launchd before deleting the file
    let status = Command::new("launchctl")
        .args(["unload", &plist_path.to_string_lossy()])
        .status()?;

    if !status.success() {
        eprintln!("Warning: launchctl unload returned an error. Removing plist anyway.");
    }

    std::fs::remove_file(&plist_path)?;
    println!("Uninstalled. Removed {}", plist_path.display());

    Ok(())
}
