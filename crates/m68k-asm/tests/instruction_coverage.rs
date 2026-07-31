//! Instruction-set coverage: every supported instruction form must
//! assemble, decode back to a real instruction (not a `dc.w` data-word
//! fallback), and re-assemble to the exact same bytes.
//!
//! This is the guard that keeps the assembler and disassembler in step.
//! The two sides are implemented independently — the assembler dispatches
//! on mnemonic strings, the disassembler matches opcode bit patterns — so
//! an instruction can be perfectly encodable while being entirely
//! undecodable, with nothing failing to signal it. That is not
//! hypothetical: when this test was first written it found 13 instructions
//! (Scc with any condition but `ST`, MULU.L/MULS.L/DIVU.L/DIVS.L/DIVSL.L/
//! DIVUL.L, CAS, CHK2/CMP2, TRAPcc, RTM, CALLM, MOVES, MOVE16, PSAVE,
//! PRESTORE) that the assembler emitted and the disassembler rendered as
//! raw data words, plus two silently wrong encoders (MOVEA used the wrong
//! size bits *and* addressed a data register instead of an address
//! register; MOVES dropped both its size and its direction bit).
//!
//! Every case here is a byte-for-byte roundtrip rather than a spot-check
//! of the decoded text, so a decoder that produces plausible-looking but
//! wrong output fails too.

use m68k_asm::assembler::Assembler;

/// One instruction form to check, with the CPU it needs.
struct Case {
    source: &'static str,
    cpu: &'static str,
}

const fn c(source: &'static str, cpu: &'static str) -> Case {
    Case { source, cpu }
}

/// Base 68000 instruction set — the most important group: this is what an
/// Amiga (68000/68020) actually runs.
const BASE_68000: &[Case] = &[
    // Data movement
    c("MOVE.B D0,D1", "68000"),
    c("MOVE.W D0,D1", "68000"),
    c("MOVE.L D0,D1", "68000"),
    c("MOVE.L (A0),(A1)", "68000"),
    c("MOVE.L #$12345678,D0", "68000"),
    c("MOVEA.W D0,A1", "68000"),
    c("MOVEA.L A0,A1", "68000"),
    c("MOVEA.L (A0),A1", "68000"),
    c("MOVEQ #5,D0", "68000"),
    c("MOVEQ #-1,D0", "68000"),
    c("MOVEM.L D0-D7,-(A7)", "68000"),
    c("MOVEM.L (A7)+,D0-D7", "68000"),
    c("MOVEP.W (4,A0),D0", "68000"),
    c("MOVEP.L D0,(4,A0)", "68000"),
    c("MOVE.W SR,D0", "68000"),
    c("MOVE.W D0,CCR", "68000"),
    c("MOVE.L A0,USP", "68000"),
    // Integer arithmetic
    c("ADD.W D0,D1", "68000"),
    c("ADD.W D0,(A1)", "68000"),
    c("ADDA.W D0,A1", "68000"),
    c("ADDA.L A0,A1", "68000"),
    c("ADDI.W #5,D0", "68000"),
    c("ADDQ.W #4,D0", "68000"),
    c("ADDQ.L #8,A0", "68000"),
    c("ADDX.W D0,D1", "68000"),
    c("ADDX.W -(A0),-(A1)", "68000"),
    c("SUB.W D0,D1", "68000"),
    c("SUBA.L A0,A1", "68000"),
    c("SUBI.W #5,D0", "68000"),
    c("SUBQ.W #4,D0", "68000"),
    c("SUBX.W D0,D1", "68000"),
    c("MULU.W D0,D1", "68000"),
    c("MULS.W D0,D1", "68000"),
    c("DIVU.W D0,D1", "68000"),
    c("DIVS.W D0,D1", "68000"),
    c("NEG.W D0", "68000"),
    c("NEGX.W D0", "68000"),
    c("CLR.W D0", "68000"),
    c("EXT.W D0", "68000"),
    c("EXT.L D0", "68000"),
    // BCD
    c("ABCD D0,D1", "68000"),
    c("ABCD -(A0),-(A1)", "68000"),
    c("SBCD D0,D1", "68000"),
    c("NBCD D0", "68000"),
    // Logic
    c("AND.W D0,D1", "68000"),
    c("ANDI.W #5,D0", "68000"),
    c("OR.W D0,D1", "68000"),
    c("ORI.W #5,D0", "68000"),
    c("EOR.W D0,D1", "68000"),
    c("EORI.W #5,D0", "68000"),
    c("NOT.W D0", "68000"),
    c("TST.W D0", "68000"),
    c("TAS D0", "68000"),
    // Shifts and rotates (register and memory forms)
    c("ASL.W #1,D0", "68000"),
    c("ASR.W D1,D0", "68000"),
    c("LSL.W #1,D0", "68000"),
    c("LSR.L #8,D0", "68000"),
    c("ROL.W #1,D0", "68000"),
    c("ROR.W #1,D0", "68000"),
    c("ROXL.W #1,D0", "68000"),
    c("ROXR.W #1,D0", "68000"),
    c("ASL.W (A0)", "68000"),
    // Bit manipulation
    c("BTST #1,D0", "68000"),
    c("BTST D1,D0", "68000"),
    c("BSET #1,(A0)", "68000"),
    c("BCLR #1,D0", "68000"),
    c("BCHG D1,(A0)", "68000"),
    // Compare
    c("CMP.W D0,D1", "68000"),
    c("CMPA.W D0,A1", "68000"),
    c("CMPA.L A0,A1", "68000"),
    c("CMPI.W #5,D0", "68000"),
    c("CMPM.W (A0)+,(A1)+", "68000"),
    // Program control
    c("JMP (A0)", "68000"),
    c("JSR (A0)", "68000"),
    c("LEA (4,A0),A1", "68000"),
    c("PEA (A0)", "68000"),
    c("LINK A5,#-4", "68000"),
    c("UNLK A5", "68000"),
    c("SWAP D0", "68000"),
    c("EXG D0,D1", "68000"),
    c("EXG A0,A1", "68000"),
    c("EXG D0,A1", "68000"),
    c("CHK.W D0,D1", "68000"),
    // System control
    c("TRAP #1", "68000"),
    c("TRAPV", "68000"),
    c("RTS", "68000"),
    c("RTE", "68000"),
    c("RTR", "68000"),
    c("NOP", "68000"),
    c("RESET", "68000"),
    c("STOP #$2700", "68000"),
    c("ILLEGAL", "68000"),
];

/// Scc / DBcc / Bcc across every condition code — these share opcode
/// space and are easy to get wrong in the pattern table (Scc originally
/// matched only `ST` because its mask pinned the condition bits to zero).
const CONDITIONALS: &[Case] = &[
    c("ST D0", "68000"),
    c("SF D0", "68000"),
    c("SHI D0", "68000"),
    c("SLS D0", "68000"),
    c("SCC D0", "68000"),
    c("SCS D0", "68000"),
    c("SNE D0", "68000"),
    c("SEQ D0", "68000"),
    c("SVC D0", "68000"),
    c("SVS D0", "68000"),
    c("SPL D0", "68000"),
    c("SMI D0", "68000"),
    c("SGE D0", "68000"),
    c("SLT D0", "68000"),
    c("SGT D0", "68000"),
    c("SLE D0", "68000"),
    c("SEQ (A0)", "68000"),
    c("DBT D0,*", "68000"),
    c("DBF D0,*", "68000"),
    c("DBEQ D0,*", "68000"),
    c("DBNE D0,*", "68000"),
    c("DBMI D0,*", "68000"),
    c("DBLE D0,*", "68000"),
    c("BRA *", "68000"),
    c("BSR *", "68000"),
    c("BEQ *", "68000"),
    c("BNE *", "68000"),
    c("BCC *", "68000"),
    c("BCS *", "68000"),
    c("BVS *", "68000"),
    c("BLE *", "68000"),
];

/// 68010 and 68020 additions.
const EXT_68010_68020: &[Case] = &[
    // 68010
    c("MOVEC VBR,D0", "68010"),
    c("MOVEC D0,VBR", "68010"),
    c("MOVEC CACR,D0", "68010"),
    c("RTD #8", "68010"),
    c("BKPT #3", "68010"),
    c("MOVES.B D3,(A2)+", "68010"),
    c("MOVES.W A1,(A0)", "68010"),
    c("MOVES.L D0,(A0)", "68010"),
    c("MOVES.L (A0),D0", "68010"),
    c("MOVES.L (A0),A2", "68010"),
    // 68020 long multiply/divide, including the register-pair forms
    c("MULU.L D0,D1", "68020"),
    c("MULS.L D0,D1", "68020"),
    c("MULU.L D0,D2:D1", "68020"),
    c("MULS.L D0,D2:D1", "68020"),
    c("MULU.L (A0),D1", "68020"),
    c("DIVU.L D0,D1", "68020"),
    c("DIVS.L D0,D1", "68020"),
    c("DIVU.L D0,D2:D1", "68020"),
    c("DIVS.L D0,D2:D1", "68020"),
    c("DIVSL.L D0,D1:D2", "68020"),
    c("DIVUL.L D0,D1:D2", "68020"),
    // 68020 misc
    c("EXTB.L D0", "68020"),
    c("CAS.B D0,D1,(A2)", "68020"),
    c("CAS.W D0,D1,(A2)", "68020"),
    c("CAS.L D0,D1,(A2)", "68020"),
    c("CHK2.B (A0),D1", "68020"),
    c("CHK2.W (A0),D1", "68020"),
    c("CHK2.L (A0),A1", "68020"),
    c("CMP2.B (A0),D1", "68020"),
    c("CMP2.W (A0),D1", "68020"),
    c("CMP2.L (A0),A3", "68020"),
    c("PACK D0,D1,#0", "68020"),
    c("UNPK D0,D1,#0", "68020"),
    c("RTM D2", "68020"),
    c("RTM A3", "68020"),
    c("CALLM #4,(A0)", "68020"),
    c("CHK.L D0,D1", "68020"),
    c("LINK.L A5,#-4", "68020"),
    // TRAPcc, all three operand forms
    c("TRAPEQ", "68020"),
    c("TRAPNE", "68020"),
    c("TRAPF", "68020"),
    c("TRAPMI.W #5", "68020"),
    c("TRAPGT.L #7", "68020"),
    // Bitfield
    c("BFTST D0{0:8}", "68020"),
    c("BFEXTU (A0){0:8},D1", "68020"),
    c("BFEXTS (A0){0:8},D1", "68020"),
    c("BFCHG D0{0:8}", "68020"),
    c("BFCLR D0{0:8}", "68020"),
    c("BFSET D0{0:8}", "68020"),
    c("BFFFO (A0){0:8},D1", "68020"),
    c("BFINS D1,(A0){0:8}", "68020"),
];

/// 68030/68040/68060: MMU, cache control, MOVE16, LPSTOP.
const EXT_68030_PLUS: &[Case] = &[
    c("PSAVE (A0)", "68030"),
    c("PRESTORE (A0)", "68030"),
    c("MOVE16 (A0)+,(A1)+", "68040"),
    c("CINVA NC", "68040"),
    c("CINVA DC", "68040"),
    c("CINVA IC", "68040"),
    c("CINVA BC", "68040"),
    c("CPUSHA DC", "68040"),
    c("CPUSHA IC", "68040"),
    c("CINVL DC,(A0)", "68040"),
    c("CINVP IC,(A0)", "68040"),
    c("CPUSHL DC,(A0)", "68040"),
    c("CPUSHP IC,(A0)", "68040"),
    c("LPSTOP #$2700", "68060"),
];

/// FPU (68881/68882 and on-chip). Covers the arithmetic families, both
/// FMOVE directions, control-register moves, FMOVEM, and the branch/set/
/// trap variants.
const FPU: &[Case] = &[
    c("FADD.X FP0,FP1", "68040"),
    c("FSUB.X FP0,FP1", "68040"),
    c("FMUL.X FP0,FP1", "68040"),
    c("FDIV.X FP0,FP1", "68040"),
    c("FCMP.X FP0,FP1", "68040"),
    c("FABS.X FP0,FP1", "68040"),
    c("FNEG.X FP0,FP1", "68040"),
    c("FSQRT.X FP0", "68040"),
    c("FSIN.X FP0", "68040"),
    c("FCOS.X FP0", "68040"),
    c("FTAN.X FP0", "68040"),
    c("FETOX.X FP0", "68040"),
    c("FLOGN.X FP0", "68040"),
    c("FTST.X FP0", "68040"),
    c("FINT.X FP0", "68040"),
    c("FINTRZ.X FP0", "68040"),
    c("FGETEXP.X FP0", "68040"),
    c("FMOVE.X FP0,FP1", "68040"),
    c("FMOVE.L D0,FP0", "68040"),
    c("FMOVE.S D0,FP1", "68040"),
    c("FMOVE.X FP0,(A0)", "68040"),
    c("FMOVE.X FP0,-(A0)", "68040"),
    c("FMOVE.P FP0,(A0){#5}", "68040"),
    c("FMOVE.P FP0,(A0){#-5}", "68040"),
    c("FMOVE.P FP0,(A0){D3}", "68040"),
    c("FMOVECR #0,FP0", "68040"),
    c("FMOVE.L FPCR,D0", "68040"),
    c("FMOVE.L D0,FPCR", "68040"),
    c("FMOVEM.X FP0-FP3,-(A7)", "68040"),
    c("FMOVEM.X (A7)+,FP0-FP3", "68040"),
    c("FSMOVE.X FP0,FP1", "68040"),
    c("FDMOVE.X FP0,FP1", "68040"),
    c("FSADD.X FP0,FP1", "68040"),
    c("FDDIV.X FP0,FP1", "68040"),
    c("FNOP", "68040"),
    c("FSAVE (A0)", "68040"),
    c("FRESTORE (A0)", "68040"),
    c("FBEQ *", "68040"),
    c("FBNE *", "68040"),
    c("FBGT *", "68040"),
    c("FDBEQ D0,*", "68040"),
    c("FSEQ D0", "68040"),
    c("FSNE D0", "68040"),
];

/// Addressing modes exercised through a single instruction, so a broken
/// EA encoding shows up here rather than only in whichever instruction
/// happens to use it.
const ADDRESSING_MODES: &[Case] = &[
    c("MOVE.L D0,D1", "68000"),
    c("MOVE.L A0,D1", "68000"),
    c("MOVE.L (A0),D1", "68000"),
    c("MOVE.L (A0)+,D1", "68000"),
    c("MOVE.L -(A0),D1", "68000"),
    c("MOVE.L (4,A0),D1", "68000"),
    c("MOVE.L (4,A0,D1.W),D2", "68000"),
    c("MOVE.L (4,A0,D1.L),D2", "68000"),
    c("MOVE.L $1000,D1", "68000"),
    c("MOVE.L $12345678,D1", "68000"),
    // PC-relative: `(d,PC)` names a target *address* resolved against
    // the extension word, so at ORG $1000 the target must be near $1000.
    c("MOVE.L ($1004,PC),D1", "68000"),
    // `(d,PC,Xn)` names a target address too, against the same reference
    // point, but the brief format's displacement is only 8-bit — so the
    // target has to stay within -128..127 of the extension word.
    c("MOVE.L ($1004,PC,D1.W),D2", "68000"),
    c("MOVE.L #$12345678,D1", "68000"),
    // 68020+ scaled index and full-format
    c("MOVE.L (4,A0,D1.W*2),D2", "68020"),
    c("MOVE.L (4,A0,D1.L*4),D2", "68020"),
    c("MOVE.L (4,A0,D1.W*8),D2", "68020"),
];

fn roundtrip(case: &Case) -> Result<(), String> {
    // Branch targets are written as `*` (the instruction's own address).
    // The disassembler renders that as a generated `label0`, so the
    // reassembly step needs a definition for it — otherwise every branch
    // "fails" for a reason that has nothing to do with encoding. Defining
    // it at the same address the branch sits at keeps the displacement,
    // and therefore the bytes, identical.
    let source = format!("    ORG $1000\nlabel0:\n    {}\n", case.source);

    let mut asm = Assembler::new(0x1000);
    asm.set_cpu(case.cpu);
    let original = asm
        .assemble_bytes(&source)
        .map_err(|e| format!("assemble failed: {:?}", e))?;
    if original.is_empty() {
        return Err("assembled to zero bytes".to_string());
    }

    let mut disasm = m68k_disasm::disassembler::Disassembler::new(original.clone(), 0x1000);
    disasm.set_cpu(case.cpu);
    let lines = disasm.disassemble();
    let decoded = lines
        .first()
        .ok_or_else(|| "disassembler produced no output".to_string())?;

    // A `dc.w` here means the decoder has no pattern for an instruction
    // the assembler just produced — the exact gap this test exists for.
    if decoded.text.trim_start().starts_with("dc.w") {
        return Err(format!(
            "decoded as data word (no decoder pattern): bytes={} text={:?}",
            hex(&original),
            decoded.text.trim()
        ));
    }
    if decoded.is_error {
        return Err(format!("decode error: {}", decoded.text));
    }

    let reasm_source = format!("    ORG $1000\nlabel0:\n    {}\n", decoded.text.trim());
    let mut asm2 = Assembler::new(0x1000);
    asm2.set_cpu(case.cpu);
    let reencoded = asm2.assemble_bytes(&reasm_source).map_err(|e| {
        format!(
            "decoded to {:?} but that failed to reassemble: {:?}",
            decoded.text.trim(),
            e
        )
    })?;

    if reencoded != original {
        return Err(format!(
            "roundtrip changed bytes: {} -> {:?} -> {}",
            hex(&original),
            decoded.text.trim(),
            hex(&reencoded)
        ));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn run_group(name: &str, cases: &[Case]) {
    let mut failures = Vec::new();
    for case in cases {
        if let Err(err) = roundtrip(case) {
            failures.push(format!("  {:<28} {}", case.source, err));
        }
    }
    assert!(
        failures.is_empty(),
        "{}: {}/{} instruction forms failed to roundtrip:\n{}",
        name,
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn base_68000_instruction_set_roundtrips() {
    run_group("base 68000", BASE_68000);
}

#[test]
fn conditional_instructions_roundtrip() {
    run_group("conditionals (Scc/DBcc/Bcc)", CONDITIONALS);
}

#[test]
fn extended_68010_68020_instructions_roundtrip() {
    run_group("68010/68020 extensions", EXT_68010_68020);
}

#[test]
fn extended_68030_plus_instructions_roundtrip() {
    run_group("68030+/MMU/cache", EXT_68030_PLUS);
}

#[test]
fn fpu_instructions_roundtrip() {
    run_group("FPU", FPU);
}

#[test]
fn addressing_modes_roundtrip() {
    run_group("addressing modes", ADDRESSING_MODES);
}

/// Regression: `EAOperand::PcIndex` carried (displacement, target) while
/// the formatter — like the neighbouring `PcDisp` — reads the *first*
/// field as the target. `(d,PC,Xn)` therefore disassembled to its raw
/// displacement rendered as an address, which then reassembled to
/// something else entirely (or, past $7F, not at all).
///
/// `MOVE.L ($1004,PC,D1.W),D2` at $1000 encodes as 243B 1002: target
/// $1004 minus the extension word's address $1002. Disassembly must name
/// $1004 again, not $2.
#[test]
fn pc_relative_index_disassembles_target_not_displacement() {
    let bytes = vec![0x24, 0x3B, 0x10, 0x02];
    let mut disasm = m68k_disasm::disassembler::Disassembler::new(bytes, 0x1000);
    disasm.set_cpu("68000");
    let text = disasm
        .disassemble()
        .into_iter()
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("00001004"),
        "expected target $00001004 in disassembly, got: {}",
        text
    );
}
