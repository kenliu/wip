use clap::Parser;

mod index;
mod user_mode;
mod scan_mode;

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
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let result = match cli.command {
        None => user_mode::run().await,
        Some(Command::Scan { force }) => scan_mode::run(force).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
