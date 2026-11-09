//! Reference search provider (Find All References) for labels, constants, and macros.

use tower_lsp::lsp_types::{Location, Position, Range};

use crate::document::{Document, byte_to_utf16_col};

/// Compute all reference locations for the symbol under the cursor.
pub fn compute_references(
    doc: &Document,
    pos: Position,
    include_declaration: bool,
) -> Vec<Location> {
    let mut locations = Vec::new();

    let word_info = match doc.get_word_at_position(pos) {
        Some(w) => w,
        None => return locations,
    };

    let target_symbol = word_info.word.trim();
    if target_symbol.is_empty() {
        return locations;
    }

    // Strip local dot prefix for comparison if necessary
    let clean_target = target_symbol.trim_start_matches('.');

    for (line_idx, line) in doc.lines.iter().enumerate() {
        let code_end = code_length(line);
        let mut search_start = 0;
        while let Some(found_idx) = line[search_start..].find(clean_target) {
            let actual_idx = search_start + found_idx;
            search_start = actual_idx + clean_target.len();
            // A name inside a comment or a string literal is text, not a
            // reference: renaming it would rewrite data the program emits.
            if actual_idx >= code_end || inside_quotes(&line[..actual_idx]) {
                continue;
            }

            // Verify word boundary before and after
            let is_boundary_before = if actual_idx == 0 {
                true
            } else {
                let prev = line.as_bytes()[actual_idx - 1] as char;
                !prev.is_alphanumeric() && prev != '_' && prev != '.' && prev != '$'
            };

            let is_boundary_after = if actual_idx + clean_target.len() >= line.len() {
                true
            } else {
                let next = line.as_bytes()[actual_idx + clean_target.len()] as char;
                !next.is_alphanumeric() && next != '_' && next != '$'
            };

            if is_boundary_before && is_boundary_after {
                // Check if this is the declaration line and if we should filter it
                let is_declaration = doc
                    .symbols
                    .get(target_symbol)
                    .map(|s| s.line_idx == line_idx && s.character == actual_idx)
                    .unwrap_or(false);

                if is_declaration && !include_declaration {
                    continue;
                }

                locations.push(Location {
                    uri: doc.uri.clone(),
                    range: Range {
                        start: Position {
                            line: line_idx as u32,
                            character: byte_to_utf16_col(line, actual_idx),
                        },
                        end: Position {
                            line: line_idx as u32,
                            character: byte_to_utf16_col(line, actual_idx + clean_target.len()),
                        },
                    },
                });
            }
        }
    }

    locations
}

/// Byte length of the code part of `line`: everything before a `;`
/// comment (outside quotes); 0 for a `*` full-line comment.
fn code_length(line: &str) -> usize {
    if line.starts_with('*') {
        return 0;
    }
    let mut quote = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, ';') => return i,
            _ => {}
        }
    }
    line.len()
}

/// Whether the end of `prefix` lies inside a quoted string.
fn inside_quotes(prefix: &str) -> bool {
    let mut quote = None;
    for c in prefix.chars() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            _ => {}
        }
    }
    quote.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_compute_references() {
        let uri = Url::parse("file:///test.s").unwrap();
        let src = "START:\n    bra     Loop\nLoop:\n    dbra    d0,Loop\n    rts\n";
        let doc = Document::new(uri, 1, src.to_string());

        let refs = compute_references(
            &doc,
            Position {
                line: 2,
                character: 1,
            },
            true,
        );

        assert_eq!(
            refs.len(),
            3,
            "Expected 3 occurrences of 'Loop', found: {:?}",
            refs
        );
    }

    #[test]
    fn references_skip_comments_and_strings() {
        let src = "Lbl:\n bra Lbl ; Größe Lbl\n; Lbl\n* Lbl\n dc.b \"Lbl\",0\n dc.l Lbl\n";
        let doc = Document::new(Url::parse("file:///t.s").unwrap(), 1, src.to_string());
        let refs = compute_references(&doc, Position::new(0, 0), true);
        let lines: Vec<u32> = refs.iter().map(|l| l.range.start.line).collect();
        assert_eq!(lines, [0, 1, 5]);
        assert_eq!(refs[1].range.start.character, 5);
    }
}
