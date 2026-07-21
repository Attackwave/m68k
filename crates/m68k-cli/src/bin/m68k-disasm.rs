use std::fs;
use std::path::PathBuf;
use std::process;

use clap::Parser;
use m68k_disasm::disassembler::Disassembler;

#[derive(Parser, Debug)]
#[command(name = "m68k-disasm")]
#[command(about = "Motorola 68000 disassembler", long_about = None)]
struct Args {
    /// Input binary file to disassemble
    input: PathBuf,

    /// Start address (hex, e.g. $1000 or 0x1000)
    #[arg(short, long, default_value = "0")]
    address: String,

    /// CPU target
    #[arg(short, long, default_value = "68000")]
    cpu: String,

    /// Show raw bytes before each instruction
    #[arg(short = 'r', long)]
    raw: bool,
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

fn run(args: Args) -> Result<(), String> {
    let start_addr = parse_address(&args.address)?;

    let data = fs::read(&args.input)
        .map_err(|e| format!("cannot read '{}': {}", args.input.display(), e))?;

    if data.is_empty() {
        return Err("empty input file".to_string());
    }

    let mut disasm = Disassembler::new(data, start_addr);
    disasm.set_cpu(&args.cpu);

    for line in disasm.disassemble() {
        if line.is_error {
            eprintln!("{}", line.text);
            continue;
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
