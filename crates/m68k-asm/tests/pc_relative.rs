//! Every instruction that takes a PC-relative operand must encode a
//! displacement that reaches the label it names.
//!
//! The CPU adds the displacement to the address of the extension word
//! holding it. That word is not always right after the opword: a bit
//! number (`BTST #n,d16(PC)`), an FPU command word or a bit field
//! specifier comes first. Each form is assembled at a fixed address, decoded
//! again, and the decoded target has to be the label.

use std::collections::HashMap;

use m68k_asm::assembler::Assembler;
use m68k_core::addressing::InstructionStream;
use m68k_disasm::decoder::{DecodeResult, decode_next};

const ORG: u32 = 0x1000;

/// Assemble `insn` at `ORG`, followed by padding and the target label, and
/// return the decoded instruction text with the target shown as `TGT`.
fn decoded(insn: &str, cpu: &str) -> String {
    let src = format!(" ORG ${ORG:X}\n {insn}\n dcb.w 3,$4E71\ntgt: rts\n");
    let mut asm = Assembler::new(ORG);
    asm.set_cpu(cpu);
    let bytes = asm
        .assemble_bytes(&src)
        .unwrap_or_else(|e| panic!("{insn}: {}", e.message));
    let target = asm.symbols.get("tgt").unwrap().value;
    let labels = HashMap::from([(target, "TGT".to_string())]);
    let mut stream = InstructionStream::new(&bytes, ORG);
    let text = match decode_next(&mut stream, cpu) {
        Ok((_, DecodeResult::Instruction(i))) => i.format(&labels),
        other => panic!("{insn}: did not decode: {other:?}"),
    };
    // Bit field operands are printed with their address rather than a label.
    text.replace(&format!("${target:08x}(pc"), "TGT(pc")
}

fn check(cpu: &str, forms: &[&str]) {
    let wrong: Vec<String> = forms
        .iter()
        .flat_map(|form| {
            [
                form.replace("@", "tgt(pc)"),
                form.replace("@", "tgt(pc,d0.w)"),
            ]
        })
        .filter_map(|insn| {
            let text = decoded(&insn, cpu);
            (!text.contains("TGT")).then(|| format!("{insn:32} -> {text}"))
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} forms miss their target:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn mc68000_forms() {
    check(
        "68000",
        &[
            "jmp @",
            "jsr @",
            "lea @,a0",
            "pea @",
            "move.w @,d1",
            "move.l @,$2000",
            "movea.w @,a1",
            "move.w @,ccr",
            "move.w @,sr",
            "add.w @,d1",
            "sub.l @,d1",
            "and.w @,d1",
            "or.b @,d1",
            "cmp.w @,d1",
            "adda.w @,a1",
            "suba.l @,a1",
            "cmpa.w @,a1",
            "mulu.w @,d1",
            "muls.w @,d1",
            "divu.w @,d1",
            "divs.w @,d1",
            "chk.w @,d1",
            "btst d0,@",
            "btst #1,@",
            "movem.w @,d0-d1",
            "movem.l @,d0/a0",
        ],
    );
}

#[test]
fn mc68020_forms() {
    check(
        "68020",
        &[
            "tst.w @",
            "chk.l @,d1",
            "bftst @{0:8}",
            "bfextu @{0:8},d1",
            "bfexts @{0:8},d1",
            "bfffo @{0:8},d1",
            "chk2.w @,d1",
            "cmp2.l @,d1",
            "mulu.l @,d1",
            "muls.l @,d1:d2",
            "divu.l @,d1",
            "divs.l @,d1:d2",
            "divsl.l @,d1:d2",
            "divul.l @,d1:d2",
            "callm #0,@",
        ],
    );
}

#[test]
fn fpu_forms() {
    check(
        "68040",
        &[
            "fmove.s @,fp0",
            "fadd.d @,fp1",
            "fcmp.x @,fp2",
            "fmove.l @,fpcr",
            "fmovem.x @,fp0-fp1",
            "fsincos.x @,fp0:fp1",
            "frestore @",
        ],
    );
}

#[test]
fn mmu_forms() {
    check("68030", &["pmove @,tc"]);
}
