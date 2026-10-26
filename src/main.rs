use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

mod annotate;
#[cfg(feature = "compress")]
mod compress;
mod config;
mod content_type;
#[cfg(feature = "convert")]
mod convert;
mod format;
mod generate;
mod lint;
mod panic_guard;
mod parsers;

#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
    /// Lint all Lua files with selene, aware of Luanti globals, and the package's files
    Lint {
        /// Apply safe fixes, e.g. migrating legacy files to their modern equivalents
        #[arg(long)]
        write: bool,
    },
    /// Migrate legacy mod files to their modern equivalents, same as `lint --write`
    Modernize,
    #[cfg(feature = "compress")]
    /// Losslessly compress PNG textures with oxipng and Ogg Vorbis sounds with OptiVorbis
    Compress {
        /// Don't write changes, fail if any file could be compressed
        #[arg(long)]
        check: bool,
    },
    /// Create or update the package's files from its code, e.g. `min_minetest_version` and translation templates
    Generate {
        /// Files to generate, all that apply to the package and the mods and modpacks in it by default
        files: Vec<generate::Target>,
        /// Don't write changes, fail if any file is out of date
        #[arg(long)]
        check: bool,
    },
    #[cfg(feature = "convert")]
    /// Convert a file to another format: `.b3d` models to glTF (`.gltf`/`.glb`), `.obj` models to node boxes (`.lua`), and `.tr` translations to `.po`
    Convert {
        /// File to convert
        input: PathBuf,
        /// File to write, its extension picks the format
        output: PathBuf,
        /// Multiply the node box's coordinates by this, for `.obj` models
        #[arg(long)]
        scale: Option<f64>,
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
            println!(
                "{} ({})",
                env!("CARGO_PKG_VERSION"),
                env!("FLINT_GIT_HASH")
            );
            Ok(())
        }
        Commands::Format { check } => format::run(&config, check),
        Commands::Lint { write } => lint::run(&config, write),
        Commands::Modernize => lint::run(&config, true),
        #[cfg(feature = "compress")]
        Commands::Compress { check } => compress::run(&config, check),
        Commands::Generate { files, check } => generate::run(&config, &files, check),
        #[cfg(feature = "convert")]
        Commands::Convert { input, output, scale } => convert::run(&config, &input, &output, scale),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}