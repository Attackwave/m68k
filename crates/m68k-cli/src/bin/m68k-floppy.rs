//! CLI entry point for floppy image inspection.

use std::io::Write;
use std::path::PathBuf;
use std::process;

use clap::{Parser, ValueEnum};
use m68k_floppy::amigados::AmigaFs;
use m68k_floppy::factory::{Backend, open_floppy_image};
use m68k_floppy::floppy_base::FloppyImageReader;

/// Directory nesting depth for `--list-all`/`--extract-all`. Deep enough
/// for any real Workbench layout, bounded so a corrupt image with a cyclic
/// directory chain can't loop forever.
const MAX_WALK_DEPTH: usize = 64;

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

    /// List an AmigaDOS (OFS/FFS) directory. Takes an optional path, e.g.
    /// `--list` for the root or `--list C` for a subdirectory
    #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
    list: Option<String>,

    /// Recursively list the whole filesystem (optionally from a
    /// subdirectory), showing each entry's full path
    #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
    list_all: Option<String>,

    /// Extract a file from the AmigaDOS filesystem and write it to stdout,
    /// or to --output if given. Accepts a path, e.g. `Libs/diskfont.library`
    #[arg(long, value_name = "PATH")]
    extract: Option<String>,

    /// Extract every file into the given directory, recreating the disk's
    /// directory structure
    #[arg(long, value_name = "DIR")]
    extract_all: Option<PathBuf>,

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
    } else if let Some(path) = &args.list {
        let mut fs = AmigaFs::mount(&mut reader, args.tracks)
            .map_err(|e| format!("cannot mount filesystem: {}", e))?;
        let entries = fs
            .list_dir_at_path(path)
            .map_err(|e| format!("cannot list directory: {}", e))?;
        for entry in entries {
            if entry.is_dir {
                println!("{:>10}  {}/", "<DIR>", entry.name);
            } else {
                println!("{:>10}  {}", entry.size, entry.name);
            }
        }
    } else if let Some(path) = &args.list_all {
        let mut fs = AmigaFs::mount(&mut reader, args.tracks)
            .map_err(|e| format!("cannot mount filesystem: {}", e))?;
        let entries = fs
            .walk(path, MAX_WALK_DEPTH)
            .map_err(|e| format!("cannot walk filesystem: {}", e))?;
        for (full_path, entry) in entries {
            if entry.is_dir {
                println!("{:>10}  {}/", "<DIR>", full_path);
            } else {
                println!("{:>10}  {}", entry.size, full_path);
            }
        }
    } else if let Some(name) = &args.extract {
        let mut fs = AmigaFs::mount(&mut reader, args.tracks)
            .map_err(|e| format!("cannot mount filesystem: {}", e))?;
        let data = fs
            .read_file_at_path(name)
            .map_err(|e| format!("cannot extract '{}': {}", name, e))?;
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
    } else if let Some(dest) = &args.extract_all {
        let mut fs = AmigaFs::mount(&mut reader, args.tracks)
            .map_err(|e| format!("cannot mount filesystem: {}", e))?;
        let entries = fs
            .walk("", MAX_WALK_DEPTH)
            .map_err(|e| format!("cannot walk filesystem: {}", e))?;
        let mut files = 0usize;
        let mut dirs = 0usize;
        for (full_path, entry) in entries {
            // Reject path components that would escape the destination
            // directory. AmigaDOS names can legally contain characters
            // that mean something else to the host filesystem.
            if full_path
                .split('/')
                .any(|c| c == "." || c == ".." || c.contains('\\'))
            {
                eprintln!("skipping unsafe path: {}", full_path);
                continue;
            }
            let target = dest.join(&full_path);
            if entry.is_dir {
                std::fs::create_dir_all(&target)
                    .map_err(|e| format!("cannot create '{}': {}", target.display(), e))?;
                dirs += 1;
            } else {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("cannot create '{}': {}", parent.display(), e))?;
                }
                let data = fs
                    .read_file(entry.block)
                    .map_err(|e| format!("cannot read '{}': {}", full_path, e))?;
                std::fs::write(&target, &data)
                    .map_err(|e| format!("cannot write '{}': {}", target.display(), e))?;
                files += 1;
            }
        }
        eprintln!("extracted {} files and {} directories", files, dirs);
    } else {
        eprintln!(
            "No action specified; pass --bootblock, --sector TRACK SIDE SECTOR, --list [PATH], --list-all [PATH], --extract PATH, --extract-all DIR, or --fix-bootblock"
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
