//! Source code formatting provider for Motorola 68000 assembly files.

use tower_lsp::lsp_types::{FormattingOptions, Position, Range, TextEdit};

use crate::analysis::directives_docs::lookup_directive;
use crate::document::Document;
use m68k_core::tokens::{is_mnemonic, split_line};

/// Configuration options for m68k assembly formatting.
#[derive(Debug, Clone)]
pub struct AsmFormatConfig {
    /// Number of spaces for instruction/directive indentation if no label.
    pub indent_spaces: usize,
    /// Column position where the mnemonic should start (if preceded by a short label).
    pub mnemonic_col: usize,
    /// Column position where operands should start.
    pub operand_col: usize,
    /// Column position where end-of-line comments should be aligned.
    pub comment_col: usize,
    /// Space after comma in operand lists.
    pub space_after_comma: bool,
}

impl Default for AsmFormatConfig {
    fn default() -> Self {
        Self {
            indent_spaces: 4,
            mnemonic_col: 12,
            operand_col: 20,
            comment_col: 40,
            space_after_comma: true,
        }
    }
}

/// Compute document formatting edits.
pub fn compute_formatting(doc: &Document, options: &FormattingOptions) -> Vec<TextEdit> {
    let config = AsmFormatConfig {
        indent_spaces: options.tab_size as usize,
        ..Default::default()
    };

    let formatted_text = format_source(&doc.text, &config);

    if formatted_text == doc.text {
        return Vec::new();
    }

    let end_line = doc.lines.len().saturating_sub(1) as u32;
    let end_char = doc.lines.last().map(|l| l.len()).unwrap_or(0) as u32;

    vec![TextEdit {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: end_line,
                character: end_char,
            },
        },
        new_text: formatted_text,
    }]
}

/// Format a complete assembly source string.
pub fn format_source(source: &str, config: &AsmFormatConfig) -> String {
    let mut output = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();

        // Preserve empty lines
        if trimmed.is_empty() {
            output.push(String::new());
            continue;
        }

        // Preserve comment-only lines
        if trimmed.starts_with(';') || trimmed.starts_with('*') {
            output.push(line.trim_end().to_string());
            continue;
        }

        // Extract comment from end of line if present
        let (code_part, comment_part) = extract_line_comment(line);

        let (mut label, mut mnemonic, size, operands) = split_line(code_part);

        if mnemonic.is_empty()
            && let Some(ref lbl) = label
        {
            let lbl_lower = lbl.to_ascii_lowercase();
            if is_mnemonic(&lbl_lower)
                || is_mnemonic(lbl_lower.split('.').next().unwrap_or(&lbl_lower))
                || lookup_directive(&lbl_lower).is_some()
            {
                mnemonic = lbl.clone();
                label = None;
            }
        }

        if mnemonic.is_empty() && label.is_none() {
            output.push(line.trim_end().to_string());
            continue;
        }

        let mut formatted_line = String::new();

        // 1. Label
        if let Some(lbl) = label {
            formatted_line.push_str(&lbl);
            if !code_part.trim().ends_with(':') && !lbl.ends_with(':') && !mnemonic.is_empty() {
                formatted_line.push(':');
            }
        }

        // 2. Mnemonic & size
        if !mnemonic.is_empty() {
            let mnem_with_size = if !size.is_empty() {
                format!("{}.{}", mnemonic, size)
            } else {
                mnemonic
            };

            if formatted_line.is_empty() {
                // Indent with config spaces
                let pad = " ".repeat(config.indent_spaces);
                formatted_line.push_str(&pad);
            } else {
                // Pad from label to mnemonic column
                let current_len = formatted_line.len();
                if current_len < config.mnemonic_col {
                    let pad = " ".repeat(config.mnemonic_col - current_len);
                    formatted_line.push_str(&pad);
                } else {
                    formatted_line.push(' ');
                }
            }

            formatted_line.push_str(&mnem_with_size);

            // 3. Operands
            if !operands.is_empty() {
                let sep = if config.space_after_comma { ", " } else { "," };
                let ops_str = operands.join(sep);

                let current_len = formatted_line.len();
                if current_len < config.operand_col {
                    let pad = " ".repeat(config.operand_col - current_len);
                    formatted_line.push_str(&pad);
                } else {
                    formatted_line.push(' ');
                }

                formatted_line.push_str(&ops_str);
            }
        }

        // 4. End-of-line comment
        if let Some(comment) = comment_part {
            let current_len = formatted_line.len();
            if current_len < config.comment_col {
                let pad = " ".repeat(config.comment_col - current_len);
                formatted_line.push_str(&pad);
            } else {
                formatted_line.push_str("  ");
            }
            formatted_line.push_str(&comment);
        }

        output.push(formatted_line.trim_end().to_string());
    }

    let mut result = output.join("\n");
    if source.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn extract_line_comment(line: &str) -> (&str, Option<String>) {
    let mut in_quote = false;
    let mut quote_char = ' ';

    for (i, ch) in line.char_indices() {
        if in_quote {
            if ch == quote_char {
                in_quote = false;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_quote = true;
            quote_char = ch;
        } else if ch == ';' {
            let code = &line[..i];
            let comment = &line[i..];
            return (code, Some(comment.trim_end().to_string()));
        }
    }

    (line, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_source() {
        let src = "START:\nmove.w #1,d0 ; set d0\n.loop: dbra d0,.loop\nrts\n";
        let config = AsmFormatConfig::default();
        let formatted = format_source(src, &config);

        assert!(formatted.contains("    move.w          #1, d0"));
        assert!(formatted.contains("; set d0"));
        assert!(formatted.contains("    rts"));
    }
}
