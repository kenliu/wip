use clap::{Parser, Subcommand};

mod index;
mod install_mode;
mod scan_mode;
mod user_mode;

#[derive(Parser)]
#[command(name = "wip")]
#[command(about = "Find and resume your in-progress LLM sessions")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Parser)]
enum Command {
    /// Scan filesystem for sessions and assess their state
    Scan {
        #[arg(long)]
        force: bool,
    },
    /// Install launchd agent for automatic background scanning (macOS)
    Install,
    /// Uninstall the launchd agent
    Uninstall,
    /// Manage the session index
    Index {
        #[command(subcommand)]
        action: IndexAction,
    },
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
        None => user_mode::run().await,
        Some(Command::Scan { force }) => scan_mode::run(force).await,
        Some(Command::Install) => install_mode::install(),
        Some(Command::Uninstall) => install_mode::uninstall(),
        Some(Command::Index { action: IndexAction::Clear }) => index_clear(),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn index_clear() -> Result<(), Box<dyn std::error::Error>> {
    let path = index::index_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
        println!("Deleted {}", path.display());
    } else {
        println!("Index does not exist ({}), nothing to clear.", path.display());
    }
    Ok(())
}
