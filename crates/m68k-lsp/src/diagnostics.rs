//! Diagnostics computation and error reporting for Motorola 68000 assembly files.

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

use m68k_asm::assembler::Assembler;
use m68k_core::cpu_gate::{MinCpu, min_cpu_for_mnemonic};
use m68k_core::errors::Severity;

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

    diagnostics.extend(crate::custom_regs::diagnostics(doc));

    // 2. Assemble in memory for everything the per-line pass cannot see:
    //    operands, branch distances, expressions, directives. Every error
    //    and warning the assembler collected is reported, not just the one
    //    it stopped on.
    let mut asm = Assembler::new(0);
    asm.set_cpu(config.cpu.name());
    // Relative INCLUDEs resolve against the file's own directory, as they
    // do when the CLI assembles it.
    if let Ok(path) = doc.uri.to_file_path()
        && let Some(dir) = path.parent()
    {
        asm.set_source_root(dir.to_path_buf());
    }
    let _ = asm.assemble(&doc.text);

    for diag in asm.diagnostics() {
        // The top-level source has no file name here (none was given to
        // the assembler); anything that names a file is in an included
        // file, which has no range in this document. Such a diagnostic is
        // shown at the top with its own location in the message.
        let (line_idx, message) = if diag.filename.is_empty() {
            (diag.line_no.unwrap_or(1).saturating_sub(1), diag.message)
        } else {
            let line = diag.line_no.map(|l| format!(":{l}")).unwrap_or_default();
            (0, format!("{}{}: {}", diag.filename, line, diag.message))
        };
        let line_text = doc.lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");
        let char_start = line_text.len().saturating_sub(line_text.trim_start().len()) as u32;
        let char_end = line_text.len() as u32;
        let (severity, code) = match diag.severity {
            Severity::Error => (DiagnosticSeverity::ERROR, "ASM_ERR"),
            Severity::Warning => (DiagnosticSeverity::WARNING, "ASM_WARN"),
        };
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
            severity: Some(severity),
            code: Some(tower_lsp::lsp_types::NumberOrString::String(
                code.to_string(),
            )),
            source: Some("m68k-lsp".to_string()),
            message,
            ..Default::default()
        });
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

    #[test]
    fn every_error_line_is_reported_not_just_the_first() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src =
            "    move.l  d0,nowhere\n    nop\n    bra.s   far\n    dcb.b   300,0\nfar:\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());
        let diags = compute_diagnostics(&doc, &LinterConfig::default());
        let error_lines: Vec<u32> = diags
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .map(|d| d.range.start.line)
            .collect();
        assert_eq!(error_lines, vec![0, 2], "{diags:?}");
    }

    #[test]
    fn assembler_warnings_are_reported_as_warnings() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "    moveq   #200,d0\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());
        let diags = compute_diagnostics(&doc, &LinterConfig::default());
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::WARNING)
                    && d.range.start.line == 0
                    && d.message.contains("MOVEQ")),
            "{diags:?}"
        );
    }
}
