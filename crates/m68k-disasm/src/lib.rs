//! Motorola 68000 instruction decoder.
//!
//! The core decoding primitive is [`decoder::decode_next`]: given an
//! [`m68k_core::addressing::InstructionStream`] positioned at an
//! instruction boundary and a CPU level string (e.g. `"68000"`,
//! `"68020"`), it decodes one instruction (or a data word, if the bytes
//! don't match any known opcode pattern) and returns a
//! [`decoder::DecodedInstruction`] with its mnemonic, operands, and any
//! branch/jump target address.
//!
//! ```
//! use m68k_core::addressing::InstructionStream;
//! use m68k_disasm::decoder::{DecodeResult, decode_next};
//!
//! let bytes = [0x4E, 0x71, 0x4E, 0x75]; // NOP; RTS
//! let mut stream = InstructionStream::new(&bytes, 0x1000);
//! let (_pc, result) = decode_next(&mut stream, "68000").unwrap();
//! if let DecodeResult::Instruction(inst) = result {
//!     assert_eq!(inst.mnemonic, "nop");
//! }
//! ```
//!
//! The two-pass driver (pass 1: scan and discover branch/jump targets to
//! auto-generate label names; pass 2: format final output substituting
//! discovered labels for addresses) is [`disassembler::Disassembler`],
//! mirroring `m68k-asm`'s `Assembler` as a reusable library API. The
//! `m68k-disasm` CLI binary (`m68k-cli/src/bin/m68k-disasm.rs`) is a thin
//! wrapper over it.

pub mod decoder;
pub mod disassembler;
