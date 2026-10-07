//! Python-to-m68k AOT Transpiler & Compiler Engine for m68k Studio.
//! Translates a clean, typed Python subset directly into high-performance Motorola 68000 assembly.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct TranspilePythonRequest {
    pub python_code: string_or_source::PythonSource,
    pub target_platform: Option<String>, // "amiga", "megadrive", "baremetal"
    pub target_cpu: Option<String>,      // "68000", "68020", etc.
}

mod string_or_source {
    use serde::{Deserialize, Deserializer};

    #[derive(Debug, Clone)]
    pub struct PythonSource(pub String);

    impl<'de> Deserialize<'de> for PythonSource {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            Ok(PythonSource(s))
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TranspilePythonResponse {
    pub success: bool,
    pub asm_code: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum PyToken {
    Def,
    Return,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    Range,
    Pass,
    Import,
    From,
    True,
    False,
    Ident(String),
    Number(i64),
    HexNumber(i64),
    Str(String),
    Assign,   // =
    Plus,     // +
    Minus,    // -
    Star,     // *
    Slash,    // /
    Amp,      // &
    Pipe,     // |
    Caret,    // ^
    Shl,      // <<
    Shr,      // >>
    Eq,       // ==
    NotEq,    // !=
    Lt,       // <
    Lte,      // <=
    Gt,       // >
    Gte,      // >=
    LParen,   // (
    RParen,   // )
    LBracket, // [
    RBracket, // ]
    Colon,    // :
    Comma,    // ,
    Dot,      // .
    Newline,
    Indent,
    Dedent,
}

fn tokenize(source: &str) -> Result<Vec<PyToken>, String> {
    let mut tokens = Vec::new();
    let mut indent_stack = vec![0];
    let lines = source.lines();

    for raw_line in lines {
        let trimmed = raw_line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue; // Skip comments and empty lines
        }

        let leading_spaces = raw_line.len() - trimmed.len();
        let current_indent = *indent_stack.last().unwrap_or(&0);

        if leading_spaces > current_indent {
            indent_stack.push(leading_spaces);
            tokens.push(PyToken::Indent);
        } else if leading_spaces < current_indent {
            while let Some(&top) = indent_stack.last() {
                if top > leading_spaces {
                    indent_stack.pop();
                    tokens.push(PyToken::Dedent);
                } else {
                    break;
                }
            }
        }

        let mut chars = trimmed.chars().peekable();

        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else if c == '#' {
                // Comment until end of line
                break;
            } else if c == '"' || c == '\'' {
                let quote = chars.next().unwrap();
                let mut s = String::new();
                while let Some(&ch) = chars.peek() {
                    chars.next();
                    if ch == quote {
                        break;
                    }
                    s.push(ch);
                }
                tokens.push(PyToken::Str(s));
            } else if c.is_alphabetic() || c == '_' {
                let mut ident = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        ident.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                match ident.as_str() {
                    "def" => tokens.push(PyToken::Def),
                    "return" => tokens.push(PyToken::Return),
                    "if" => tokens.push(PyToken::If),
                    "elif" => tokens.push(PyToken::Elif),
                    "else" => tokens.push(PyToken::Else),
                    "while" => tokens.push(PyToken::While),
                    "for" => tokens.push(PyToken::For),
                    "in" => tokens.push(PyToken::In),
                    "range" => tokens.push(PyToken::Range),
                    "pass" => tokens.push(PyToken::Pass),
                    "import" => tokens.push(PyToken::Import),
                    "from" => tokens.push(PyToken::From),
                    "True" => tokens.push(PyToken::True),
                    "False" => tokens.push(PyToken::False),
                    _ => tokens.push(PyToken::Ident(ident)),
                }
            } else if c.is_ascii_digit() {
                let mut num_str = String::new();
                let mut is_hex = false;
                if c == '0' {
                    num_str.push(chars.next().unwrap());
                    if let Some(&next_c) = chars.peek()
                        && (next_c == 'x' || next_c == 'X')
                    {
                        chars.next();
                        is_hex = true;
                        while let Some(&hc) = chars.peek() {
                            if hc.is_ascii_hexdigit() {
                                num_str.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                    }
                }

                if !is_hex {
                    while let Some(&dc) = chars.peek() {
                        if dc.is_ascii_digit() {
                            num_str.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                }

                if is_hex {
                    let val = i64::from_str_radix(&num_str[1..], 16).unwrap_or(0);
                    tokens.push(PyToken::HexNumber(val));
                } else {
                    let val = num_str.parse::<i64>().unwrap_or(0);
                    tokens.push(PyToken::Number(val));
                }
            } else {
                chars.next();
                match c {
                    '=' => {
                        if chars.peek() == Some(&'=') {
                            chars.next();
                            tokens.push(PyToken::Eq);
                        } else {
                            tokens.push(PyToken::Assign);
                        }
                    }
                    '!' => {
                        if chars.peek() == Some(&'=') {
                            chars.next();
                            tokens.push(PyToken::NotEq);
                        }
                    }
                    '<' => {
                        if chars.peek() == Some(&'=') {
                            chars.next();
                            tokens.push(PyToken::Lte);
                        } else if chars.peek() == Some(&'<') {
                            chars.next();
                            tokens.push(PyToken::Shl);
                        } else {
                            tokens.push(PyToken::Lt);
                        }
                    }
                    '>' => {
                        if chars.peek() == Some(&'=') {
                            chars.next();
                            tokens.push(PyToken::Gte);
                        } else if chars.peek() == Some(&'>') {
                            chars.next();
                            tokens.push(PyToken::Shr);
                        } else {
                            tokens.push(PyToken::Gt);
                        }
                    }
                    '+' => tokens.push(PyToken::Plus),
                    '-' => tokens.push(PyToken::Minus),
                    '*' => tokens.push(PyToken::Star),
                    '/' => tokens.push(PyToken::Slash),
                    '&' => tokens.push(PyToken::Amp),
                    '|' => tokens.push(PyToken::Pipe),
                    '^' => tokens.push(PyToken::Caret),
                    '(' => tokens.push(PyToken::LParen),
                    ')' => tokens.push(PyToken::RParen),
                    '[' => tokens.push(PyToken::LBracket),
                    ']' => tokens.push(PyToken::RBracket),
                    ':' => tokens.push(PyToken::Colon),
                    ',' => tokens.push(PyToken::Comma),
                    '.' => tokens.push(PyToken::Dot),
                    _ => {}
                }
            }
        }
        tokens.push(PyToken::Newline);
    }

    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(PyToken::Dedent);
    }

    Ok(tokens)
}

pub struct TranspilerContext {
    pub platform: String,
    pub label_counter: usize,
    pub asm_output: String,
    pub vars: HashMap<String, String>, // var_name -> data register or memory label
    pub next_reg_idx: usize,
    pub in_function: bool,
}

impl TranspilerContext {
    pub fn new(platform: &str) -> Self {
        Self {
            platform: platform.to_string(),
            label_counter: 0,
            asm_output: String::new(),
            vars: HashMap::new(),
            next_reg_idx: 0,
            in_function: false,
        }
    }

    fn new_label(&mut self, prefix: &str) -> String {
        self.label_counter += 1;
        format!(".{}_{}", prefix, self.label_counter)
    }

    fn emit(&mut self, line: &str) {
        self.asm_output.push_str(line);
        self.asm_output.push('\n');
    }

    fn emit_instr(&mut self, mnem: &str, ops: &str) {
        self.asm_output
            .push_str(&format!("    {:<8} {}\n", mnem, ops));
    }
}

pub fn transpile_python_to_m68k(
    source: &str,
    platform: &str,
    target_cpu: &str,
) -> Result<String, String> {
    let tokens = tokenize(source)?;
    let mut ctx = TranspilerContext::new(platform);

    // Header & Platform Startup boilerplate
    ctx.emit("; =============================================================================");
    ctx.emit(&format!(
        "; Generated from Python source by m68k Studio (Target: {}, CPU: {})",
        platform.to_uppercase(),
        target_cpu
    ));
    ctx.emit("; =============================================================================");

    if platform == "megadrive" {
        ctx.emit("    ORG $000000");
        ctx.emit("    dc.l    $00FF0000           ; Initial SP");
        ctx.emit("    dc.l    _start              ; Entry Point");
        ctx.emit("    dcb.l   62, DefaultHandler  ; Vectors");
        ctx.emit("    dc.b    \"SEGA GENESIS    \"");
        ctx.emit("    dc.b    \"(C)2026 PYTHON  \"");
        ctx.emit("    dc.b    \"M68K PYTHON DEMO ROM                                   \"");
        ctx.emit("    dc.b    \"M68K PYTHON DEMO ROM                                   \"");
        ctx.emit("    dc.b    \"GM 00000000-00\"");
        ctx.emit("    dc.w    $0000");
        ctx.emit("    dc.b    \"J6              \"");
        ctx.emit("    dc.l    $00000000, $0007FFFF, $00FF0000, $00FFFFFF");
        ctx.emit("");
        ctx.emit("_start:");
        ctx.emit("    move.b  $A10001,d0");
        ctx.emit("    andi.b  #$0F,d0");
        ctx.emit("    beq.s   .skip_tmss");
        ctx.emit("    move.l  #\"SEGA\",$A14000");
        ctx.emit(".skip_tmss:");
    } else if platform == "amiga" {
        ctx.emit("    SECTION Code,CODE");
        ctx.emit("CUSTOM_BASE EQU $DFF000");
        ctx.emit("");
        ctx.emit("_start:");
        ctx.emit("    move.l  $4.w,a6");
        ctx.emit("    suba.l  a1,a1");
        ctx.emit("    jsr     -$126(a6)           ; FindTask(NULL)");
        ctx.emit("    lea     CUSTOM_BASE,a6");
    } else {
        ctx.emit("    ORG $000000");
        ctx.emit("    dc.l    $00010000, _start");
        ctx.emit("    dcb.l   62, DefaultHandler");
        ctx.emit("");
        ctx.emit("_start:");
    }

    ctx.emit("    jsr     main");
    if platform == "amiga" {
        ctx.emit("    moveq   #0,d0");
        ctx.emit("    rts");
    } else {
        ctx.emit(".halt:");
        ctx.emit("    stop    #$2700");
        ctx.emit("    bra.s   .halt");
    }
    ctx.emit("");

    // Parse tokens and generate assembly
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            PyToken::From | PyToken::Import => {
                // Skip import statements (from m68k.amiga import ...)
                while i < tokens.len() && tokens[i] != PyToken::Newline {
                    i += 1;
                }
            }
            PyToken::Def => {
                // def function_name():
                i += 1;
                if let Some(PyToken::Ident(name)) = tokens.get(i) {
                    let fn_name = name.clone();
                    i += 1; // skip ident
                    if tokens.get(i) == Some(&PyToken::LParen) {
                        i += 1; // skip (
                        // parse parameters if any
                        while i < tokens.len() && tokens[i] != PyToken::RParen {
                            i += 1;
                        }
                        if tokens.get(i) == Some(&PyToken::RParen) {
                            i += 1;
                        }
                    }
                    if tokens.get(i) == Some(&PyToken::Colon) {
                        i += 1;
                    }

                    ctx.emit(&format!("{}:", fn_name));
                    ctx.in_function = true;
                    ctx.next_reg_idx = 0;
                    ctx.vars.clear();
                }
            }
            PyToken::Ident(name) => {
                let id = name.clone();
                if i + 1 < tokens.len() && tokens[i + 1] == PyToken::Dot {
                    // Object/Hardware member write: e.g. custom.color00 = 0x0F80 or vdp.color = ...
                    i += 2;
                    if let Some(PyToken::Ident(member)) = tokens.get(i) {
                        let mem = member.clone();
                        i += 1;
                        if tokens.get(i) == Some(&PyToken::Assign) {
                            i += 1;
                            let val_str = parse_val_to_asm_literal(&tokens, &mut i);
                            emit_hardware_write(&mut ctx, &id, &mem, &val_str);
                        }
                    }
                } else if i + 1 < tokens.len() && tokens[i + 1] == PyToken::LParen {
                    // Function call: e.g. wait_line(128) or wait_vbl() or poke_w(addr, val)
                    i += 2; // skip name and (
                    let mut args = Vec::new();
                    while i < tokens.len() && tokens[i] != PyToken::RParen {
                        if tokens[i] != PyToken::Comma {
                            let arg = parse_val_to_asm_literal(&tokens, &mut i);
                            args.push(arg);
                        } else {
                            i += 1;
                        }
                    }
                    if tokens.get(i) == Some(&PyToken::RParen) {
                        i += 1;
                    }
                    emit_builtin_call(&mut ctx, &id, &args);
                } else if i + 1 < tokens.len() && tokens[i + 1] == PyToken::Assign {
                    // Variable assignment: x = 10 or x = x + 1
                    i += 2;
                    let val_str = parse_val_to_asm_literal(&tokens, &mut i);
                    let reg = if let Some(r) = ctx.vars.get(&id) {
                        r.clone()
                    } else {
                        let r = format!("d{}", (ctx.next_reg_idx % 7));
                        ctx.next_reg_idx += 1;
                        ctx.vars.insert(id.clone(), r.clone());
                        r
                    };
                    ctx.emit_instr("move.l", &format!("{},{}", val_str, reg));
                }
            }
            PyToken::While => {
                // while True: or while condition:
                i += 1;
                let loop_label = ctx.new_label("while_start");
                let _end_label = ctx.new_label("while_end");
                ctx.emit(&format!("{}:", loop_label));

                if tokens.get(i) == Some(&PyToken::True) {
                    i += 1;
                    if tokens.get(i) == Some(&PyToken::Colon) {
                        i += 1;
                    }
                }

                // Inner loop lines will be emitted until Dedent
                // At loop end we emit: bra.s loop_label
            }
            PyToken::For => {
                // for i in range(start, end):
                i += 1;
                if let Some(PyToken::Ident(var_name)) = tokens.get(i) {
                    let var = var_name.clone();
                    i += 1;
                    if tokens.get(i) == Some(&PyToken::In) {
                        i += 1;
                        if tokens.get(i) == Some(&PyToken::Range) {
                            i += 1;
                            if tokens.get(i) == Some(&PyToken::LParen) {
                                i += 1;
                                let start_val;
                                let end_val;

                                let val1 = parse_val_to_asm_literal(&tokens, &mut i);
                                if tokens.get(i) == Some(&PyToken::Comma) {
                                    i += 1;
                                    let val2 = parse_val_to_asm_literal(&tokens, &mut i);
                                    start_val = val1;
                                    end_val = val2;
                                } else {
                                    start_val = "#0".to_string();
                                    end_val = val1;
                                }

                                if tokens.get(i) == Some(&PyToken::RParen) {
                                    i += 1;
                                }
                                if tokens.get(i) == Some(&PyToken::Colon) {
                                    i += 1;
                                }

                                let reg = format!("d{}", (ctx.next_reg_idx % 7));
                                ctx.next_reg_idx += 1;
                                ctx.vars.insert(var.clone(), reg.clone());

                                let loop_start = ctx.new_label("for_loop");
                                let loop_end = ctx.new_label("for_end");

                                ctx.emit_instr("move.l", &format!("{},{}", start_val, reg));
                                ctx.emit(&format!("{}:", loop_start));
                                ctx.emit_instr("cmpi.l", &format!("{},{}", end_val, reg));
                                ctx.emit_instr("bge", &loop_end);
                            }
                        }
                    }
                }
            }
            PyToken::Return => {
                i += 1;
                if tokens.get(i) != Some(&PyToken::Newline) && i < tokens.len() {
                    let val = parse_val_to_asm_literal(&tokens, &mut i);
                    ctx.emit_instr("move.l", &format!("{},d0", val));
                }
                ctx.emit_instr("rts", "");
            }
            PyToken::Dedent if ctx.in_function => {
                ctx.emit_instr("rts", "");
                ctx.in_function = false;
            }
            _ => {}
        }
        i += 1;
    }

    if ctx.in_function {
        ctx.emit_instr("rts", "");
    }

    ctx.emit("");
    ctx.emit("DefaultHandler:");
    ctx.emit("    rte");
    ctx.emit("");

    Ok(ctx.asm_output)
}

fn parse_val_to_asm_literal(tokens: &[PyToken], idx: &mut usize) -> String {
    if *idx >= tokens.len() {
        return "#0".to_string();
    }
    match &tokens[*idx] {
        PyToken::HexNumber(val) => {
            *idx += 1;
            format!("#${:X}", val)
        }
        PyToken::Number(val) => {
            *idx += 1;
            format!("#{}", val)
        }
        PyToken::Ident(name) => {
            let n = name.clone();
            *idx += 1;
            n
        }
        PyToken::Str(s) => {
            let string_val = s.clone();
            *idx += 1;
            format!("#\"{}\"", string_val)
        }
        _ => {
            *idx += 1;
            "#0".to_string()
        }
    }
}

fn emit_hardware_write(ctx: &mut TranspilerContext, obj: &str, member: &str, val_str: &str) {
    if obj == "custom" || obj == "amiga" {
        let reg_offset = match member {
            "color00" | "color_00" => "$180",
            "color01" | "color_01" => "$182",
            "color02" | "color_02" => "$184",
            "color03" | "color_03" => "$186",
            "dmacon" => "$096",
            "intena" => "$09A",
            "bplcon0" => "$100",
            "cop1lch" => "$080",
            "copjmp1" => "$088",
            "bltcon0" => "$040",
            "bltcon1" => "$042",
            "bltsize" => "$058",
            _ => "$180",
        };
        ctx.emit_instr("move.w", &format!("{},{}(a6)", val_str, reg_offset));
    } else if obj == "vdp" {
        // Sega Mega Drive VDP
        match member {
            "control" => ctx.emit_instr("move.w", &format!("{},$C00004", val_str)),
            "data" => ctx.emit_instr("move.w", &format!("{},$C00000", val_str)),
            "color" | "backdrop" => {
                ctx.emit_instr("move.w", &format!("{},$C00004", val_str));
            }
            _ => ctx.emit_instr("move.w", &format!("{},$C00004", val_str)),
        }
    }
}

fn emit_builtin_call(ctx: &mut TranspilerContext, name: &str, args: &[String]) {
    match name {
        "wait_vbl" => {
            let label = ctx.new_label("vbl_wait");
            ctx.emit(&format!("{}:", label));
            ctx.emit_instr("move.l", "$DFF004,d0");
            ctx.emit_instr("andi.l", "#$1FF00,d0");
            ctx.emit_instr("cmpi.l", "#$13000,d0");
            ctx.emit_instr("bne.s", &label);
        }
        "wait_line" => {
            let line_val = args.first().cloned().unwrap_or_else(|| "#128".to_string());
            let label = ctx.new_label("raster_wait");
            ctx.emit(&format!("{}:", label));
            ctx.emit_instr("move.l", "$DFF004,d0");
            ctx.emit_instr("lsr.l", "#8,d0");
            ctx.emit_instr("andi.w", "#$1FF,d0");
            ctx.emit_instr("cmpi.w", &format!("{},d0", line_val));
            ctx.emit_instr("bne.s", &label);
        }
        "poke_w" => {
            if args.len() >= 2 {
                ctx.emit_instr("move.w", &format!("{},({})", args[1], args[0]));
            }
        }
        "poke_l" => {
            if args.len() >= 2 {
                ctx.emit_instr("move.l", &format!("{},({})", args[1], args[0]));
            }
        }
        "asm" => {
            if let Some(code) = args.first() {
                let unquoted = code.trim_matches('#').trim_matches('"');
                ctx.emit(&format!("    {}", unquoted));
            }
        }
        _ => {
            ctx.emit_instr("jsr", name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transpile_simple_python_function() {
        let py = r#"
def main():
    x = 10
    y = 20
    custom.color00 = 0x0F80
    wait_line(128)
    return x
"#;
        let asm = transpile_python_to_m68k(py, "amiga", "68000").unwrap();
        assert!(asm.contains("main:"));
        assert!(asm.contains("move.w   #$F80,$180(a6)"));
        assert!(asm.contains("rts"));
    }

    #[test]
    fn test_transpile_megadrive_python() {
        let py = r#"
def main():
    vdp.control = 0x8004
    vdp.backdrop = 0x8700
"#;
        let asm = transpile_python_to_m68k(py, "megadrive", "68000").unwrap();
        assert!(asm.contains("SEGA GENESIS"));
        assert!(asm.contains("move.w   #$8004,$C00004"));
    }
}
