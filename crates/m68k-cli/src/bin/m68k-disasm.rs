use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;
use m68k_cli::system::System;
use m68k_core::amiga_hunk::{SectionKind, read_hunk_executable};
use m68k_core::amiga_lvo::Library;
use m68k_core::atari_prg::{looks_like_prg, read_prg};
use m68k_disasm::disassembler::{Disassembler, parse_pc_trace};

/// Layout facts recovered from a program file's own metadata (Amiga Hunk,
/// TOS program), handed to the disassembler so it need not infer them
/// from the bytes.
struct LayoutInfo {
    /// Addresses of relocated 32-bit pointers.
    relocs: Vec<u32>,
    /// `[start, end)` spans of non-code hunks.
    data_ranges: Vec<(u32, u32)>,
    /// The executable's entry point, if it has any hunks at all.
    entry: Option<u32>,
}

#[derive(Parser, Debug)]
#[command(name = "m68k-disasm", version)]
#[command(about = "Motorola 68000 disassembler", long_about = None)]
struct Args {
    /// Input binary file to disassemble (use `-` to read from stdin)
    input: PathBuf,

    /// Start address (hex, e.g. $1000 or 0x1000). For Amiga Hunk
    /// executables and TOS programs this is the load base the file is
    /// placed at, rather than a required manual guess.
    #[arg(short, long, default_value = "0")]
    address: String,

    /// CPU target
    #[arg(short, long, default_value = "68000")]
    cpu: String,

    /// Show raw bytes before each instruction
    #[arg(short = 'r', long, conflicts_with = "source")]
    raw: bool,

    /// Emit assembler source instead of a listing: an ORG line, no
    /// address column, instructions indented. Feeding the output back to
    /// m68k-asm with the same --cpu reproduces the input byte for byte.
    #[arg(long)]
    source: bool,

    /// Follow control flow from this address instead of decoding linearly
    /// (hex, e.g. $1000). Repeatable. Only code reachable from the given
    /// entry points is disassembled; everything else is emitted as data,
    /// so vectored handlers and computed-jump targets must be listed too.
    /// Hunk executables supply their own entry point automatically.
    #[arg(short = 'e', long = "entry", value_name = "ADDR")]
    entries: Vec<String>,

    /// Also treat runs of in-image longword pointers as entry points.
    /// Recovers code reached through jump tables (`jmp (a6)`), which
    /// control-flow tracing alone cannot follow. Heuristic: a run of
    /// plausible-looking longwords need not be a real table.
    #[arg(long = "scan-tables")]
    scan_tables: bool,

    /// PC trace table: a file of program counter values recorded while the
    /// image actually ran (one per line, decimal or $hex). The strongest
    /// evidence of what is code — it settles computed jumps and jump
    /// tables that no static analysis can follow. Compatible with the
    /// format used by Oxore/m68k-disasm.
    #[arg(short = 't', long = "pc-trace", value_name = "FILE")]
    pc_trace: Option<PathBuf>,

    /// Annotate `jsr -$xx(a6)` calls with AmigaOS routine names from this
    /// library (exec, dos, graphics, intuition, gadtools, utility, asl,
    /// layers, diskfont). The library must be named
    /// because the offset alone is ambiguous — `-$1e` is exec/Supervisor
    /// but dos/Input — and which base a program keeps in a6 usually cannot
    /// be decided from the instruction.
    #[arg(long = "lvo", value_name = "LIBRARY")]
    lvo: Option<String>,

    /// Name the hardware registers, system variables and OS entry points
    /// of this machine (from the equates in `include/`), and for a
    /// cartridge image at address 0 treat the vector table as pointers,
    /// its targets as entry points and the header as data. `atarist` also
    /// names GEMDOS, BIOS and XBIOS trap calls
    #[arg(long, value_name = "SYSTEM", value_enum)]
    system: Option<System>,

    /// Symbol file: one `address name` per line (`$1000 main`, `0x1000
    /// main` or `4096 main`; blank lines and `#`/`;` comments are
    /// ignored). Repeatable. The names replace generated labels and are
    /// used for any reference to those addresses
    #[arg(long = "symbols", value_name = "FILE")]
    symbols: Vec<PathBuf>,

    /// Equates file: assembler source whose `EQU` and label definitions
    /// name addresses (`custom.i`, a hardware map of your own). Every
    /// defined symbol becomes a label; unlike `--system` no address range
    /// is applied, so keep register numbers and bit masks out of it.
    /// Repeatable
    #[arg(long = "equates", value_name = "FILE")]
    equates: Vec<PathBuf>,
}

fn parse_address(s: &str) -> Result<u32, String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('$') {
        u32::from_str_radix(hex, 16).map_err(|e| format!("invalid hex address: {}", e))
    } else if let Some(hex) = s.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).map_err(|e| format!("invalid hex address: {}", e))
    } else {
        s.parse::<u32>()
            .map_err(|e| format!("invalid address: {}", e))
    }
}

/// Parses a symbol file: `address name` or `name = address` per line (the
/// latter is what `m68k-asm --sym` writes), blank lines and `#`/`;`
/// comments ignored. On failure returns the 1-based line and its text.
fn parse_symbol_file(text: &str) -> Result<Vec<(u32, String)>, (usize, String)> {
    let mut labels = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let parsed = match line.split_once('=') {
            Some((name, addr)) => Some((addr.trim(), name.trim())),
            None => line.split_once(char::is_whitespace),
        }
        .and_then(|(addr, name)| {
            let name = name.trim();
            (!name.is_empty() && !name.contains(char::is_whitespace))
                .then(|| parse_address(addr).ok().map(|a| (a, name.to_string())))
                .flatten()
        });
        match parsed {
            Some(label) => labels.push(label),
            None => return Err((index + 1, line.to_string())),
        }
    }
    Ok(labels)
}

/// `-` means stdin, as documented in the README. Everything else is a
/// filesystem path — including `./-`, which is the escape hatch for a file
/// literally named `-`.
fn is_stdin_path(p: &Path) -> bool {
    p.as_os_str() == "-"
}

/// Reads the input image, either from stdin (`-`) or from a file. stdin is
/// read as raw bytes, not text: the input is a binary image, and Hunk
/// executables in particular are not valid UTF-8.
fn read_input(path: &Path) -> Result<Vec<u8>, String> {
    if is_stdin_path(path) {
        let mut data = Vec::new();
        std::io::stdin()
            .read_to_end(&mut data)
            .map_err(|e| format!("cannot read stdin: {}", e))?;
        Ok(data)
    } else {
        fs::read(path).map_err(|e| format!("cannot read '{}': {}", path.display(), e))
    }
}

fn input_display(path: &Path) -> String {
    if is_stdin_path(path) {
        "<stdin>".to_string()
    } else {
        path.display().to_string()
    }
}

fn run(args: Args) -> Result<(), String> {
    m68k_core::cpu_gate::validate_cpu_name(&args.cpu)?;
    let start_addr = parse_address(&args.address)?;

    let data = read_input(&args.input)?;

    if data.is_empty() {
        return Err(format!("empty input: {}", input_display(&args.input)));
    }

    // Amiga Hunk executables (HUNK_HEADER magic 0x000003F3) get their
    // hunks loaded, relocated, and flattened into one image; everything
    // else is treated as a raw binary at the given start address.
    let mut layout: Option<LayoutInfo> = None;
    let (image, base_addr, known_labels) = if data.len() >= 4 && data[0..4] == [0, 0, 0x03, 0xf3] {
        let exe = read_hunk_executable(&data, start_addr)
            .map_err(|e| format!("cannot read '{}': {}", input_display(&args.input), e))?;
        let labels: Vec<(u32, String)> = exe
            .all_symbols()
            .into_iter()
            .map(|(name, addr)| (addr, name))
            .collect();
        // What the linker recorded about this executable: every relocated
        // longword is a pointer, and every non-Code hunk is data outright.
        // Both beat guessing from the bytes.
        layout = Some(LayoutInfo {
            relocs: exe.all_relocs(),
            data_ranges: exe
                .sections
                .iter()
                .filter(|s| s.kind != SectionKind::Code)
                .filter_map(|s| {
                    s.address
                        .checked_add(s.data.len() as u32)
                        .map(|end| (s.address, end))
                })
                .collect(),
            // Execution begins at the start of the first hunk.
            entry: exe.sections.first().map(|s| s.address),
        });
        (exe.image, exe.load_base, labels)
    } else if looks_like_prg(&data) {
        // A TOS program: text and data loaded back to back at the given
        // address, every relocated longword a pointer, the data segment
        // data by the file's own account, and its symbol table's names as
        // labels. Execution begins at the start of the text segment.
        let prg = read_prg(&data, start_addr)
            .map_err(|e| format!("cannot read '{}': {}", input_display(&args.input), e))?;
        let data_start = prg.load_base.wrapping_add(prg.text_len);
        layout = Some(LayoutInfo {
            relocs: prg.relocs,
            data_ranges: vec![(data_start, data_start.wrapping_add(prg.data_len))],
            entry: Some(prg.load_base),
        });
        (prg.image, prg.load_base, prg.symbols)
    } else {
        (data, start_addr, Vec::new())
    };

    let image_len = image.len();
    let system = args
        .system
        .map(|system| system.facts(base_addr, &image))
        .transpose()?;
    let mut disasm = Disassembler::new(image, base_addr);
    disasm.add_known_labels(known_labels);
    if let Some(facts) = system {
        disasm.add_known_labels(facts.labels);
        disasm.add_pointer_sites(facts.pointer_sites);
        disasm.add_data_ranges(facts.data_ranges);
        disasm.add_entry_points(facts.entry_points);
        disasm.add_trap_names(facts.trap_names);
        disasm.add_write_notes(facts.write_notes);
    }
    if let Some(info) = layout {
        disasm.add_pointer_sites(info.relocs);
        disasm.add_data_ranges(info.data_ranges);
        disasm.add_entry_points(info.entry);
    }
    for path in &args.symbols {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;
        let labels = parse_symbol_file(&text).map_err(|(line, text)| {
            format!(
                "{}:{}: expected 'address name', got: {}",
                path.display(),
                line,
                text
            )
        })?;
        disasm.add_known_labels(labels);
    }
    for path in &args.equates {
        let source = fs::read_to_string(path)
            .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;
        let labels = m68k_cli::system::equates(&source)
            .map_err(|e| format!("{}: {}", path.display(), e))?
            .into_iter()
            // Register numbers, bit masks and sizes are small values that
            // would otherwise name the first bytes of an image at 0; a
            // place inside the image is named through `--symbols`.
            .filter(|(_, value, _)| {
                !(base_addr..base_addr.wrapping_add(image_len as u32)).contains(value)
            })
            .map(|(name, value, _)| (value, name));
        disasm.add_known_labels(labels);
    }
    let extra_entries = args
        .entries
        .iter()
        .map(|e| parse_address(e))
        .collect::<Result<Vec<_>, _>>()?;
    disasm.add_entry_points(extra_entries);
    if args.scan_tables {
        disasm.seed_entry_points_from_pointer_tables();
    }
    if let Some(path) = &args.pc_trace {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;
        let pcs = parse_pc_trace(&text).map_err(|(line, text)| {
            format!("{}:{}: not a PC value: {}", path.display(), line, text)
        })?;
        let before = pcs.len();
        disasm.add_pc_trace(pcs);
        // A trace recorded against a different image (or a different load
        // address) silently contributes nothing, which would look like the
        // option being ignored; say so rather than leaving the user to
        // wonder why the output did not change.
        let kept = disasm.pc_trace_len();
        if kept == 0 && before > 0 {
            return Err(format!(
                "none of the {} PC values in '{}' fall inside the image \
                 (${:08x}..${:08x}) — wrong trace, or wrong --address?",
                before,
                path.display(),
                base_addr,
                base_addr.saturating_add(image_len as u32)
            ));
        }
    }
    if let Some(name) = &args.lvo {
        let library = Library::parse(name).ok_or_else(|| {
            format!(
                "unknown library '{}' (known: {})",
                name,
                Library::names().join(", ")
            )
        })?;
        disasm.set_lvo_library(library);
    }
    disasm.set_cpu(&args.cpu);

    let mut out = std::io::stdout().lock();
    let written = (|| -> std::io::Result<()> {
        if args.source {
            writeln!(out, "        org     ${:x}", base_addr)?;
        }
        for line in disasm.disassemble() {
            if line.is_error {
                eprintln!("{}", line.text);
                continue;
            }
            // Emit the label definition on its own line, ahead of the
            // instruction it marks. Without it the output referenced labels
            // that were never defined anywhere, so it could not be fed back
            // into the assembler.
            if let Some(label) = &line.label {
                writeln!(out, "{}:", label)?;
            }
            if args.source {
                // Column one is where labels live; an instruction without
                // operands there (`rts`) would be read as one.
                write!(out, "        ")?;
            } else if args.raw {
                let hex: String = line
                    .raw_bytes
                    .iter()
                    .map(|b| format!(" {:02x}", b))
                    .collect();
                write!(out, "{:08x}:{}  ", line.address, hex)?;
            } else {
                write!(out, "{:08x}:  ", line.address)?;
            }
            match &line.comment {
                // Rendered as a trailing comment so the instruction text stays
                // exactly what the assembler accepts.
                Some(note) => writeln!(out, "{:<40} ; {}", line.text, note)?,
                None => writeln!(out, "{}", line.text)?,
            }
        }
        out.flush()
    })();
    match written {
        // The reader closed the pipe (`| head`, `| grep -q`): the listing
        // was cut short on purpose, so it is not an error.
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other.map_err(|e| format!("cannot write to stdout: {}", e)),
    }
}

fn main() {
    let args = Args::parse();

    if let Err(e) = run(args) {
        eprintln!("error: {}", e);
        process::exit(1);
    }
}
