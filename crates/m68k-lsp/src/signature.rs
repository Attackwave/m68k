//! Signature help provider for active opcode operand prompts.

use tower_lsp::lsp_types::{
    ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation,
};

use crate::analysis::op_docs::lookup_opcode;
use crate::document::Document;

/// Compute signature help for active instruction operands.
pub fn compute_signature_help(doc: &Document, pos: Position) -> Option<SignatureHelp> {
    let line_idx = pos.line as usize;
    let char_idx = pos.character as usize;

    let line = doc.lines.get(line_idx)?;
    if line.is_empty() || char_idx > line.len() {
        return None;
    }

    let prefix = &line[..char_idx];
    let parsed = doc.parsed_lines.get(line_idx)?;

    if parsed.mnemonic.is_empty() {
        return None;
    }

    let doc_entry = lookup_opcode(&parsed.mnemonic)?;

    // Calculate active parameter index by counting commas before cursor
    let mut comma_count = 0;
    let mut in_quote = false;
    for ch in prefix.chars() {
        if ch == '"' || ch == '\'' {
            in_quote = !in_quote;
        } else if ch == ',' && !in_quote {
            comma_count += 1;
        }
    }

    let mut signatures = Vec::new();

    for syn in doc_entry.syntax {
        let params: Vec<ParameterInformation> = syn
            .split_whitespace()
            .skip(1)
            .collect::<Vec<&str>>()
            .join(" ")
            .split(',')
            .map(|p| ParameterInformation {
                label: ParameterLabel::Simple(p.trim().to_string()),
                documentation: None,
            })
            .collect();

        signatures.push(SignatureInformation {
            label: syn.to_string(),
            documentation: Some(tower_lsp::lsp_types::Documentation::String(
                doc_entry.summary.to_string(),
            )),
            parameters: Some(params),
            active_parameter: Some(comma_count as u32),
        });
    }

    Some(SignatureHelp {
        signatures,
        active_signature: Some(0),
        active_parameter: Some(comma_count as u32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_signature_help_for_move() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "    move.w  d0, ";
        let doc = Document::new(uri, 1, src.to_string());

        let sig = compute_signature_help(
            &doc,
            Position {
                line: 0,
                character: 15,
            },
        )
        .expect("should find signature");

        assert_eq!(sig.active_parameter, Some(1));
        assert!(!sig.signatures.is_empty());
    }
}
