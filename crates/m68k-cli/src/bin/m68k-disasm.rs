use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;
use m68k_core::amiga_hunk::{SectionKind, read_hunk_executable};
use m68k_disasm::disassembler::{Disassembler, parse_pc_trace};

/// Layout facts recovered from an Amiga Hunk executable's own metadata,
/// handed to the disassembler so it need not infer them from the bytes.
struct HunkInfo {
    /// Addresses of relocated 32-bit pointers.
    relocs: Vec<u32>,
    /// `[start, end)` spans of non-code hunks.
    data_ranges: Vec<(u32, u32)>,
    /// The executable's entry point, if it has any hunks at all.
    entry: Option<u32>,
}

#[derive(Parser, Debug)]
#[command(name = "m68k-disasm")]
#[command(about = "Motorola 68000 disassembler", long_about = None)]
struct Args {
    /// Input binary file to disassemble (use `-` to read from stdin)
    input: PathBuf,

    /// Start address (hex, e.g. $1000 or 0x1000). For Amiga Hunk
    /// executables this is the load base the first hunk is placed at,
    /// rather than a required manual guess.
    #[arg(short, long, default_value = "0")]
    address: String,

    /// CPU target
    #[arg(short, long, default_value = "68000")]
    cpu: String,

    /// Show raw bytes before each instruction
    #[arg(short = 'r', long)]
    raw: bool,

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
    let mut hunk_info: Option<HunkInfo> = None;
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
        hunk_info = Some(HunkInfo {
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
    } else {
        (data, start_addr, Vec::new())
    };

    let image_len = image.len();
    let mut disasm = Disassembler::new(image, base_addr);
    disasm.add_known_labels(known_labels);
    if let Some(info) = hunk_info {
        disasm.add_pointer_sites(info.relocs);
        disasm.add_data_ranges(info.data_ranges);
        disasm.add_entry_points(info.entry);
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
    disasm.set_cpu(&args.cpu);

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
            println!("{}:", label);
        }
        if args.raw {
            let hex: String = line
                .raw_bytes
                .iter()
                .map(|b| format!(" {:02x}", b))
                .collect();
            print!("{:08x}:{}  ", line.address, hex);
        } else {
            print!("{:08x}:  ", line.address);
        }
        println!("{}", line.text);
    }

    Ok(())
}

fn main() {
    let args = Args::parse();

    if let Err(e) = run(args) {
        eprintln!("error: {}", e);
        process::exit(1);
    }
}
