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

    /// Create a new empty, mountable AmigaDOS disk image at IMAGE with the
    /// given volume name, then apply any write options below to it
    #[arg(long, value_name = "VOLUME_NAME")]
    format: Option<String>,

    /// Use FFS instead of OFS when creating a disk with --format
    #[arg(long)]
    ffs: bool,

    /// Write a local file into the image: --add-file LOCAL AMIGA_PATH
    #[arg(long, num_args = 2, value_names = ["LOCAL", "AMIGA_PATH"])]
    add_file: Option<Vec<String>>,

    /// Delete a file from the image
    #[arg(long, value_name = "PATH")]
    delete_file: Option<String>,

    /// Create a directory in the image
    #[arg(long, value_name = "PATH")]
    make_dir: Option<String>,

    /// Delete an empty directory from the image
    #[arg(long, value_name = "PATH")]
    delete_dir: Option<String>,

    /// Rename or move an entry: --rename FROM TO
    #[arg(long, num_args = 2, value_names = ["FROM", "TO"])]
    rename: Option<Vec<String>>,

    /// Set an entry's comment: --set-comment PATH TEXT
    #[arg(long, num_args = 2, value_names = ["PATH", "TEXT"])]
    set_comment: Option<Vec<String>>,

    /// Set an entry's protection bits: --set-protection PATH BITS
    /// (decimal, or hex with a 0x prefix)
    #[arg(long, num_args = 2, value_names = ["PATH", "BITS"])]
    set_protection: Option<Vec<String>>,
}

/// Whether any option that modifies the filesystem was given.
fn has_write_ops(args: &Args) -> bool {
    args.format.is_some()
        || args.add_file.is_some()
        || args.delete_file.is_some()
        || args.make_dir.is_some()
        || args.delete_dir.is_some()
        || args.rename.is_some()
        || args.set_comment.is_some()
        || args.set_protection.is_some()
}

/// Apply every requested filesystem modification, in a fixed order, to a
/// raw image buffer and write it back.
///
/// These operate on the raw ADF bytes rather than through the backend
/// abstraction: IPF is a flux-level format that cannot be written back,
/// and UAE-extended images have variable-length tracks. A plain
/// sector-addressable image is the only kind a filesystem writer applies
/// to, so that is what this requires.
fn run_write_ops(args: &Args) -> Result<(), String> {
    use m68k_floppy::adf_writer;
    use m68k_floppy::amigados_write::AmigaFsWriter;

    let mut image = match &args.format {
        Some(volume_name) => {
            let created = if args.ffs {
                adf_writer::format_empty_ffs_disk(volume_name)
            } else {
                adf_writer::format_empty_ofs_disk(volume_name)
            }
            .map_err(|e| format!("cannot format image: {}", e))?;
            eprintln!(
                "Formatted {} volume {:?} ({} KiB)",
                if args.ffs { "FFS" } else { "OFS" },
                volume_name,
                created.len() / 1024
            );
            created
        }
        None => std::fs::read(&args.image).map_err(|e| format!("cannot read image: {}", e))?,
    };

    {
        let mut fs = AmigaFsWriter::mount(&mut image)
            .map_err(|e| format!("cannot mount filesystem for writing: {}", e))?;

        if let Some(path) = &args.make_dir {
            fs.create_dir(path)
                .map_err(|e| format!("cannot create directory {:?}: {}", path, e))?;
            eprintln!("Created directory {}", path);
        }

        if let Some(pair) = &args.add_file {
            let (local, amiga) = (&pair[0], &pair[1]);
            let data =
                std::fs::read(local).map_err(|e| format!("cannot read {:?}: {}", local, e))?;
            fs.write_file(amiga, &data)
                .map_err(|e| format!("cannot write {:?}: {}", amiga, e))?;
            eprintln!("Wrote {} ({} bytes) to {}", local, data.len(), amiga);
        }

        if let Some(pair) = &args.rename {
            fs.rename(&pair[0], &pair[1])
                .map_err(|e| format!("cannot rename {:?}: {}", pair[0], e))?;
            eprintln!("Renamed {} to {}", pair[0], pair[1]);
        }

        if let Some(pair) = &args.set_comment {
            fs.set_comment(&pair[0], &pair[1])
                .map_err(|e| format!("cannot set comment on {:?}: {}", pair[0], e))?;
        }

        if let Some(pair) = &args.set_protection {
            let raw = &pair[1];
            let bits = match raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
                Some(hex) => u32::from_str_radix(hex, 16),
                None => raw.parse::<u32>(),
            }
            .map_err(|e| format!("invalid protection bits {:?}: {}", raw, e))?;
            fs.set_protection(&pair[0], bits)
                .map_err(|e| format!("cannot set protection on {:?}: {}", pair[0], e))?;
        }

        if let Some(path) = &args.delete_file {
            fs.delete_file(path)
                .map_err(|e| format!("cannot delete {:?}: {}", path, e))?;
            eprintln!("Deleted {}", path);
        }

        if let Some(path) = &args.delete_dir {
            fs.delete_dir(path)
                .map_err(|e| format!("cannot delete directory {:?}: {}", path, e))?;
            eprintln!("Deleted directory {}", path);
        }

        eprintln!("{} blocks free", fs.free_blocks());
    }

    adf_writer::write_adf_file(&args.image, &image)
        .map_err(|e| format!("cannot write image: {}", e))?;
    eprintln!("Image written to {}", args.image.display());
    Ok(())
}

fn run(args: Args) -> Result<(), String> {
    if has_write_ops(&args) {
        return run_write_ops(&args);
    }

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
