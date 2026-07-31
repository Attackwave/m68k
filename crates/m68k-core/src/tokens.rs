//! Tokenizer for m68k assembler source.
//!
//! Produces `Token(kind, text, value, col)` sequences.

/// Token kinds produced by the tokenizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Ident,
    Num,
    Punct,
    Char,
    Str,
}

/// A single token from the source.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
    pub value: Option<u64>,
    pub col: usize,
}

impl Token {
    pub fn new(kind: TokenKind, text: String, value: Option<u64>, col: usize) -> Self {
        Self {
            kind,
            text,
            value,
            col,
        }
    }

    pub fn is_ident(&self) -> bool {
        self.kind == TokenKind::Ident
    }

    pub fn is_num(&self) -> bool {
        self.kind == TokenKind::Num
    }

    pub fn is_punct(&self) -> bool {
        self.kind == TokenKind::Punct
    }

    pub fn matches(&self, expected: &str) -> bool {
        self.text == expected
    }
}

/// Tokenize a single source line, dropping comments and whitespace.
pub fn tokenize(line: &str) -> Vec<Token> {
    // Handle * comment (column 0 only)
    if line.trim_start().starts_with('*') {
        return vec![];
    }

    let mut tokens = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        // Skip whitespace
        if ch.is_whitespace() {
            i += 1;
            continue;
        }

        // Comment
        if ch == ';' {
            break;
        }

        let col = i;

        // String literal
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let mut text = String::new();
            text.push(ch);
            i += 1;
            while i < chars.len() && chars[i] != quote {
                text.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                text.push(chars[i]);
                i += 1;
            }
            tokens.push(Token::new(TokenKind::Str, text, None, col));
            continue;
        }

        // Hex ($FF)
        if ch == '$' && i + 1 < chars.len() && chars[i + 1].is_ascii_hexdigit() {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let value = u64::from_str_radix(&text[1..], 16).ok();
            tokens.push(Token::new(TokenKind::Num, text, value, col));
            continue;
        }

        // Hex (0xFF)
        if ch == '0' && i + 1 < chars.len() && chars[i + 1] == 'x' {
            let start = i;
            i += 2;
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let value = u64::from_str_radix(&text[2..], 16).ok();
            tokens.push(Token::new(TokenKind::Num, text, value, col));
            continue;
        }

        // Binary (%1010)
        if ch == '%' && i + 1 < chars.len() && (chars[i + 1] == '0' || chars[i + 1] == '1') {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i] == '0' || chars[i] == '1') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let value = u64::from_str_radix(&text[1..], 2).ok();
            tokens.push(Token::new(TokenKind::Num, text, value, col));
            continue;
        }

        // Number
        if ch.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let value = text.parse::<u64>().ok();
            tokens.push(Token::new(TokenKind::Num, text, value, col));
            continue;
        }

        // Identifier
        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric()
                    || chars[i] == '_'
                    || chars[i] == '.'
                    || chars[i] == '$')
            {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            tokens.push(Token::new(TokenKind::Ident, text, None, col));
            continue;
        }

        // Punctuation (multi-char first)
        if ch == '<' && i + 1 < chars.len() && chars[i + 1] == '<' {
            tokens.push(Token::new(TokenKind::Punct, "<<".to_string(), None, col));
            i += 2;
            continue;
        }
        if ch == '>' && i + 1 < chars.len() && chars[i + 1] == '>' {
            tokens.push(Token::new(TokenKind::Punct, ">>".to_string(), None, col));
            i += 2;
            continue;
        }

        // Single-char punctuation
        if "()[]{},.+*/:~|&^=#-".contains(ch) {
            tokens.push(Token::new(TokenKind::Punct, ch.to_string(), None, col));
            i += 1;
            continue;
        }

        // Unknown character, skip
        i += 1;
    }

    tokens
}

/// Byte offset of a `*` that starts a comment, if the line has one.
///
/// Motorola syntax overloads `*` three ways: comment marker, multiply
/// operator, and "current PC". They are told apart by position:
///
/// * column 1 — always a comment (`* header banner`);
/// * preceded by whitespace *and* followed by whitespace, or by a word
///   that can't continue an expression — a trailing comment
///   (`FOO EQU 0    * for assembler's sake`);
/// * anywhere else — the operator or the PC symbol (`WIDTH*HEIGHT`,
///   `BRA *`, `DC.L *+4`).
///
/// Quoted text is skipped so a `*` inside a string stays literal.
fn find_star_comment(line: &str) -> Option<usize> {
    if line.starts_with('*') {
        return Some(0);
    }

    let bytes = line.as_bytes();
    let mut in_quote = false;
    let mut quote_char = 0u8;
    for (i, ch) in line.char_indices() {
        let b = bytes[i];
        if in_quote {
            if b == quote_char {
                in_quote = false;
            }
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_quote = true;
            quote_char = b;
            continue;
        }
        if ch != '*' || i == 0 {
            continue;
        }
        // Must follow whitespace: `A*2` is a multiply.
        if !bytes[i - 1].is_ascii_whitespace() {
            continue;
        }
        // Whitespace *before* the star is necessary but not sufficient —
        // `A * 2` is still a multiply. What follows decides:
        //
        //   * a letter or `[`/`_` starts prose — a comment
        //     (`MACRO   * [baseOffset]`, `EQU 0  * for assembler's sake`);
        //   * a digit, `$`, `(`, `%` or an operator continues the
        //     expression (`A * 2`, `SIZE * $10`);
        //   * end of line is the PC symbol (`BRA *`).
        //
        // `[` matters specifically because the Amiga headers document
        // optional macro parameters as `* [baseOffset]`; treating that as
        // a parameter list left `\1` unsubstituted in the macro body.
        let rest = line[i + 1..].trim_start();
        match rest.chars().next() {
            Some(c) if c.is_alphabetic() || c == '[' || c == '_' => return Some(i),
            _ => continue,
        }
    }
    None
}

/// Split a source line into (label, mnemonic, size, operand_texts).
pub fn split_line(line: &str) -> (Option<String>, String, String, Vec<String>) {
    let mut line = line.to_string();

    // Remove inline comments (not inside quotes)
    let mut in_quote = false;
    let mut quote_char = ' ';
    let mut comment_pos = None;

    // `char_indices()` (not `chars().enumerate()`) is required here: the
    // index must be a byte offset for the `line[..pos]` slice below, but
    // `enumerate()` counts *characters*, which only coincides with the
    // byte offset for pure-ASCII input. Any multi-byte UTF-8 character
    // before a `;` would otherwise slice mid-codepoint and panic.
    for (i, ch) in line.char_indices() {
        if in_quote {
            if ch == quote_char {
                in_quote = false;
            }
        } else if ch == '"' || ch == '\'' {
            in_quote = true;
            quote_char = ch;
        } else if ch == ';' {
            comment_pos = Some(i);
            break;
        }
    }

    if let Some(pos) = comment_pos {
        line = line[..pos].to_string();
    }

    // `*` also starts a comment in Motorola syntax, but only where an
    // operand can't continue: at the start of the line, or after
    // whitespace that follows a complete operand field. The Amiga system
    // headers rely on this — `FOO EQU 0    * for assembler's sake`.
    //
    // Crucially it must stay a multiply operator when it appears *inside*
    // an expression (`SIZE EQU WIDTH*HEIGHT`), and `*` alone is also the
    // "current PC" symbol, so only a `*` preceded by whitespace and
    // followed by something other than an operand character is treated as
    // a comment.
    if let Some(pos) = find_star_comment(&line) {
        line = line[..pos].to_string();
    }

    // Whether the source line began flush left, before trimming — used
    // below to tell a bare label in column 1 from an indented mnemonic.
    let starts_in_column_1 = line.chars().next().is_some_and(|c| !c.is_whitespace());

    let line = line.trim();
    if line.is_empty() {
        return (None, String::new(), String::new(), vec![]);
    }

    // Check for label: colon form
    let mut result: Option<(Option<String>, String, String, Vec<String>)> = None;
    if let Some(colon_pos) = line.find(':') {
        let potential_label = &line[..colon_pos];
        if is_valid_ident(potential_label.trim()) {
            let label = Some(potential_label.trim().to_string());
            let rest = line[colon_pos + 1..].trim();
            result = Some(if rest.is_empty() {
                (label, String::new(), String::new(), vec![])
            } else {
                let (_, m, s, o) = parse_rest(rest);
                (label, m, s, o)
            });
        }
    }
    if let Some(r) = result {
        return r;
    }

    // Check for label without colon (IDENT followed by directive)
    let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
    if parts.len() == 2 && is_valid_ident(parts[0]) && !takes_cache_scope_operand(parts[0]) {
        let rest = parts[1].trim();
        let rest_parts: Vec<&str> = rest.splitn(2, char::is_whitespace).collect();
        if let Some(first) = rest_parts.first() {
            let mnem = first.split('.').next().unwrap_or(first).to_lowercase();
            if is_directive(&mnem) {
                return parse_rest_with_label(rest, Some(parts[0].to_string()));
            }
        }
    }

    // A label alone on its line, without a colon — `.exit` or `START`
    // written in column 1. Motorola assemblers treat a bare identifier
    // starting in column 1 as a label definition, colon or not.
    //
    // The column-1 test is what keeps this from swallowing operand-less
    // mnemonics: `RTS` and `NOP` are written indented, so they still parse
    // as instructions and an indented typo is still reported as an unknown
    // mnemonic rather than silently becoming a label.
    if starts_in_column_1 && parts.len() == 1 && is_valid_ident(line) {
        return (Some(line.to_string()), String::new(), String::new(), vec![]);
    }

    parse_rest(line)
}

fn parse_rest_with_label(
    rest: &str,
    label: Option<String>,
) -> (Option<String>, String, String, Vec<String>) {
    let (label2, mnemonic, size, operands) = parse_rest(rest);
    let label = label.or(label2);
    (label, mnemonic, size, operands)
}

fn parse_rest(rest: &str) -> (Option<String>, String, String, Vec<String>) {
    if rest.is_empty() {
        return (None, String::new(), String::new(), vec![]);
    }

    let parts: Vec<&str> = rest.splitn(2, char::is_whitespace).collect();
    let mnemonic_full = parts[0];
    let operand_str = if parts.len() > 1 { parts[1].trim() } else { "" };

    let (mnemonic, size) = if let Some(dot_pos) = mnemonic_full.find('.') {
        (
            mnemonic_full[..dot_pos].to_lowercase(),
            mnemonic_full[dot_pos + 1..].to_lowercase(),
        )
    } else {
        (mnemonic_full.to_lowercase(), String::new())
    };

    let operands = if operand_str.is_empty() {
        vec![]
    } else {
        split_operands(operand_str)
    };

    (None, mnemonic, size, operands)
}

fn split_operands(text: &str) -> Vec<String> {
    let mut operands = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote_char = ' ';

    for ch in text.chars() {
        if in_quote {
            current.push(ch);
            if ch == quote_char {
                in_quote = false;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_quote = true;
            quote_char = ch;
            current.push(ch);
        } else if ch == '(' || ch == '[' || ch == '{' {
            depth += 1;
            current.push(ch);
        } else if ch == ')' || ch == ']' || ch == '}' {
            depth -= 1;
            current.push(ch);
        } else if ch == ',' && depth == 0 {
            let val = current.trim().to_string();
            if !val.is_empty() {
                operands.push(val);
            }
            current.clear();
        } else {
            current.push(ch);
        }
    }

    let val = current.trim().to_string();
    if !val.is_empty() {
        operands.push(val);
    }

    operands
}

pub fn is_valid_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // A leading '.' marks a *local* label (`.loop`), the convention used by
    // vasm, Devpac, PhxAss and ASM-One: the name is scoped to the preceding
    // global label, so the same `.loop` may appear once per subroutine.
    // Such a name must still have something after the dot.
    let body = match s.strip_prefix('.') {
        Some(rest) => {
            if rest.is_empty() {
                return false;
            }
            rest
        }
        None => s,
    };
    let mut chars = body.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '$')
}

/// Whether `name` is a local label (`.loop`) rather than a global one.
///
/// Callers scope these against the most recent global label; see
/// `Assembler`'s label handling.
pub fn is_local_label(name: &str) -> bool {
    name.len() > 1 && name.starts_with('.')
}

/// The 68040 cache instructions take a cache-scope name as their first
/// operand, one of which (`DC`) is spelled exactly like the define-constant
/// directive. Without this exclusion, `CPUSHA DC` parses as "label CPUSHA,
/// directive DC" and fails with "DC requires size and values" — the
/// label-without-colon heuristic below can't tell the two apart on its own.
fn takes_cache_scope_operand(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "cinva" | "cpusha" | "cinvl" | "cinvp" | "cpushl" | "cpushp"
    )
}

fn is_directive(s: &str) -> bool {
    matches!(
        s,
        "org"
            | "equ"
            | "dc"
            | "dcb"
            | "ds"
            | "even"
            | "align"
            | "include"
            | "incbin"
            | "macro"
            | "endm"
            | "section"
            | "set"
            | "xref"
            | "xdef"
            | "public"
            | "extern"
            | "rept"
            | "irp"
            | "irpc"
            | "endr"
            | "if"
            | "ifeq"
            | "ifne"
            | "ifgt"
            | "iflt"
            | "ifge"
            | "ifle"
            | "ifdef"
            | "ifndef"
            | "ifc"
            | "ifnc"
            | "end"
            | "fail"
            | "warning"
            | "error"
            | "rs"
            | "rsreset"
            | "rsset"
            | "opt"
            | "cnop"
            | "offset"
            | "mexit"
            | "exitm"
            | "print"
            | "printt"
            | "printv"
            | "list"
            | "nolist"
            | "page"
            | "title"
            | "sopt"
            | "llen"
            | "plen"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_line_label_colon() {
        let result = split_line("label: nopmac");
        eprintln!("split_line('label: nopmac') = {:?}", result);
        assert_eq!(
            result.0,
            Some("label".to_string()),
            "label should be Some('label')"
        );
        assert_eq!(result.1, "nopmac", "mnemonic should be 'nopmac'");
    }

    #[test]
    fn test_split_line_label_colon_2() {
        let result = split_line("myLabel: instruction arg1,arg2");
        eprintln!(
            "split_line('myLabel: instruction arg1,arg2') = {:?}",
            result
        );
        assert_eq!(result.0, Some("myLabel".to_string()));
        assert_eq!(result.1, "instruction");
    }

    #[test]
    fn test_split_line_multibyte_utf8_before_comment_does_not_panic() {
        // Fuzzing regression: the comment-stripping loop used to pair a
        // `chars().enumerate()` *character* index with a byte-indexed
        // `line[..pos]` slice. Any multi-byte UTF-8 character before a `;`
        // made the character index diverge from the byte offset, slicing
        // mid-codepoint and panicking ("byte index is not a char
        // boundary"). `\u{fffd}` (replacement character, 3 bytes) is what
        // `String::from_utf8_lossy` produces for invalid input, which is
        // how the fuzz harness (assembler_pipeline) reached this.
        let result = split_line("\u{fffd} ; comment");
        assert_eq!(result.1, "\u{fffd}");
    }
}
