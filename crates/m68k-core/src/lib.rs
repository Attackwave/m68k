//! Shared primitives used by both the m68k assembler and disassembler:
//! tokenizing, effective-address category constraints, the opcode table,
//! and error types.
//!
//! This crate has no dependency on `m68k-asm` or `m68k-disasm` — it exists
//! so both can share the parsing/encoding building blocks that don't differ
//! between assembling and disassembling (e.g. the opcode pattern table in
//! [`opcodes`], or sign-extension helpers in [`utils`]).
//!
//! Most callers will use this crate indirectly through `m68k-asm` or
//! `m68k-disasm` rather than directly.
//!
//! Expression parsing/evaluation is *not* here: the canonical (and only)
//! evaluator is `m68k_asm::directives::parse_simple_expr` — a combined
//! tokenizer + recursive-descent parser wired directly to `SymbolTable`
//! (hex/binary/decimal literals, `$`/`*` as PC, `HIGH()`/`LOW()`/
//! `DEFINED()`, full operator precedence). This crate previously also had
//! a standalone `expr` module (a separate AST + evaluator, no parser, no
//! symbol table integration) that nothing outside its own tests ever
//! called — removed rather than kept in sync by hand a second time, which
//! is exactly the kind of drift that caused bug 1.5 (a shift/negate-panic
//! fix applied to `expr.rs` that `directives.rs`'s evaluator didn't
//! automatically get, and vice versa).

pub mod addressing;
pub mod amiga_hunk;
pub mod amiga_lvo;
pub mod cpu_gate;
pub mod ea_categories;
pub mod errors;
pub mod opcodes;
pub mod operands;
pub mod tokens;
pub mod utils;
