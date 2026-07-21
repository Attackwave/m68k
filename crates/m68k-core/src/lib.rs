//! Shared primitives used by both the m68k assembler and disassembler:
//! tokenizing, expression ASTs, effective-address category constraints,
//! the opcode table, float encoding, and error types.
//!
//! This crate has no dependency on `m68k-asm` or `m68k-disasm` — it exists
//! so both can share the parsing/encoding building blocks that don't differ
//! between assembling and disassembling (e.g. the opcode pattern table in
//! [`opcodes`], or sign-extension helpers in [`utils`]).
//!
//! Most callers will use this crate indirectly through `m68k-asm` or
//! `m68k-disasm` rather than directly.

pub mod addressing;
pub mod amiga_hunk;
pub mod ea_categories;
pub mod errors;
pub mod expr;
pub mod floats;
pub mod opcodes;
pub mod operands;
pub mod tokens;
pub mod utils;
