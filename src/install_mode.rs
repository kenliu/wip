// Generates and installs a launchd plist so `wip scan` runs automatically
// in the background on macOS without requiring manual cron setup.
//
// The plist is installed to ~/Library/LaunchAgents/ and loaded immediately.
// launchd will then run `wip scan` every 10 minutes, keeping the index fresh.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{Config, SummaryBackend};

use std::path::Path;

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

fn prompt(text: &str) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::{BufRead, Write};
    print!("{}", text);
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

/// Finds the directory containing `gcloud` by running `which gcloud`.
fn detect_gcloud_dir() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("which")
        .arg("gcloud")
        .output()
        .map_err(|_| "could not run 'which gcloud'")?;
    if !output.status.success() {
        return Err("gcloud not found on PATH. Install the Google Cloud SDK first.".into());
    }
    let gcloud_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let dir = Path::new(&gcloud_path)
        .parent()
        .ok_or("could not determine gcloud directory")?
        .to_string_lossy()
        .to_string();
    Ok(dir)
}

pub(crate) fn generate_plist(wip_bin: &str, env_vars: &HashMap<String, String>, log_file: &str) -> String {
    // Build the EnvironmentVariables block only if there are vars to embed.
    // launchd agents don't inherit the shell environment, so any required env
    // vars must be explicitly set here.
    let env_block = if env_vars.is_empty() {
        String::new()
    } else {
        let entries: String = env_vars
            .iter()
            .map(|(k, v)| format!("        <key>{k}</key>\n        <string>{v}</string>\n"))
            .collect();
        format!(
            "\n    <key>EnvironmentVariables</key>\n    <dict>\n{entries}    </dict>\n"
        )
    };

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
{env_block}
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
        env_block = env_block,
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

    let input = prompt(&format!("Path to wip binary [{}]: ", suggested_bin))?;
    let wip_bin = input.trim().to_string();
    let wip_bin = if wip_bin.is_empty() { suggested_bin } else { wip_bin };

    // Load config to determine which auth backend is in use.
    // If config doesn't exist yet, fall back to Anthropic behavior.
    let backend = Config::load()
        .map(|c| c.scan.summary_backend)
        .unwrap_or_default();

    // Build the env vars to embed in the plist. launchd agents don't inherit
    // the shell environment, so required credentials must be set explicitly.
    let mut env_vars: HashMap<String, String> = HashMap::new();
    match backend {
        SummaryBackend::Anthropic => {
            // Embed the API key — will be replaced by keychain once issue #5 is done.
            let api_key = std::env::var("ANTHROPIC_API_KEY")
                .map_err(|_| "ANTHROPIC_API_KEY is not set. Set it before running wip install.")?;
            env_vars.insert("ANTHROPIC_API_KEY".to_string(), api_key);
        }
        SummaryBackend::Vertex => {
            if let Ok(creds) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
                env_vars.insert("GOOGLE_APPLICATION_CREDENTIALS".to_string(), creds);
            }
            // launchd agents get a minimal PATH that won't include gcloud.
            // Detect gcloud's location and embed just its directory so the
            // summarizer can shell out to `gcloud auth print-access-token`.
            // TODO: read ADC tokens directly instead of shelling out to gcloud.
            let gcloud_dir = detect_gcloud_dir()?;
            let confirmed_dir = prompt(&format!("Path to gcloud directory [{}]: ", gcloud_dir))?;
            let confirmed_dir = confirmed_dir.trim();
            let gcloud_dir = if confirmed_dir.is_empty() { gcloud_dir } else { confirmed_dir.to_string() };
            env_vars.insert("PATH".to_string(), format!("{gcloud_dir}:/usr/bin:/bin"));

            println!("Make sure 'gcloud auth application-default login' has been run on this machine.");
        }
    }

    // Ensure ~/.wip/ exists for the log file
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Ensure ~/Library/LaunchAgents/ exists
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let plist = generate_plist(&wip_bin, &env_vars, &log_path.to_string_lossy());
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
    if backend == SummaryBackend::Anthropic {
        println!();
        println!("Note: the API key is embedded in the plist in plaintext.");
        println!("This will be replaced by keychain storage in a future release (issue #5).");
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_no_env_vars() {
        let plist = generate_plist("/usr/local/bin/wip", &HashMap::new(), "/tmp/wip.log");
        assert!(plist.contains("<string>/usr/local/bin/wip</string>"));
        assert!(plist.contains("<string>scan</string>"));
        assert!(plist.contains("<string>/tmp/wip.log</string>"));
        assert!(plist.contains(&format!("<integer>{}</integer>", INTERVAL_SECS)));
        assert!(!plist.contains("EnvironmentVariables"));
    }

    #[test]
    fn plist_with_api_key() {
        let mut env = HashMap::new();
        env.insert("ANTHROPIC_API_KEY".to_string(), "sk-test-123".to_string());
        let plist = generate_plist("/usr/local/bin/wip", &env, "/tmp/wip.log");
        assert!(plist.contains("EnvironmentVariables"));
        assert!(plist.contains("<key>ANTHROPIC_API_KEY</key>"));
        assert!(plist.contains("<string>sk-test-123</string>"));
    }

    #[test]
    fn plist_is_valid_xml_structure() {
        let plist = generate_plist("/usr/local/bin/wip", &HashMap::new(), "/tmp/wip.log");
        assert!(plist.starts_with("<?xml"));
        assert!(plist.contains("<plist version=\"1.0\">"));
        assert!(plist.trim_end().ends_with("</plist>"));
    }

    #[test]
    fn plist_contains_label() {
        let plist = generate_plist("/usr/local/bin/wip", &HashMap::new(), "/tmp/wip.log");
        assert!(plist.contains(&format!("<string>{}</string>", LABEL)));
    }
}
