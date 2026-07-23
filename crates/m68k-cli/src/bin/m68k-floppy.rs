//! CLI entry point for floppy image inspection.

use std::io::Write;
use std::path::PathBuf;
use std::process;

use clap::{Parser, ValueEnum};
use m68k_floppy::factory::{Backend, open_floppy_image};
use m68k_floppy::floppy_base::FloppyImageReader;

#[derive(Copy, Clone, Debug, ValueEnum)]
enum BackendArg {
    Auto,
    Adf,
    Native,
    Uae,
}

impl From<BackendArg> for Backend {
    fn from(b: BackendArg) -> Self {
        match b {
            BackendArg::Auto => Backend::Auto,
            BackendArg::Adf => Backend::Adf,
            BackendArg::Native => Backend::Native,
            BackendArg::Uae => Backend::Uae,
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "m68k-floppy")]
#[command(about = "Inspect Amiga floppy disk images and extract data", long_about = None)]
struct Args {
    /// Path to the disk image (.adf or .ipf)
    image: PathBuf,

    /// Backend to use (default: auto-detect from extension)
    #[arg(long, value_enum, default_value = "auto")]
    backend: BackendArg,

    /// Dump the bootblock (1024 bytes) to stdout
    #[arg(long)]
    bootblock: bool,

    /// Dump a specific sector to stdout: TRACK SIDE SECTOR
    #[arg(long, num_args = 3, value_names = ["TRACK", "SIDE", "SECTOR"])]
    sector: Option<Vec<u32>>,
}

fn run(args: Args) -> Result<(), String> {
    let mut reader =
        open_floppy_image(&args.image, args.backend.into()).map_err(|e| e.to_string())?;

    if args.bootblock {
        let data = reader
            .get_bootblock()
            .map_err(|e| format!("Error reading bootblock: {}", e))?;
        std::io::stdout()
            .write_all(&data)
            .map_err(|e| e.to_string())?;
    } else if let Some(sector) = args.sector {
        let (track, side, sec) = (sector[0], sector[1], sector[2]);
        let data = reader
            .read_sector(track, side, sec)
            .map_err(|e| format!("Error reading sector: {}", e))?;
        std::io::stdout()
            .write_all(&data)
            .map_err(|e| e.to_string())?;
    } else {
        eprintln!("No action specified; pass --bootblock or --sector TRACK SIDE SECTOR");
    }

    Ok(())
}

fn main() {
    let args = Args::parse();
    if let Err(e) = run(args) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
