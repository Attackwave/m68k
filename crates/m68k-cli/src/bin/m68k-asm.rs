use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

use clap::{Parser, ValueEnum};
use m68k_asm::amiga_hunk_writer::generate_hunk_exe;
use m68k_asm::assembler::Assembler;
use m68k_asm::ieee695::generate_ieee695_sections;
use m68k_asm::output::{
    OutputFormat, generate_binary_to, generate_elf_sections, generate_intel_hex, generate_srecord,
};

#[derive(Parser, Debug)]
#[command(name = "m68k-asm")]
#[command(about = "Motorola 68000 assembler", long_about = None)]
struct Args {
    /// Input assembly source file (use `-` to read from stdin)
    input: PathBuf,

    /// Output file (use `-` for stdout; default: input with
    /// .bin/.srec/.hex extension)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, value_enum, default_value = "binary")]
    format: OutputFormatArg,

    /// Origin address (hex, e.g. $1000 or 0x1000)
    #[arg(long, default_value = "0")]
    origin: String,

    /// CPU target
    #[arg(short, long, default_value = "68000")]
    cpu: String,

    /// Add a directory to the INCLUDE search path (repeatable), e.g.
    /// `-I /path/to/Include` for the Amiga system headers
    #[arg(short = 'I', long = "include", value_name = "DIR")]
    include_paths: Vec<PathBuf>,

    /// Enable size-changing optimizations: encode an absolute address
    /// that fits in 16 bits as absolute-short. Off by default, so output
    /// matches assemblers running without optimization; an explicit `.W`
    /// suffix always yields the short form regardless.
    #[arg(long)]
    optimize: bool,

    /// S-Record header name (used with -f srecord)
    #[arg(long, default_value = "m68k-asm")]
    srec_name: String,

    /// Generate listing file
    #[arg(short, long)]
    listing: Option<PathBuf>,

    /// Export symbol table to file
    #[arg(long)]
    sym: Option<PathBuf>,

    /// Export memory map to file
    #[arg(long)]
    map: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormatArg {
    Binary,
    Srecord,
    IntelHex,
    Elf,
    Ieee695,
    HunkExe,
}

impl From<OutputFormatArg> for OutputFormat {
    fn from(val: OutputFormatArg) -> Self {
        match val {
            OutputFormatArg::Binary => OutputFormat::Binary,
            OutputFormatArg::Srecord => OutputFormat::SRecord,
            OutputFormatArg::IntelHex => OutputFormat::IntelHex,
            OutputFormatArg::Elf => OutputFormat::Elf,
            OutputFormatArg::Ieee695 => OutputFormat::Ieee695,
            // HunkExe has no OutputFormat counterpart — it's written directly
            // via amiga_hunk_writer::generate_hunk_exe, bypassing OutputFormat
            // entirely (same as Elf/Ieee695 already do in `run()`'s match).
            OutputFormatArg::HunkExe => OutputFormat::Elf,
        }
    }
}

fn parse_origin(s: &str) -> Result<u32, String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('$') {
        u32::from_str_radix(hex, 16).map_err(|e| format!("invalid hex origin: {}", e))
    } else if let Some(hex) = s.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).map_err(|e| format!("invalid hex origin: {}", e))
    } else {
        s.parse::<u32>()
            .map_err(|e| format!("invalid origin: {}", e))
    }
}

/// `-` means stdin/stdout, as documented in the README. Everything else is a
/// filesystem path — including `./-`, which is the escape hatch for a file
/// literally named `-`.
fn is_dash(p: &Path) -> bool {
    p.as_os_str() == "-"
}

/// Writes generated output to a file, or to stdout when the path is `-`.
/// Binary formats go out as raw bytes, so stdout must not be line-buffered
/// text here.
fn write_output(path: &Path, data: &[u8]) -> Result<(), String> {
    if is_dash(path) {
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        lock.write_all(data)
            .and_then(|()| lock.flush())
            .map_err(|e| format!("cannot write to stdout: {}", e))
    } else {
        fs::write(path, data).map_err(|e| format!("cannot write '{}': {}", path.display(), e))
    }
}

fn output_display(path: &Path) -> String {
    if is_dash(path) {
        "<stdout>".to_string()
    } else {
        path.display().to_string()
    }
}

fn run(args: Args) -> Result<(), String> {
    m68k_core::cpu_gate::validate_cpu_name(&args.cpu)?;
    let origin = parse_origin(&args.origin)?;
    let reading_stdin = is_dash(&args.input);
    let input_name = if reading_stdin {
        "<stdin>".to_string()
    } else {
        args.input.to_string_lossy().to_string()
    };

    // Read source file (or stdin for `-`)
    let source = if reading_stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("{}: cannot read: {}", input_name, e))?;
        buf
    } else {
        fs::read_to_string(&args.input)
            .map_err(|e| format!("{}: cannot read: {}", input_name, e))?
    };

    // Assemble
    let mut asm = Assembler::new(origin);
    asm.set_cpu(&args.cpu);
    asm.set_optimize(args.optimize);
    for dir in &args.include_paths {
        asm.add_include_path(dir.clone());
    }
    // With stdin there is no source directory to resolve relative INCLUDEs
    // against; the current directory (and any -I paths) is all we have.
    if !reading_stdin && let Some(parent) = args.input.parent() {
        asm.set_source_root(parent.to_path_buf());
    }
    asm.errors.filename = input_name.clone();

    asm.assemble_bytes(&source)
        .map_err(|e| format!("{}: {}", input_name, e))?;

    // Print warnings and errors with filename
    for diag in &asm.errors.warnings {
        eprintln!("{}", format_diagnostic(diag, &input_name));
    }
    for diag in &asm.errors.errors {
        eprintln!("{}", format_diagnostic(diag, &input_name));
    }

    // The ERROR directive (and other non-fatal diagnostics collected into
    // asm.errors rather than surfaced via assemble_bytes's Result) doesn't
    // stop assembly, so assemble_bytes above can return Ok even though the
    // source contains an ERROR. Previously nothing checked has_errors()
    // here, so the CLI printed the error but still wrote an output file
    // and exited 0 — a build script driving this CLI would see success.
    if asm.errors.has_errors() {
        return Err(format!(
            "{}: assembly reported errors, no output written",
            input_name
        ));
    }

    // Determine output path. Deriving one from the input name is impossible
    // when the source came from stdin, so `-o` is required in that case
    // (`-o -` to pipe the result onward).
    let output_path = match args.output.clone() {
        Some(p) => p,
        None if reading_stdin => {
            return Err(
                "reading source from stdin requires an explicit -o/--output (use `-o -` for stdout)"
                    .to_string(),
            );
        }
        None => {
            let mut p = args.input.clone();
            match args.format {
                OutputFormatArg::Binary => {
                    p.set_extension("bin");
                }
                OutputFormatArg::Srecord => {
                    p.set_extension("srec");
                }
                OutputFormatArg::IntelHex => {
                    p.set_extension("hex");
                }
                OutputFormatArg::Elf => {
                    p.set_extension("o");
                }
                OutputFormatArg::Ieee695 => {
                    p.set_extension("ieee");
                }
                OutputFormatArg::HunkExe => {
                    p.set_extension("");
                }
            }
            p
        }
    };

    // Generate output
    let output_data = match args.format {
        OutputFormatArg::Binary => {
            // Previously concatenated `instr.words` directly, ignoring
            // `instr.pc` entirely — a gap between ORG-separated blocks
            // (e.g. `ORG $1000 / NOP / ORG $1010 / RTS`) collapsed to
            // nothing, producing a file inconsistent with S-Record/Intel-
            // Hex/ELF (which all honor `pc`) and silently dropping DS
            // reservations. generate_binary fills gaps with zero bytes
            // between the lowest and highest instruction address instead.
            const MAX_BINARY_SIZE: usize = 16 * 1024 * 1024; // 68k address space
            // Pass the final PC so a trailing DS/DCB reservation (which
            // emits no instruction) still sizes the image.
            match generate_binary_to(&asm.code, Some(asm.end_pc())) {
                Some((bytes, _base_addr)) => {
                    if bytes.len() > MAX_BINARY_SIZE {
                        return Err(format!(
                            "binary output would be {} bytes (limit {} bytes) \
                             — check for a large gap between ORG-separated blocks",
                            bytes.len(),
                            MAX_BINARY_SIZE
                        ));
                    }
                    bytes
                }
                None => return Err("no code generated".to_string()),
            }
        }
        OutputFormatArg::Srecord => {
            let srec = generate_srecord(&asm.code, &args.srec_name);
            write_output(&output_path, srec.as_bytes())?;
            eprintln!(
                "Assembled {} instructions, {} bytes -> {}",
                asm.code.len(),
                srec.len(),
                output_display(&output_path)
            );
            return write_auxiliary_outputs(&asm, &args, &input_name);
        }
        OutputFormatArg::IntelHex => {
            let ihex = generate_intel_hex(&asm.code);
            write_output(&output_path, ihex.as_bytes())?;
            eprintln!(
                "Assembled {} instructions, {} bytes -> {}",
                asm.code.len(),
                ihex.len(),
                output_display(&output_path)
            );
            return write_auxiliary_outputs(&asm, &args, &input_name);
        }
        OutputFormatArg::Elf => {
            let elf = generate_elf_sections(&asm.sections, &asm.symbols);
            write_output(&output_path, &elf)?;
            eprintln!(
                "Assembled {} instructions, {} bytes -> {}",
                asm.code.len(),
                elf.len(),
                output_display(&output_path)
            );
            return write_auxiliary_outputs(&asm, &args, &input_name);
        }
        OutputFormatArg::Ieee695 => {
            let ieee = generate_ieee695_sections(&asm.sections, &asm.symbols);
            write_output(&output_path, &ieee)?;
            eprintln!(
                "Assembled {} instructions, {} bytes -> {}",
                asm.code.len(),
                ieee.len(),
                output_display(&output_path)
            );
            return write_auxiliary_outputs(&asm, &args, &input_name);
        }
        OutputFormatArg::HunkExe => {
            let hunk = generate_hunk_exe(&asm.sections, &asm.symbols);
            write_output(&output_path, &hunk)?;
            eprintln!(
                "Assembled {} instructions, {} bytes -> {}",
                asm.code.len(),
                hunk.len(),
                output_display(&output_path)
            );
            return write_auxiliary_outputs(&asm, &args, &input_name);
        }
    };

    // Write binary output
    write_output(&output_path, &output_data)?;

    // Summary
    eprintln!(
        "Assembled {} instructions, {} bytes -> {}",
        asm.code.len(),
        output_data.len(),
        output_display(&output_path)
    );

    write_auxiliary_outputs(&asm, &args, &input_name)
}

fn format_diagnostic(diag: &m68k_core::errors::Diagnostic, filename: &str) -> String {
    let prefix = match diag.severity {
        m68k_core::errors::Severity::Error => "error",
        m68k_core::errors::Severity::Warning => "warning",
    };
    match diag.line_no {
        Some(line) => format!("{}:{}: {}: {}", filename, line, prefix, diag.message),
        None => format!("{}: {}: {}", filename, prefix, diag.message),
    }
}

fn write_auxiliary_outputs(asm: &Assembler, args: &Args, input_name: &str) -> Result<(), String> {
    // Generate listing
    if let Some(listing_path) = &args.listing {
        let mut listing = String::new();
        listing.push_str(&format!("; Listing for {}\n\n", input_name));
        listing.push_str(&format!("{:<8} {:<20} {}\n", "Address", "Bytes", "Source"));
        listing.push_str(&format!("{:-<8} {:-<20} {:-<}\n", "", "", ""));
        for instr in &asm.code {
            let bytes_str: Vec<String> = instr
                .words
                .iter()
                .flat_map(|w| {
                    vec![
                        format!("{:02X}", (w >> 8) as u8),
                        format!("{:02X}", (w & 0xFF) as u8),
                    ]
                })
                .collect();
            let source = instr.source.as_deref().unwrap_or("").trim();
            listing.push_str(&format!(
                "${:06X}  {:<20} {}\n",
                instr.pc,
                bytes_str.join(" "),
                source
            ));
        }
        fs::write(listing_path, &listing)
            .map_err(|e| format!("cannot write listing '{}': {}", listing_path.display(), e))?;
        eprintln!("Listing written -> {}", listing_path.display());
    }

    // Export symbol table
    if let Some(sym_path) = &args.sym {
        let mut sym_out = String::new();
        sym_out.push_str(&format!("; Symbol table for {}\n\n", input_name));
        let mut syms: Vec<_> = asm.symbols.iter().collect();
        syms.sort_by_key(|(name, _)| name.to_lowercase());
        for (name, entry) in &syms {
            sym_out.push_str(&format!("{:<20} = ${:06X}\n", name, entry.value));
        }
        fs::write(sym_path, &sym_out)
            .map_err(|e| format!("cannot write symbol table '{}': {}", sym_path.display(), e))?;
        eprintln!("Symbol table written -> {}", sym_path.display());
    }

    // Generate memory map
    if let Some(map_path) = &args.map {
        let mut map_out = String::new();
        map_out.push_str(&format!("; Memory map for {}\n\n", input_name));
        let mut total_bytes = 0u32;
        for instr in &asm.code {
            let size = instr.size_bytes() as u32;
            map_out.push_str(&format!(
                "${:06X}-${:06X}  {:>6} bytes  {}\n",
                instr.pc,
                instr.pc + size - 1,
                size,
                instr.source.as_deref().unwrap_or("").trim()
            ));
            total_bytes += size;
        }
        map_out.push_str(&format!("\nTotal: {} bytes\n", total_bytes));
        fs::write(map_path, &map_out)
            .map_err(|e| format!("cannot write memory map '{}': {}", map_path.display(), e))?;
        eprintln!("Memory map written -> {}", map_path.display());
    }

    // Print symbol table to stderr (always)
    if asm.symbols.iter().count() > 0 {
        eprintln!("\nSymbols:");
        let mut syms: Vec<_> = asm.symbols.iter().collect();
        syms.sort_by_key(|(name, _)| name.to_lowercase());
        for (name, entry) in syms {
            eprintln!("  {:<20} ${:08X}", name, entry.value);
        }
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
