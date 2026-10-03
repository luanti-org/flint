use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod content_type;
mod format;
mod lint;
mod modernize;
mod panic_guard;
mod parsers;

#[derive(Parser)]
#[command(about = "format + lint")]
struct Cli {
    /// Mod directory to operate on (defaults to current dir)
    #[arg(long, global = true, default_value = ".")]
    path: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Version,
    /// Format all Lua files with StyLua (Lua 5.1 + LuaJIT syntax)
    Format,
    /// Lint all Lua files with selene, aware of Luanti globals
    Lint,
    /// Migrate legacy mod files to their modern equivalents
    Modernize,
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = std::env::set_current_dir(&cli.path) {
        eprintln!("error: cannot enter {}: {e}", cli.path.display());
        std::process::exit(1);
    }

    let result = match cli.command {
        Commands::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Commands::Format => format::run(),
        Commands::Lint => lint::run(),
        Commands::Modernize => modernize::run(),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}