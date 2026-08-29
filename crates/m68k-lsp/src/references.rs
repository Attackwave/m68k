//! Reference search provider (Find All References) for labels, constants, and macros.

use tower_lsp::lsp_types::{Location, Position, Range};

use crate::document::Document;

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
        let mut search_start = 0;
        while let Some(found_idx) = line[search_start..].find(clean_target) {
            let actual_idx = search_start + found_idx;
            search_start = actual_idx + clean_target.len();

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
                            character: actual_idx as u32,
                        },
                        end: Position {
                            line: line_idx as u32,
                            character: (actual_idx + clean_target.len()) as u32,
                        },
                    },
                });
            }
        }
    }

    locations
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
}
