use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

use clap::{Parser, ValueEnum};
use m68k_asm::amiga_hunk_writer::{LineOf, generate_relocatable_hunk_exe};
use m68k_asm::assembler::Assembler;
use m68k_asm::ieee695::generate_ieee695_sections;
use m68k_asm::output::{
    OutputFormat, generate_binary_from, generate_elf_sections, generate_intel_hex, generate_srecord,
};

#[derive(Parser, Debug)]
#[command(name = "m68k-asm", version)]
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

    /// Add source line debug info (`LINE` debug hunks) to a hunk
    /// executable, for source-level debugging.
    #[arg(long)]
    linedebug: bool,

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

    /// Treat warnings as errors: any warning fails the run and no output
    /// is written
    #[arg(long = "Werror")]
    werror: bool,

    /// Check every routine's stack use: a return or a jump to another
    /// routine with the stack pointer off its entry level, two paths
    /// reaching a line with different stack heights, or a point from
    /// which the stack height cannot be followed, is an error
    #[arg(long)]
    stack_check: bool,

    /// Write a report of the maximum stack depth of every routine,
    /// including everything it calls; recursion is marked unbounded and
    /// an unknown callee makes the figure a lower bound
    #[arg(long, value_name = "FILE")]
    stack_report: Option<PathBuf>,

    /// Warn about TST/CMP/BTST whose condition codes every following path
    /// overwrites before any instruction reads them
    #[arg(long)]
    flag_check: bool,

    /// Write a make dependency file: the output depends on the source
    /// and every INCLUDE/INCBIN file, each also listed as an empty rule
    /// so a deleted file does not break the build (like `gcc -MD -MP`)
    #[arg(long, value_name = "FILE")]
    depfile: Option<PathBuf>,
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

/// Quote a path for a make rule: a space or `#` would end the name or
/// start a comment, a `$` would start a variable reference.
fn make_escape(path: &Path) -> String {
    let mut out = String::new();
    // GNU make reads `/` on every platform; a Windows backslash would be
    // an escape character in a rule.
    for c in path.to_string_lossy().replace('\\', "/").chars() {
        match c {
            ' ' | '#' => {
                out.push('\\');
                out.push(c);
            }
            '$' => out.push_str("$$"),
            _ => out.push(c),
        }
    }
    out
}

/// `source` is `None` when it came from stdin. It gets no empty rule of
/// its own: without it there is nothing to build anyway.
fn depfile_contents(target: &Path, source: Option<&Path>, deps: &[PathBuf]) -> String {
    let mut out = format!("{}:", make_escape(target));
    for dep in source.into_iter().chain(deps.iter().map(PathBuf::as_path)) {
        out.push_str(" \\\n  ");
        out.push_str(&make_escape(dep));
    }
    out.push('\n');
    for dep in deps {
        out.push_str(&format!("\n{}:\n", make_escape(dep)));
    }
    out
}

/// A mnemonic that is an instruction, as opposed to data or a directive,
/// so a missing timing is worth counting.
fn is_timed_instruction(mnemonic: &str) -> bool {
    !mnemonic.is_empty() && !m68k_core::tokens::is_directive(mnemonic)
}

/// One line per routine: name, address, depth, balance, then notes.
fn stack_report_text(asm: &Assembler) -> String {
    let reports = asm.stack_analysis();
    let width = reports
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(7)
        .max(7);
    let mut out = format!(
        "{:<width$}  {:<8}  {:<16}  balance\n",
        "routine", "address", "depth"
    );
    for r in &reports {
        let balance = match r.balanced {
            Some(true) => "ok",
            Some(false) => "MISMATCH",
            None => "n/a",
        };
        out.push_str(&format!(
            "{:<width$}  ${:06X}  {:<16}  {balance}\n",
            r.name,
            r.pc,
            r.depth.to_string()
        ));
        for (line, p) in &r.problems {
            let diag =
                asm.locate_diagnostic(m68k_core::errors::Diagnostic::error(p.clone(), Some(*line)));
            let where_ = match diag.line_no {
                Some(l) if !diag.filename.is_empty() => format!("{}:{l}", diag.filename),
                Some(l) => format!("line {l}"),
                None => String::new(),
            };
            out.push_str(&format!("{:width$}    {where_}: {p}\n", ""));
        }
        for n in &r.notes {
            out.push_str(&format!("{:width$}    {n}\n", ""));
        }
    }
    out
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

    // Assemble. The hunk writer assembles again with sections moved, so the
    // setup lives in one place.
    let new_assembler = |origin: u32| {
        let mut asm = Assembler::new(origin);
        asm.set_cpu(&args.cpu);
        asm.set_optimize(args.optimize);
        for dir in &args.include_paths {
            asm.add_include_path(dir.clone());
        }
        // With stdin there is no source directory to resolve relative
        // INCLUDEs against; the current directory (and any -I paths) is all
        // we have.
        if !reading_stdin && let Some(parent) = args.input.parent() {
            asm.set_source_root(parent.to_path_buf());
        }
        asm.errors.filename = input_name.clone();
        asm
    };
    let mut asm = new_assembler(origin);

    // Everything the assembler found, located in the file it was written
    // in, in source order. The run fails on any error, or on a warning
    // with --Werror; nothing is written then.
    let result = asm.assemble_bytes(&source);
    // The analyses read the encoded program, so they only make sense when
    // it assembled; their findings join the other diagnostics.
    if result.is_ok() {
        // Written before any error exit: the report is how a reported
        // problem is looked into.
        if let Some(report_path) = &args.stack_report {
            fs::write(report_path, stack_report_text(&asm)).map_err(|e| {
                format!(
                    "cannot write stack report '{}': {}",
                    report_path.display(),
                    e
                )
            })?;
        }
        if args.stack_check {
            for report in asm.stack_analysis() {
                for (line, problem) in report.problems {
                    asm.errors
                        .error(format!("{}: {problem}", report.name), Some(line));
                }
            }
        }
        if args.flag_check {
            for (line, message) in asm.dead_flag_setters() {
                asm.errors.warning(message, Some(line));
            }
        }
    }
    for diag in asm.diagnostics() {
        eprintln!("{diag}");
    }
    let errors = asm.errors.errors.len() + usize::from(result.is_err() && !asm.errors.has_errors());
    let warnings = asm.errors.warnings.len();
    if errors > 0 {
        return Err(format!(
            "{input_name}: {errors} error(s), {warnings} warning(s), no output written"
        ));
    }
    if args.werror && warnings > 0 {
        return Err(format!(
            "{input_name}: {warnings} warning(s) treated as errors (--Werror), no output written"
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
    // Written ahead of the output, which has several exit paths below.
    if let Some(dep_path) = &args.depfile {
        if is_dash(&output_path) {
            return Err(
                "--depfile needs an output file to name as the make target, not stdout".into(),
            );
        }
        let source = (!reading_stdin).then_some(args.input.as_path());
        fs::write(
            dep_path,
            depfile_contents(&output_path, source, &asm.dependencies),
        )
        .map_err(|e| format!("cannot write depfile '{}': {}", dep_path.display(), e))?;
    }

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
            match generate_binary_from(&asm.code, Some(asm.end_pc()), Some(asm.origin())) {
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
            // Debuggers open these paths, so they are absolute; diagnostics
            // keep the short names.
            // `absolute`, not `canonicalize`: no `\\?\` prefix on Windows,
            // and symlinks stay as the editor sees them.
            let main_file = std::path::absolute(&args.input).unwrap_or_else(|_| args.input.clone());
            let line_of = |line_no: usize| {
                let origin = asm.source_origin(line_no)?;
                let path = match &origin.file {
                    Some(name) => asm.include_paths_resolved.get(name)?,
                    None => &main_file,
                };
                Some((path.to_string_lossy().into_owned(), origin.line as u32))
            };
            let line_of: Option<LineOf> = args.linedebug.then_some(&line_of as LineOf);
            let hunk = generate_relocatable_hunk_exe(&asm, line_of, |origin, overrides| {
                let mut moved = new_assembler(origin);
                for (kind, at) in overrides {
                    moved.sections.set_origin_override(kind.clone(), *at);
                }
                moved.assemble(&source)?;
                Ok(moved)
            })
            .map_err(|e| {
                let mut asm_err = m68k_core::errors::Diagnostic::from(e);
                asm_err.filename = String::new();
                asm.locate_diagnostic(asm_err).to_string()
            })?;
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

fn write_auxiliary_outputs(asm: &Assembler, args: &Args, input_name: &str) -> Result<(), String> {
    // Generate listing
    if let Some(listing_path) = &args.listing {
        let mut listing = String::new();
        listing.push_str(&format!("; Listing for {}\n\n", input_name));
        listing.push_str(&format!(
            "{:<8} {:<20} {:<7} {}\n",
            "Address", "Bytes", "Cycles", "Source"
        ));
        listing.push_str(&format!("{:-<8} {:-<20} {:-<7} {:-<}\n", "", "", "", ""));
        // 68000 clock periods per instruction (M68000 user's manual,
        // section 8), summed at the end. Lines without a 68000 timing
        // (data, 68020+ forms) stay blank and are counted.
        let (mut cyc_min, mut cyc_max, mut cyc_unknown) = (0u32, Some(0u32), 0u32);
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
            let (_, mnemonic, size, operands) = m68k_core::tokens::split_line(source);
            let timing = m68k_core::cycles::instruction_timing(&mnemonic, &size, &operands);
            let cycles = match &timing {
                Some(t) => {
                    cyc_min += t.min;
                    cyc_max = cyc_max.zip(t.max).map(|(a, b)| a + b);
                    t.label.clone()
                }
                None => {
                    if is_timed_instruction(&mnemonic) {
                        cyc_unknown += 1;
                    }
                    String::new()
                }
            };
            listing.push_str(&format!(
                "${:06X}  {:<20} {:<7} {}\n",
                instr.pc,
                bytes_str.join(" "),
                cycles,
                source
            ));
        }
        let max = cyc_max.map_or("no bound".to_string(), |m| m.to_string());
        listing.push_str(&format!(
            "\n; 68000 cycles: {cyc_min} min, {max} max, {cyc_unknown} instructions without timing\n"
        ));
        fs::write(listing_path, &listing)
            .map_err(|e| format!("cannot write listing '{}': {}", listing_path.display(), e))?;
        eprintln!("Listing written -> {}", listing_path.display());
    }

    // Export symbol table
    if let Some(sym_path) = &args.sym {
        let mut sym_out = String::new();
        sym_out.push_str(&format!("; Symbol table for {}\n\n", input_name));
        let mut syms: Vec<_> = asm.symbols.iter().collect();
        // Case-insensitive order, ties broken by the exact name: `err_exist`
        // and `ERR_EXIST` otherwise came out in hash-map order, so the file
        // differed from run to run.
        syms.sort_by_cached_key(|(name, _)| (name.to_lowercase(), name.to_string()));
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
        // Case-insensitive order, ties broken by the exact name: `err_exist`
        // and `ERR_EXIST` otherwise came out in hash-map order, so the file
        // differed from run to run.
        syms.sort_by_cached_key(|(name, _)| (name.to_lowercase(), name.to_string()));
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
