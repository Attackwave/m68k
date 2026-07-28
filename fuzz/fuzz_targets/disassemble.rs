//! Fuzz the disassembler's two-pass driver over arbitrary byte input,
//! across all recognized CPU levels. History: the disassembler previously
//! had no dedicated fuzz coverage, and the assembler side found several
//! panics on malformed/adversarial input during the July 2026 audit
//! (integer overflow on DS/DCB sizes, shift-count panics in expr.rs).
#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_disasm::disassembler::Disassembler;

const CPUS: &[&str] = &["68000", "68010", "68020", "68030", "68040", "68060"];

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let cpu = CPUS[data[0] as usize % CPUS.len()];
    let mut disasm = Disassembler::new(data[1..].to_vec(), 0x1000);
    disasm.set_cpu(cpu);
    let _ = disasm.disassemble();
});
