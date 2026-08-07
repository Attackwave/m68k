//! Encodings pinned to bytes measured from the reference assembler.
//!
//! Every case here was produced by running `vasmm68k_mot -Fbin -no-opt` on
//! the source shown and recording what came out. They are written as exact
//! byte strings rather than as roundtrips on purpose: each one is a defect
//! that a roundtrip could not have caught, because a roundtrip only proves
//! the encoder and decoder agree with *each other*.
//!
//! The forms below all appeared in ordinary third-party Amiga sources, and
//! each was rejected outright or silently mis-assembled before.

use m68k_asm::assembler::Assembler;

const ORIGIN: u32 = 0x1000;

/// Assemble `source` at [`ORIGIN`] and return it as a lowercase hex string.
fn asm(source: &str) -> String {
    let mut a = Assembler::new(ORIGIN);
    a.set_cpu("68000");
    let bytes = a
        .assemble_bytes(source)
        .unwrap_or_else(|e| panic!("failed to assemble {:?}: {:?}", source, e));
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Assert that a single instruction encodes to exactly `want`.
fn check(instruction: &str, want: &str) {
    let got = asm(&format!("\t{}\n", instruction));
    assert_eq!(got, want, "{}", instruction);
}

/// An address register as the destination of ADD/SUB/CMP selects the
/// ADDA/SUBA/CMPA encoding. `ADD.L #2,A1` is how real sources spell it; the
/// A-form encoders existed but were reachable only through the explicit
/// `ADDA` mnemonic, so these were rejected as "one operand must be a data
/// register".
#[test]
fn address_register_destination_selects_the_a_form() {
    check("add.l #2,a1", "d3fc00000002");
    check("add.w #2,a1", "d2fc0002");
    check("add.l d0,a1", "d3c0");
    check("add.l a2,a1", "d3ca");
    check("sub.l #2,a1", "93fc00000002");
    check("cmp.l #2,a1", "b3fc00000002");
    check("cmp.w a2,a1", "b2ca");

    // The data-register forms must keep their own (different) encodings.
    check("add.l #2,d1", "d2bc00000002");
    check("add.b d0,d1", "d200");
}

/// A MOVEM range is over the flat numbering D0..D7,A0..A7 and may cross the
/// bank boundary: `D0-A6` means "everything but A7". Requiring both ends in
/// the same bank rejected the single most common spelling of a full
/// register save.
#[test]
fn movem_range_may_cross_the_register_bank_boundary() {
    check("movem.l d0-a6,-(sp)", "48e7fffe");
    check("movem.l (sp)+,d0-a6", "4cdf7fff");
    check("movem.w d3-a2,-(sp)", "48a71fe0");

    // Ranges within one bank, and the predecrement mask reversal, still hold.
    check("movem.l a0-a3,-(sp)", "48e700f0");
}

/// A one-register MOVEM is a register list of length one.
///
/// `MOVEM.L A1,-(SP)` never reaches the list parser: with no `/` or `-` in
/// it, `a1` is recognised as an ordinary register operand first, and the
/// encoder then refused it for not being a mask. Saving a single register
/// around a routine is entirely ordinary.
#[test]
fn movem_accepts_a_single_register() {
    check("movem.l a1,-(sp)", "48e70040");
    check("movem.l (sp)+,a1", "4cdf0200");
    check("movem.w d0,-(sp)", "48a78000");
    check("movem.l d7,(a0)", "48d00080");
    check("movem.l (a0),d7", "4cd00080");
}

/// PC-relative modes are not alterable, so they cannot be a destination —
/// on any 68k, not just pre-68020, since there is nothing to write back to.
///
/// MOVE was the only instruction permissive here: it passed `ALL` for its
/// destination, so `MOVE.W D0,LAB(PC)` silently encoded. `CLR` and `ADD`
/// already rejected the same shape.
#[test]
fn pc_relative_is_rejected_as_a_move_destination() {
    let rejected = |src: &str| {
        let mut a = Assembler::new(ORIGIN);
        a.set_cpu("68000");
        assert!(
            a.assemble_bytes(&format!("lab:\tdc.w 0\n\t{}\n", src))
                .is_err(),
            "{} should be rejected: PC-relative is not alterable",
            src
        );
    };

    rejected("move.w d0,lab(pc)");
    rejected("move.b d0,2(pc,d1.w)");
    rejected("move.w d0,#5");

    // The load direction, and every ordinary destination, still work.
    check("lab:\tmove.w lab(pc),d0", "303afffe");
    check("move.w d0,a1", "3240");
    check("move.w d0,(a0)", "3080");
    check("move.w d0,$1000", "33c000001000");
}

/// `HS`/`LO` are the unsigned-comparison spellings of `CC`/`CS` and encode
/// identically. The Shrinkler decompressor uses `BLO` throughout, so their
/// absence rejected whole files.
#[test]
fn hs_and_lo_are_accepted_as_cc_and_cs() {
    check("lab:\tblo.b lab", "65fe");
    check("lab:\tbhs.b lab", "64fe");
    check("lab:\tblo.w lab", "6500fffe");
    check("lab:\tdblo d0,lab", "55c8fffe");
    check("slo d0", "55c0");
    check("shs d0", "54c0");

    // Same encodings via the CC/CS spelling.
    check("lab:\tbcs.b lab", "65fe");
    check("lab:\tbcc.b lab", "64fe");
}

/// ALIGN's argument is a *bit count*, and padding is zero bytes.
///
/// The previous reading (byte count, NOP fill) is CNOP's, and got both the
/// length and the bytes wrong: `ALIGN 4` aligns to 16, not to 4.
#[test]
fn align_takes_a_bit_count_and_pads_with_zeroes() {
    // ORG is stated explicitly: how much padding ALIGN emits depends on
    // the current address, so a test that relied on the default origin
    // would be asserting something other than what it reads.
    let from_origin = |directive: &str| {
        asm(&format!(
            "\torg $1000\n\tdc.b 1\n\t{}\n\tdc.b $ee\n",
            directive
        ))
    };

    // One byte in from a 16-byte boundary, ALIGN 2 reaches the next 4-byte
    // one: three bytes of padding.
    assert_eq!(from_origin("align 2"), "01000000ee");
    // ALIGN 4 reaches a 16-byte boundary — the spelling most sources use.
    assert_eq!(from_origin("align 4"), "01000000000000000000000000000000ee");
    // An odd bit count is legal, and is not a "power of two" question.
    assert_eq!(from_origin("align 3"), "0100000000000000ee");
    // ALIGN 0 aligns to 2^0 = 1 and so never pads; the second operand is
    // accepted and ignored, which is what `align 0,4` in the Shrinkler
    // headers relies on.
    assert_eq!(from_origin("align 0,4"), "01ee");
    // A fill-looking second operand really is ignored — padding stays zero.
    assert_eq!(from_origin("align 2,$ff"), "01000000ee");
}

/// A label in column 1 may be followed by an instruction on the same line
/// without a colon — `.loop  move.l ...`, the ordinary Amiga loop idiom.
/// This was read as a mnemonic called `.loop`, so the instruction after it
/// became an unparsable operand.
#[test]
fn column_one_label_may_precede_an_instruction_without_a_colon() {
    assert_eq!(
        asm("waitvbl:\n\tmoveq #1,d0\n.loop\tmove.l\t$dff004,d0\n\tbne.b\t.loop\n\trts\n"),
        "7001203900dff00466f84e75"
    );
}

/// `NAME = expr` is the assignment spelling of EQU, and the one the Amiga
/// system headers use (`ExecBase = 4`).
#[test]
fn equals_is_accepted_as_equ() {
    assert_eq!(asm("ExecBase = 4\n\tmove.l ExecBase,a6\n"), "2c7900000004");
}

/// A directive name is still usable as a label — `END` most of all.
///
/// `BEQ END` was split as "label BEQ, directive END", which *ended the
/// assembly* at a forward branch: no error, just a silently truncated
/// program. This is the worst failure mode in this file, and the reason
/// these tests assert bytes rather than success.
#[test]
fn a_directive_name_is_still_usable_as_a_label() {
    let source = "\tmoveq #1,d0\n\tbeq\tend\n\tnop\nend:\n\trts\n";
    assert_eq!(asm(source), "7001670000044e714e75");

    // The whole program must survive, not just the branch: four
    // instructions, ten bytes. A truncated assembly would still "pass" a
    // test that only checked the branch encoding.
    assert_eq!(asm(source).len() / 2, 10);
}
