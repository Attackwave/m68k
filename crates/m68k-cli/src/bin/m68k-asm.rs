use std::fs;
use std::path::PathBuf;
use std::process;

use clap::{Parser, ValueEnum};
use m68k_asm::amiga_hunk_writer::generate_hunk_exe;
use m68k_asm::assembler::Assembler;
use m68k_asm::ieee695::generate_ieee695_sections;
use m68k_asm::output::{OutputFormat, generate_elf_sections, generate_intel_hex, generate_srecord};

#[derive(Parser, Debug)]
#[command(name = "m68k-asm")]
#[command(about = "Motorola 68000 assembler", long_about = None)]
struct Args {
    /// Input assembly source file
    input: PathBuf,

    /// Output file (default: stdout, or input with .bin/.srec/.hex extension)
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

fn run(args: Args) -> Result<(), String> {
    let origin = parse_origin(&args.origin)?;
    let input_name = args.input.to_string_lossy().to_string();

    // Read source file
    let source = fs::read_to_string(&args.input)
        .map_err(|e| format!("{}: cannot read: {}", input_name, e))?;

    // Assemble
    let mut asm = Assembler::new(origin);
    asm.set_cpu(&args.cpu);
    if let Some(parent) = args.input.parent() {
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

    // Determine output path
    let output_path = args.output.clone().unwrap_or_else(|| {
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
    });

    // Generate output
    let output_data = match args.format {
        OutputFormatArg::Binary => {
            let mut bytes = Vec::new();
            for instr in &asm.code {
                for word in &instr.words {
                    bytes.push((word >> 8) as u8);
                    bytes.push((word & 0xFF) as u8);
                }
            }
            if bytes.is_empty() {
                return Err("no code generated".to_string());
            }
            bytes
        }
        OutputFormatArg::Srecord => {
            let srec = generate_srecord(&asm.code, &args.srec_name);
            fs::write(&output_path, &srec)
                .map_err(|e| format!("cannot write '{}': {}", output_path.display(), e))?;
            eprintln!(
                "Assembled {} instructions, {} bytes -> {}",
                asm.code.len(),
                srec.len(),
                output_path.display()
            );
            return write_auxiliary_outputs(&asm, &args, &input_name);
        }
        OutputFormatArg::IntelHex => {
            let ihex = generate_intel_hex(&asm.code);
            fs::write(&output_path, &ihex)
                .map_err(|e| format!("cannot write '{}': {}", output_path.display(), e))?;
            eprintln!(
                "Assembled {} instructions, {} bytes -> {}",
                asm.code.len(),
                ihex.len(),
                output_path.display()
            );
            return write_auxiliary_outputs(&asm, &args, &input_name);
        }
        OutputFormatArg::Elf => {
            let elf = generate_elf_sections(&asm.sections, &asm.symbols);
            fs::write(&output_path, &elf)
                .map_err(|e| format!("cannot write '{}': {}", output_path.display(), e))?;
            eprintln!(
                "Assembled {} instructions, {} bytes -> {}",
                asm.code.len(),
                elf.len(),
                output_path.display()
            );
            return write_auxiliary_outputs(&asm, &args, &input_name);
        }
        OutputFormatArg::Ieee695 => {
            let ieee = generate_ieee695_sections(&asm.sections, &asm.symbols);
            fs::write(&output_path, &ieee)
                .map_err(|e| format!("cannot write '{}': {}", output_path.display(), e))?;
            eprintln!(
                "Assembled {} instructions, {} bytes -> {}",
                asm.code.len(),
                ieee.len(),
                output_path.display()
            );
            return write_auxiliary_outputs(&asm, &args, &input_name);
        }
        OutputFormatArg::HunkExe => {
            let hunk = generate_hunk_exe(&asm.sections, &asm.symbols);
            fs::write(&output_path, &hunk)
                .map_err(|e| format!("cannot write '{}': {}", output_path.display(), e))?;
            eprintln!(
                "Assembled {} instructions, {} bytes -> {}",
                asm.code.len(),
                hunk.len(),
                output_path.display()
            );
            return write_auxiliary_outputs(&asm, &args, &input_name);
        }
    };

    // Write binary output
    if output_path.to_str() == Some("-") {
        use std::io::Write;
        std::io::stdout()
            .write_all(&output_data)
            .map_err(|e| format!("cannot write to stdout: {}", e))?;
    } else {
        fs::write(&output_path, &output_data)
            .map_err(|e| format!("cannot write '{}': {}", output_path.display(), e))?;
    }

    // Summary
    eprintln!(
        "Assembled {} instructions, {} bytes -> {}",
        asm.code.len(),
        output_data.len(),
        output_path.display()
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
