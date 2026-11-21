//! Fuzz `--syntax gas`: parser, expression evaluator, layout, relaxation
//! and fixup search on arbitrary source text. A NUL byte splits the input
//! into several files, so cross-file symbols and far branches are reached.
#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_asm::gas::{assemble, SourceFile};

const CPUS: &[&str] = &["68000", "68010", "68020", "68030", "68040", "68060"];

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    let cpu = CPUS[first as usize % CPUS.len()];
    let files: Vec<SourceFile> = rest
        .split(|&b| b == 0)
        .take(4)
        .enumerate()
        .map(|(i, text)| SourceFile {
            name: format!("f{i}.s"),
            text: String::from_utf8_lossy(text).into_owned(),
        })
        .collect();
    let _ = assemble(&files, cpu, 0);
});
