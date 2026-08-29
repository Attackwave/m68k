//! Motorola 68000 Language Server Protocol (LSP) implementation.
//!
//! Provides Language Server Protocol capabilities for Motorola 68000 (m68k)
//! assembly code:
//! - **Real-time Diagnostics**: Syntax validation and CPU-generation architecture gating.
//! - **Hover Documentation**: Opcode details, condition codes (CCR/SR effects), cycle timing, directives, registers, and Amiga LVO references.
//! - **Completion**: Context-aware suggestions with snippets for mnemonics, directives, registers, document symbols, and AmigaOS LVOs.
//! - **Definition**: Go-to-definition for labels, constants (`EQU`/`SET`), and macros (across current and included workspace files).
//! - **Document Symbols**: Outline of sections, labels, and equates.
//! - **Find References**: Project-wide reference lookup for labels and symbols.
//! - **Rename Refactoring**: Safe identifier renaming across the workspace.
//! - **Document Formatting**: Clean tabular column formatting.
//! - **Semantic Tokens**: Compiler-grade syntax highlighting.
//! - **Signature Help**: Operand parameter prompts.

pub mod analysis;
pub mod backend;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod document;
pub mod formatting;
pub mod hover;
pub mod references;
pub mod rename;
pub mod semantic_tokens;
pub mod signature;
pub mod symbols;
pub mod workspace;

pub use backend::Backend;
