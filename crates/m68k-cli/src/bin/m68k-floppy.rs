//! CLI entry point for floppy image inspection.

use std::io::Write;
use std::path::PathBuf;
use std::process;

use clap::{Parser, ValueEnum};
use m68k_floppy::amigados::AmigaFs;
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

    /// List the AmigaDOS (OFS/FFS) root directory contents
    #[arg(long)]
    list: bool,

    /// Extract a file from the AmigaDOS filesystem by name (root
    /// directory only) and write it to stdout, or to --output if given
    #[arg(long, value_name = "NAME")]
    extract: Option<String>,

    /// Output file for --extract (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Total disk tracks, for AmigaDOS filesystem operations
    /// (--list/--extract/--fix-bootblock); 80 is standard for DD/HD ADFs
    #[arg(long, default_value = "80")]
    tracks: u32,

    /// Recompute and repair the bootblock checksum in place
    #[arg(long)]
    fix_bootblock: bool,
}

fn run(args: Args) -> Result<(), String> {
    if args.fix_bootblock {
        let mut image =
            std::fs::read(&args.image).map_err(|e| format!("cannot read image: {}", e))?;
        m68k_floppy::adf_writer::fix_bootblock_checksum(&mut image)
            .map_err(|e| format!("cannot fix bootblock: {}", e))?;
        std::fs::write(&args.image, &image)
            .map_err(|e| format!("cannot write repaired image: {}", e))?;
        println!("Bootblock checksum repaired.");
        return Ok(());
    }

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
    } else if args.list {
        let mut fs = AmigaFs::mount(&mut reader, args.tracks)
            .map_err(|e| format!("cannot mount filesystem: {}", e))?;
        let root = fs.root_block_num();
        let entries = fs
            .list_dir(root)
            .map_err(|e| format!("cannot list directory: {}", e))?;
        for entry in entries {
            if entry.is_dir {
                println!("{:>10}  {}/", "<DIR>", entry.name);
            } else {
                println!("{:>10}  {}", entry.size, entry.name);
            }
        }
    } else if let Some(name) = &args.extract {
        let mut fs = AmigaFs::mount(&mut reader, args.tracks)
            .map_err(|e| format!("cannot mount filesystem: {}", e))?;
        let root = fs.root_block_num();
        let entry = fs
            .find_entry(root, name)
            .map_err(|e| format!("cannot look up '{}': {}", name, e))?
            .ok_or_else(|| format!("'{}' not found in root directory", name))?;
        if entry.is_dir {
            return Err(format!("'{}' is a directory, not a file", name));
        }
        let data = fs
            .read_file(entry.block)
            .map_err(|e| format!("cannot read '{}': {}", name, e))?;
        match &args.output {
            Some(path) => {
                std::fs::write(path, &data).map_err(|e| format!("cannot write output: {}", e))?;
            }
            None => {
                std::io::stdout()
                    .write_all(&data)
                    .map_err(|e| e.to_string())?;
            }
        }
    } else {
        eprintln!(
            "No action specified; pass --bootblock, --sector TRACK SIDE SECTOR, --list, --extract NAME, or --fix-bootblock"
        );
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
