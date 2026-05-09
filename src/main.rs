use clap::Parser;

mod config;
mod index;
mod keychain;
mod user_mode;
mod scan_mode;

#[derive(Parser)]
#[command(name = "wip")]
#[command(about = "CLI tool for discovering and resuming active LLM sessions", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Search/filter sessions by term (user mode)
    search: Option<String>,
}

#[derive(Parser)]
enum Command {
    /// Scan filesystem for sessions and assess their state
    Scan {
        /// Force re-assessment of all sessions
        #[arg(long)]
        force: bool,

        /// Scan only specific provider (e.g., claude-code)
        #[arg(long)]
        provider: Option<String>,
    },

    /// Show current configuration
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },

    /// Manage session index
    Index {
        #[command(subcommand)]
        action: IndexAction,
    },

    /// Show token usage and scan statistics
    Stats,
}

#[derive(Parser)]
enum ConfigAction {
    /// Edit configuration file
    Edit,
}

#[derive(Parser)]
enum IndexAction {
    /// Show current session index
    Show,

    /// Clear index (forces rescan on next use)
    Clear,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Default: user mode
            if let Err(e) = user_mode::run(cli.search).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Command::Scan { force, provider }) => {
            if let Err(e) = scan_mode::run(force, provider).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Command::Config { action }) => {
            match action {
                Some(ConfigAction::Edit) => {
                    // TODO: Implement config edit
                    eprintln!("Not implemented yet");
                }
                None => {
                    // Show config
                    // TODO: Implement config show
                    eprintln!("Not implemented yet");
                }
            }
        }
        Some(Command::Index { action }) => {
            match action {
                IndexAction::Show => {
                    // TODO: Implement index show
                    eprintln!("Not implemented yet");
                }
                IndexAction::Clear => {
                    // TODO: Implement index clear
                    eprintln!("Not implemented yet");
                }
            }
        }
        Some(Command::Stats) => {
            // TODO: Implement stats
            eprintln!("Not implemented yet");
        }
    }
}
