//! Diagnostics computation and error reporting for Motorola 68000 assembly files.

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

use m68k_asm::assembler::Assembler;
use m68k_core::cpu_gate::{MinCpu, min_cpu_for_mnemonic};

use crate::document::Document;

/// Target architecture configuration for linting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinterConfig {
    pub cpu: MinCpu,
}

impl Default for LinterConfig {
    fn default() -> Self {
        Self {
            cpu: MinCpu::Mc68000,
        }
    }
}

/// Compute diagnostics for a document.
pub fn compute_diagnostics(doc: &Document, config: &LinterConfig) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // 1. Fast per-line syntactic & CPU-gating linting
    for (line_idx, parsed) in doc.parsed_lines.iter().enumerate() {
        let raw = &parsed.raw;
        let line_num = line_idx as u32;

        if parsed.mnemonic.is_empty() {
            continue;
        }

        // Check CPU-level requirement for mnemonic
        if let Some(req_cpu) = min_cpu_for_mnemonic(&parsed.mnemonic) {
            let req_level = match req_cpu {
                MinCpu::Mc68000 => 0,
                MinCpu::Mc68010 => 1,
                MinCpu::Mc68020 => 2,
                MinCpu::Mc68030 => 3,
                MinCpu::Mc68040 => 4,
                MinCpu::Mc68060 => 5,
            };
            let current_level = match config.cpu {
                MinCpu::Mc68000 => 0,
                MinCpu::Mc68010 => 1,
                MinCpu::Mc68020 => 2,
                MinCpu::Mc68030 => 3,
                MinCpu::Mc68040 => 4,
                MinCpu::Mc68060 => 5,
            };

            if req_level > current_level {
                let col_start = raw.to_ascii_lowercase().find(&parsed.mnemonic).unwrap_or(0) as u32;
                let col_end = col_start + parsed.mnemonic.len() as u32;

                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position {
                            line: line_num,
                            character: col_start,
                        },
                        end: Position {
                            line: line_num,
                            character: col_end,
                        },
                    },
                    severity: Some(DiagnosticSeverity::WARNING),
                    code: Some(tower_lsp::lsp_types::NumberOrString::String(
                        "CPU_LEVEL".to_string(),
                    )),
                    source: Some("m68k-lsp".to_string()),
                    message: format!(
                        "Instruction '{}' requires {} or later (target configured for {})",
                        parsed.mnemonic.to_ascii_uppercase(),
                        req_cpu.name(),
                        config.cpu.name()
                    ),
                    ..Default::default()
                });
            }
        }
    }

    // 2. Run assembler in-memory pass to catch structural, branch, and expression errors
    let mut asm = Assembler::new(0);
    asm.set_cpu(config.cpu.name());

    match asm.assemble(&doc.text) {
        Ok(_) => {}
        Err(e) => {
            // AsmError provides line_no (1-based)
            let line_no = e.line_no.unwrap_or(1);
            let line_idx = if line_no > 0 { line_no - 1 } else { 0 };
            let line_text = doc.lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");

            let char_start = line_text.len().saturating_sub(line_text.trim_start().len()) as u32;
            let char_end = line_text.len() as u32;

            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position {
                        line: line_idx as u32,
                        character: char_start,
                    },
                    end: Position {
                        line: line_idx as u32,
                        character: char_end.max(char_start + 1),
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(tower_lsp::lsp_types::NumberOrString::String(
                    "ASM_ERR".to_string(),
                )),
                source: Some("m68k-lsp".to_string()),
                message: e.message,
                ..Default::default()
            });
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_valid_source_has_no_error_diagnostics() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "START:\n    move.w  #1,d0\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());
        let diags = compute_diagnostics(&doc, &LinterConfig::default());

        assert!(
            diags.is_empty(),
            "Expected no diagnostics, got: {:?}",
            diags
        );
    }

    #[test]
    fn test_cpu_gate_warning() {
        let uri = Url::parse("file:///test.s").unwrap();
        // extb requires 68020+
        let src = "    extb.l  d0\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());
        let diags = compute_diagnostics(
            &doc,
            &LinterConfig {
                cpu: MinCpu::Mc68000,
            },
        );

        assert!(!diags.is_empty());
        assert!(diags.iter().any(|d| d.message.contains("requires 68020")));
    }
}
