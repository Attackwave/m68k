//! Hover information provider for Motorola 68000 opcodes, directives, registers, and LVOs.

use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use m68k_core::amiga_lvo::Library;

use crate::analysis::cycles::estimate_cycles;
use crate::analysis::directives_docs::lookup_directive;
use crate::analysis::op_docs::{FlagEffect, OpcodeDoc, lookup_opcode};
use crate::document::{Document, WordKind};

/// Format hover information for the token at the given cursor position.
pub fn compute_hover(doc: &Document, pos: Position) -> Option<Hover> {
    let word_info = doc.get_word_at_position(pos)?;
    let clean_word = word_info.word.trim();
    if clean_word.is_empty() {
        return None;
    }

    let markdown = match word_info.kind {
        WordKind::Mnemonic => format_opcode_hover(
            clean_word.split('.').next().unwrap_or(clean_word),
            if word_info.size.is_empty() {
                None
            } else {
                Some(word_info.size.as_str())
            },
            &word_info.operands,
        )?,
        WordKind::Directive => {
            let dir_name = clean_word.split('.').next().unwrap_or(clean_word);
            format_directive_hover(dir_name)?
        }
        WordKind::Register => format_register_hover(clean_word)?,
        WordKind::Lvo => format_lvo_hover(clean_word)?,
        WordKind::Label => format_symbol_hover(doc, clean_word)?,
        WordKind::Unknown => {
            // Try opcode first, then directive, then symbol
            if let Some(h) = format_opcode_hover(
                clean_word.split('.').next().unwrap_or(clean_word),
                None,
                &[],
            ) {
                h
            } else if let Some(h) = format_directive_hover(clean_word) {
                h
            } else {
                format_symbol_hover(doc, clean_word)?
            }
        }
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: Some(word_info.range),
    })
}

fn format_opcode_hover(mnemonic: &str, size: Option<&str>, operands: &[String]) -> Option<String> {
    let doc: OpcodeDoc = lookup_opcode(mnemonic)?;

    let mut md = String::new();
    md.push_str(&format!(
        "### **{}** — {}\n\n",
        doc.mnemonic.to_ascii_uppercase(),
        doc.title
    ));
    md.push_str(&format!("{}\n\n", doc.summary));

    md.push_str("**Syntax:**\n```assembly\n");
    for syn in doc.syntax {
        md.push_str(&format!("{}\n", syn));
    }
    md.push_str("```\n\n");

    md.push_str(&format!("* **Sizes:** `{}`\n", doc.valid_sizes.join(", ")));
    md.push_str(&format!(
        "* **Architecture:** `{} or later`\n",
        doc.min_cpu.name()
    ));

    // Estimate cycles
    if let Some(cycle_est) = estimate_cycles(mnemonic, size, operands) {
        md.push_str(&format!("* **Timing (68000):** {}\n", cycle_est.display));
    } else {
        md.push_str(&format!("* **Timing (68000):** {}\n", doc.base_cycles));
    }

    // CCR table
    md.push_str("\n**Condition Codes (CCR):**\n\n");
    md.push_str("| X | N | Z | V | C |\n");
    md.push_str("|:---:|:---:|:---:|:---:|:---:|\n");
    md.push_str(&format!(
        "| {} | {} | {} | {} | {} |\n\n",
        doc.ccr.x.symbol(),
        doc.ccr.n.symbol(),
        doc.ccr.z.symbol(),
        doc.ccr.v.symbol(),
        doc.ccr.c.symbol()
    ));

    let flag_notes: Vec<String> = [
        ("X", doc.ccr.x),
        ("N", doc.ccr.n),
        ("Z", doc.ccr.z),
        ("V", doc.ccr.v),
        ("C", doc.ccr.c),
    ]
    .iter()
    .filter(|(_, eff)| *eff != FlagEffect::Unchanged)
    .map(|(name, eff)| format!("* `{}`: {}", name, eff.description(name)))
    .collect();

    if !flag_notes.is_empty() {
        md.push_str(&flag_notes.join("\n"));
        md.push('\n');
    }

    Some(md)
}

fn format_directive_hover(name: &str) -> Option<String> {
    let doc = lookup_directive(name)?;

    let mut md = String::new();
    md.push_str(&format!(
        "### Directive: **{}** — {}\n\n",
        doc.name.to_ascii_uppercase(),
        doc.title
    ));
    md.push_str(&format!("{}\n\n", doc.summary));

    md.push_str("**Syntax:**\n```assembly\n");
    for syn in doc.syntax {
        md.push_str(&format!("{}\n", syn));
    }
    md.push_str("```\n\n");

    md.push_str("**Example:**\n```assembly\n");
    md.push_str(doc.example);
    md.push_str("\n```\n");

    Some(md)
}

fn format_register_hover(reg: &str) -> Option<String> {
    let r = reg.to_ascii_lowercase();
    let desc = match r.as_str() {
        "d0" | "d1" | "d2" | "d3" | "d4" | "d5" | "d6" | "d7" => {
            let num = &r[1..];
            format!("**Data Register D{}** (32-bit)\n\nGeneral-purpose register for data, arithmetic, and logic operations.", num)
        }
        "a0" | "a1" | "a2" | "a3" | "a4" | "a5" | "a6" => {
            let num = &r[1..];
            format!("**Address Register A{}** (32-bit)\n\nPointer/base register for memory addressing and indexing.", num)
        }
        "a7" | "sp" => {
            "**Stack Pointer (SP / A7)** (32-bit)\n\nPoints to the active stack frame. Hardware uses USP (User Stack Pointer) in user mode or SSP (Supervisor Stack Pointer) in supervisor mode.".to_string()
        }
        "pc" => {
            "**Program Counter (PC)** (32-bit)\n\nHolds the memory address of the next instruction to be fetched and executed.".to_string()
        }
        "sr" => {
            "**Status Register (SR)** (16-bit, Privileged)\n\nHigh byte contains system status (Trace mode, Supervisor flag, Interrupt Priority Mask `I0-I2`). Low byte contains Condition Code Register (CCR).".to_string()
        }
        "ccr" => {
            "**Condition Code Register (CCR)** (8-bit)\n\nStatus flags: `X` (Extend), `N` (Negative), `Z` (Zero), `V` (Overflow), `C` (Carry).".to_string()
        }
        "vbr" => {
            "**Vector Base Register (VBR)** (32-bit, 68010+)\n\nPrivileged register holding the base address of the exception vector table (defaults to `$00000000`).".to_string()
        }
        "cacr" => {
            "**Cache Control Register (CACR)** (32-bit, 68020+)\n\nPrivileged register for enabling, clearing, and freezing instruction/data caches.".to_string()
        }
        "caar" => {
            "**Cache Address Register (CAAR)** (32-bit, 68020/68030)\n\nPrivileged register for cache clear/invalidate operations.".to_string()
        }
        "usp" => {
            "**User Stack Pointer (USP)** (32-bit, Privileged access via MOVEC/MOVE USP).\n\nUser stack base pointer.".to_string()
        }
        "ssp" | "isp" => {
            "**Supervisor / Interrupt Stack Pointer (SSP / ISP)** (32-bit).\n\nActive stack pointer during supervisor mode and interrupt exceptions.".to_string()
        }
        "msp" => {
            "**Master Stack Pointer (MSP)** (32-bit, 68020+).\n\nMaster supervisor stack pointer when master/interrupt split is enabled.".to_string()
        }
        "sfc" | "dfc" => {
            "**Source / Destination Function Code (SFC / DFC)** (3-bit, 68010+).\n\nDefines memory address space for `MOVES` instructions.".to_string()
        }
        "fp0" | "fp1" | "fp2" | "fp3" | "fp4" | "fp5" | "fp6" | "fp7" => {
            let num = &r[2..];
            format!("**Floating-Point Register FP{}** (80-bit Extended Precision)\n\nFPU data register for 68881/68882/68040.", num)
        }
        "fpcr" => "**Floating-Point Control Register (FPCR)**\n\nFPU rounding mode and exception trap enables.".to_string(),
        "fpsr" => "**Floating-Point Status Register (FPSR)**\n\nFPU condition codes, quotient, and accrued exception status.".to_string(),
        _ => return None,
    };

    Some(format!(
        "### Register: `{}`\n\n{}",
        reg.to_ascii_uppercase(),
        desc
    ))
}

fn format_lvo_hover(lvo_name: &str) -> Option<String> {
    let clean = lvo_name.trim_start_matches('_');
    let name_part = clean.strip_prefix("LVO").unwrap_or(clean);

    // Search known Amiga libraries
    for lib in [
        Library::Exec,
        Library::Dos,
        Library::Graphics,
        Library::Intuition,
    ] {
        for (offset, name) in lib.entries() {
            if name.eq_ignore_ascii_case(name_part) {
                return Some(format!(
                    "### AmigaOS LVO: `_LVO{}`\n\n* **Library:** `{}.library`\n* **Offset:** `-0x{:03X}` (`-{}`)\n* **Calling Convention:** `jsr -0x{:03X}(a6)`\n",
                    name,
                    lib.file_name().trim_end_matches(".library"),
                    offset,
                    offset,
                    offset
                ));
            }
        }
    }

    Some(format!(
        "### AmigaOS Library Vector Offset: `{}`\n\nAmiga system library call offset.",
        lvo_name
    ))
}

fn format_symbol_hover(doc: &Document, name: &str) -> Option<String> {
    if let Some(sym) = doc.symbols.get(name) {
        let kind_str = match sym.kind {
            crate::document::SymbolDefKind::Label => "Label",
            crate::document::SymbolDefKind::Equate => "Constant (EQU)",
            crate::document::SymbolDefKind::Macro => "Macro",
            crate::document::SymbolDefKind::Section => "Section",
        };

        let mut md = format!(
            "### {}: `{}`\n\n* **Defined at line:** {}\n",
            kind_str,
            sym.name,
            sym.line_idx + 1
        );
        if let Some(ref val) = sym.value_str {
            md.push_str(&format!("* **Value:** `{}`\n", val));
        }
        return Some(md);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_hover_on_move() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "    move.w  #1,d0\n";
        let doc = Document::new(uri, 1, src.to_string());

        let hover = compute_hover(
            &doc,
            Position {
                line: 0,
                character: 6,
            },
        )
        .expect("hover should exist");
        if let HoverContents::Markup(m) = hover.contents {
            assert!(m.value.contains("MOVE"));
            assert!(m.value.contains("Condition Codes"));
        } else {
            panic!("Expected markup content");
        }
    }

    #[test]
    fn test_hover_on_register() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "    move.l  d0,a0\n";
        let doc = Document::new(uri, 1, src.to_string());

        let hover = compute_hover(
            &doc,
            Position {
                line: 0,
                character: 15,
            },
        )
        .expect("hover on a0");
        if let HoverContents::Markup(m) = hover.contents {
            assert!(m.value.contains("Address Register A0"));
        } else {
            panic!("Expected markup content");
        }
    }
}
