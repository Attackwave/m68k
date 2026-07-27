//! Fuzz the full two-pass assembler pipeline on arbitrary source text.
//! Catches the class of bug found in the July 2026 audit (1.4): integer
//! overflow panics from user-controlled sizes in DS/DCB directives
//! (`DS.L $80000000` panicking on `element_size * count` without
//! `checked_mul`), and similar unchecked-arithmetic/allocation panics
//! anywhere else in the parser, expression evaluator, or two-pass driver.
#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_asm::assembler::Assembler;

const CPUS: &[&str] = &["68000", "68010", "68020", "68030", "68040", "68060"];

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let cpu = CPUS[data[0] as usize % CPUS.len()];
    let source = String::from_utf8_lossy(&data[1..]);
    let mut asm = Assembler::new(0x1000);
    asm.set_cpu(cpu);
    let _ = asm.assemble_bytes(&source);
});
