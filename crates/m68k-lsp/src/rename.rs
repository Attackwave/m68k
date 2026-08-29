//! Rename refactoring provider for labels, constants, and macros.

use std::collections::HashMap;
use tower_lsp::lsp_types::{Position, Range, TextEdit, WorkspaceEdit};

use m68k_core::tokens::is_valid_ident;

use crate::document::{Document, WordKind};
use crate::references::compute_references;

/// Check if the symbol at position can be renamed.
pub fn prepare_rename(doc: &Document, pos: Position) -> Option<Range> {
    let word_info = doc.get_word_at_position(pos)?;

    // Can only rename labels, symbols, or equates (not keywords, registers, LVOs)
    if word_info.kind == WordKind::Register
        || word_info.kind == WordKind::Mnemonic
        || word_info.kind == WordKind::Directive
        || word_info.kind == WordKind::Lvo
    {
        return None;
    }

    Some(word_info.range)
}

/// Compute rename workspace edits.
pub fn compute_rename(doc: &Document, pos: Position, new_name: &str) -> Option<WorkspaceEdit> {
    if !is_valid_ident(new_name) {
        return None;
    }

    // Verify renameable
    let _ = prepare_rename(doc, pos)?;

    let references = compute_references(doc, pos, true);
    if references.is_empty() {
        return None;
    }

    let mut text_edits = Vec::new();
    for loc in references {
        text_edits.push(TextEdit {
            range: loc.range,
            new_text: new_name.to_string(),
        });
    }

    let mut changes = HashMap::new();
    changes.insert(doc.uri.clone(), text_edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_compute_rename() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "OldName:\n    bra OldName\n";
        let doc = Document::new(uri.clone(), 1, src.to_string());

        let edit = compute_rename(
            &doc,
            Position {
                line: 0,
                character: 2,
            },
            "NewName",
        )
        .expect("rename should succeed");

        let edits = &edit.changes.unwrap()[&uri];
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].new_text, "NewName");
        assert_eq!(edits[1].new_text, "NewName");
    }
}
