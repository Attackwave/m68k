//! Motorola 68000 Language Server Protocol (LSP) implementation.
//!
//! Provides Language Server Protocol capabilities for Motorola 68000 (m68k)
//! assembly code:
//! - **Real-time Diagnostics**: Syntax validation and CPU-generation architecture gating.
//! - **Hover Documentation**: Opcode details, condition codes (CCR/SR effects), cycle timing, directives, registers, and Amiga LVO references.
//! - **Completion**: Context-aware suggestions for mnemonics, size extensions, directives, registers, document symbols, and AmigaOS LVOs.
//! - **Definition**: Go-to-definition for labels, constants (`EQU`/`SET`), and macros.
//! - **Document Symbols**: Outline of sections, labels, and equates.

pub mod analysis;
pub mod backend;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod document;
pub mod hover;
pub mod symbols;

pub use backend::Backend;
