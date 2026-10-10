use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

mod annotate;
mod compress;
mod config;
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
    Format {
        /// Don't write changes, fail if any file isn't formatted
        #[arg(long)]
        check: bool,
    },
    /// Lint all Lua files with selene, aware of Luanti globals
    Lint,
    /// Migrate legacy mod files to their modern equivalents
    Modernize,
    /// Losslessly compress PNG textures with oxipng and Ogg Vorbis sounds with OptiVorbis
    Compress {
        /// Don't write changes, fail if any file could be compressed
        #[arg(long)]
        check: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = std::env::set_current_dir(&cli.path) {
        eprintln!("error: cannot enter {}: {e}", cli.path.display());
        std::process::exit(1);
    }

    let config = match config::load(Path::new(".")) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let result = match cli.command {
        Commands::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Commands::Format { check } => format::run(&config, check),
        Commands::Lint => lint::run(&config),
        Commands::Modernize => modernize::run(),
        Commands::Compress { check } => compress::run(&config, check),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}