//! Two-pass Motorola 68000 assembler.
//!
//! The entry point is [`assembler::Assembler`]: construct one with an origin
//! address, call [`assembler::Assembler::assemble`] (or
//! [`assembler::Assembler::assemble_bytes`] for a flattened byte vector)
//! with the source text, then read back `asm.code` (per-instruction PC +
//! encoded words), `asm.symbols` (the resolved symbol table), and
//! `asm.errors` (warnings/errors with source line numbers).
//!
//! ```
//! use m68k_asm::assembler::Assembler;
//! use m68k_asm::output::generate_srecord;
//!
//! let mut asm = Assembler::new(0x1000);
//! asm.assemble("
//!     ORG $1000
//! START:
//!     MOVEQ #1,D0
//!     RTS
//! ").unwrap();
//!
//! assert_eq!(asm.symbols.resolve("START").unwrap(), 0x1000);
//!
//! // Any of several output formats can be generated from the same
//! // assembled instructions: raw binary (`output::generate_binary`),
//! // Motorola S-Record, Intel Hex, ELF32 (`output::generate_elf`), or
//! // IEEE-695 (`ieee695::generate_ieee695`).
//! let srec = generate_srecord(&asm.code, "example");
//! assert!(srec.starts_with("S0"));
//! ```
//!
//! # Module map
//!
//! - [`assembler`] — the two-pass driver (`Assembler`, `SymbolTable`,
//!   `AssembledInstruction`) and directive dispatch.
//! - [`directives`] — EQU/SET/ORG/SECTION/INCLUDE/etc. and the expression
//!   parser used by directives (`parse_simple_expr`, including `DEFINED()`,
//!   `HIGH()`/`LOW()`, and `*`/`$` as the current location counter).
//! - `enc_*` modules (`enc_math`, `enc_logic`, `enc_move`, `enc_flow`,
//!   `enc_fpu`, `enc_mmu`, `enc_bitfield`) — per-instruction-family
//!   encoders, dispatched through [`encoder`].
//! - [`ea_encode`] — effective-address operand → (mode, register,
//!   extension words) encoding.
//! - [`output`] — Binary, S-Record, Intel Hex, and ELF32 output generation.
//! - [`ieee695`] — IEEE-695 object output (verified against GNU binutils BFD
//!   reader; see that module's docs for details).

pub mod assembler;
pub mod directives;
pub mod ea_encode;
pub mod enc_bitfield;
pub mod enc_flow;
pub mod enc_fpu;
pub mod enc_logic;
pub mod enc_math;
pub mod enc_mmu;
pub mod enc_move;
pub mod encoder;
pub mod ieee695;
pub mod output;
