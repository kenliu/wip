use clap::{Parser, Subcommand};

mod config;
mod fast_mode;
mod index;
mod install_mode;
mod scan_mode;
mod stats_mode;
mod user_mode;

#[derive(Parser)]
#[command(name = "wip")]
#[command(about = "Find and resume your in-progress LLM sessions")]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_SHA"), ")"))]
#[command(after_help = concat!("Build: ", env!("CARGO_PKG_VERSION"), " (", env!("GIT_SHA"), ")"))]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Run a scan in the background while the TUI is open
    #[arg(long)]
    background_scan: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Scan filesystem for sessions and summarize their state
    Scan {
        #[arg(long)]
        force: bool,
    },
    /// fzf-powered session picker (requires fzf)
    Fast,
    /// Install launchd agent for automatic background scanning (macOS)
    Install,
    /// Uninstall the launchd agent
    Uninstall,
    /// Manage the session index
    Index {
        #[command(subcommand)]
        action: IndexAction,
    },
    /// Show token usage and scan statistics
    Stats,
}

#[derive(Subcommand)]
enum IndexAction {
    /// Delete the index file, forcing a full rescan on next `wip scan`
    Clear,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let result = match cli.command {
        None => user_mode::run(cli.background_scan).await,
        Some(Command::Fast) => fast_mode::run().await,
        Some(Command::Scan { force }) => scan_mode::run(force, false).await,
        Some(Command::Install) => install_mode::install(),
        Some(Command::Uninstall) => install_mode::uninstall(),
        Some(Command::Index {
            action: IndexAction::Clear,
        }) => index_clear(),
        Some(Command::Stats) => stats_mode::run(),
    };

    if let Err(e) = result {
        let msg = e.to_string();
        // Setup guides and user-facing messages are self-describing; other errors get a prefix.
        if msg.starts_with("wip ") {
            eprintln!("{}", msg);
        } else {
            eprintln!("Error: {}", msg);
        }
        std::process::exit(1);
    }
}

fn index_clear() -> Result<(), Box<dyn std::error::Error>> {
    let path = index::index_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
        println!("Deleted {}", path.display());
    } else {
        println!(
            "Index does not exist ({}), nothing to clear.",
            path.display()
        );
    }
    Ok(())
}
