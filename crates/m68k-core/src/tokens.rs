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

        // Character literal
        if ch == '\'' && i + 2 < chars.len() && chars[i + 2] == '\'' {
            let text: String = chars[i..i + 3].iter().collect();
            tokens.push(Token::new(
                TokenKind::Char,
                text,
                Some(chars[i + 1] as u64),
                col,
            ));
            i += 3;
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

/// Split a source line into (label, mnemonic, size, operand_texts).
pub fn split_line(line: &str) -> (Option<String>, String, String, Vec<String>) {
    let mut line = line.to_string();

    // Remove inline comments (not inside quotes)
    let mut in_quote = false;
    let mut quote_char = ' ';
    let mut comment_pos = None;

    for (i, ch) in line.chars().enumerate() {
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
    if parts.len() == 2 && is_valid_ident(parts[0]) {
        let rest = parts[1].trim();
        let rest_parts: Vec<&str> = rest.splitn(2, char::is_whitespace).collect();
        if let Some(first) = rest_parts.first() {
            let mnem = first.split('.').next().unwrap_or(first).to_lowercase();
            if is_directive(&mnem) {
                return parse_rest_with_label(rest, Some(parts[0].to_string()));
            }
        }
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
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '$')
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
}
