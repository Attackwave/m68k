//! The program counter is 32 bits wide. Running past its end is an
//! error, not a wrap: before this check the assembler either panicked on
//! the addition (overflow checks on) or silently placed every following
//! label at a low address.

use m68k_asm::assembler::Assembler;

#[test]
fn running_past_the_end_of_the_address_space_is_an_error() {
    let mut asm = Assembler::new(0);
    let err = asm
        .assemble_bytes("        org $fffffff0\n        ds.w 10\n        nop\n")
        .expect_err("a program past $ffffffff must not assemble");
    assert!(
        err.message.contains("address space"),
        "unexpected error: {}",
        err.message
    );
    assert_eq!(err.line_no, Some(2));
}
