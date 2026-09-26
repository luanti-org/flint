use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "format + lint")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Bar { name: String },
}

fn main() {
    match Cli::parse().command {
        Commands::Bar { name } => println!("yourname: {name}"),
    }
}