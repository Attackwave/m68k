//! Fuzz the TOS program reader and the disassembler over what it loads.
//! The header's segment and symbol table lengths and the relocation
//! table's steps all come straight from the file.
#![no_main]

use libfuzzer_sys::fuzz_target;
use m68k_core::atari_prg::read_prg;
use m68k_disasm::disassembler::Disassembler;

fuzz_target!(|data: &[u8]| {
    for base in [0u32, 0x20000, 0xFFFF_FFF0] {
        let Ok(prg) = read_prg(data, base) else {
            continue;
        };
        if prg.image.is_empty() {
            continue;
        }
        let mut disasm = Disassembler::new(prg.image, prg.load_base);
        disasm.add_pointer_sites(prg.relocs);
        disasm.add_known_labels(prg.symbols);
        disasm.add_entry_points([prg.load_base]);
        let _ = disasm.disassemble();
    }
});
