use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "format + lint")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Version,
}

fn main() {
    match Cli::parse().command {
        Commands::Version => println!("{}", env!("CARGO_PKG_VERSION")),
    }
}