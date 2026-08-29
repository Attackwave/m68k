//! Semantic tokens provider for rich syntax highlighting in m68k assembly.

use tower_lsp::lsp_types::{
    SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensLegend,
};

use m68k_core::tokens::{is_local_label, is_mnemonic, is_valid_ident};

use crate::analysis::directives_docs::lookup_directive;
use crate::document::{Document, is_register};

pub const LEGEND_TYPE_KEYWORD: u32 = 0;
pub const LEGEND_TYPE_FUNCTION: u32 = 1;
pub const LEGEND_TYPE_VARIABLE: u32 = 2;
pub const LEGEND_TYPE_MACRO: u32 = 3;
pub const LEGEND_TYPE_NUMBER: u32 = 4;
pub const LEGEND_TYPE_STRING: u32 = 5;
pub const LEGEND_TYPE_COMMENT: u32 = 6;

/// Semantic tokens legend declaring available token types.
pub fn get_semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,  // 0: Instructions
            SemanticTokenType::FUNCTION, // 1: Labels
            SemanticTokenType::VARIABLE, // 2: Registers
            SemanticTokenType::MACRO,    // 3: Directives / Macros
            SemanticTokenType::NUMBER,   // 4: Numbers
            SemanticTokenType::STRING,   // 5: Strings
            SemanticTokenType::COMMENT,  // 6: Comments
        ],
        token_modifiers: vec![],
    }
}

/// Compute semantic tokens for a document.
pub fn compute_semantic_tokens(doc: &Document) -> SemanticTokens {
    let mut raw_tokens: Vec<(u32, u32, u32, u32, u32)> = Vec::new(); // (line, start, len, type, mods)

    for (line_idx, line) in doc.lines.iter().enumerate() {
        let line_num = line_idx as u32;
        let mut in_quote = false;
        let mut quote_char = ' ';
        let mut comment_start: Option<usize> = None;

        // Check for full-line comment (* in col 0 or trimmed starts with ;)
        if line.starts_with('*') || line.trim_start().starts_with(';') {
            raw_tokens.push((line_num, 0, line.len() as u32, LEGEND_TYPE_COMMENT, 0));
            continue;
        }

        // Find inline comment position
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
                comment_start = Some(i);
                break;
            }
        }

        let code_len = comment_start.unwrap_or(line.len());

        // Tokenize code segment
        let mut i = 0;
        let chars: Vec<char> = line[..code_len].chars().collect();

        while i < chars.len() {
            let ch = chars[i];

            // Skip whitespace
            if ch.is_whitespace() {
                i += 1;
                continue;
            }

            // String literals
            if ch == '"' || ch == '\'' {
                let start = i;
                let q = ch;
                i += 1;
                while i < chars.len() && chars[i] != q {
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
                raw_tokens.push((
                    line_num,
                    start as u32,
                    (i - start) as u32,
                    LEGEND_TYPE_STRING,
                    0,
                ));
                continue;
            }

            // Numbers: Hex ($FF, 0xFF), Binary (%1010), Decimal (1234), Immediate (#...)
            if ch == '$' || ch == '%' || ch == '#' || ch.is_ascii_digit() {
                let start = i;
                i += 1;
                while i < chars.len()
                    && (chars[i].is_ascii_hexdigit()
                        || chars[i] == 'x'
                        || chars[i] == 'X'
                        || chars[i] == '_'
                        || chars[i] == '%')
                {
                    i += 1;
                }
                raw_tokens.push((
                    line_num,
                    start as u32,
                    (i - start) as u32,
                    LEGEND_TYPE_NUMBER,
                    0,
                ));
                continue;
            }

            // Identifiers / words (labels, mnemonics, registers, directives)
            if ch.is_alphabetic() || ch == '_' || ch == '.' {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric()
                        || chars[i] == '_'
                        || chars[i] == '.'
                        || chars[i] == '$')
                {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let lower = word.to_ascii_lowercase();

                let token_type = if is_register(&lower) {
                    LEGEND_TYPE_VARIABLE
                } else if is_mnemonic(&lower)
                    || is_mnemonic(lower.split('.').next().unwrap_or(&lower))
                {
                    LEGEND_TYPE_KEYWORD
                } else if lookup_directive(&lower).is_some()
                    || lookup_directive(lower.split('.').next().unwrap_or(&lower)).is_some()
                {
                    LEGEND_TYPE_MACRO
                } else if is_local_label(&word) || is_valid_ident(&word) {
                    LEGEND_TYPE_FUNCTION
                } else {
                    LEGEND_TYPE_KEYWORD
                };

                raw_tokens.push((line_num, start as u32, (i - start) as u32, token_type, 0));
                continue;
            }

            i += 1;
        }

        // Add inline comment if present
        if let Some(c_start) = comment_start {
            raw_tokens.push((
                line_num,
                c_start as u32,
                (line.len() - c_start) as u32,
                LEGEND_TYPE_COMMENT,
                0,
            ));
        }
    }

    // Convert raw tokens to LSP delta-encoded format
    let mut data = Vec::new();
    let mut prev_line = 0;
    let mut prev_char = 0;

    for (line, start, len, token_type, mods) in raw_tokens {
        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            start.saturating_sub(prev_char)
        } else {
            start
        };

        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: len,
            token_type,
            token_modifiers_bitset: mods,
        });

        prev_line = line;
        prev_char = start;
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_compute_semantic_tokens() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "START:\n    move.w  #$1000,d0 ; init\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());
        let tokens = compute_semantic_tokens(&doc);

        assert!(!tokens.data.is_empty(), "Should produce semantic tokens");
    }
}
