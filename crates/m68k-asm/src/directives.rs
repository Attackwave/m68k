//! Assembler directives: EQU, LABEL, ALIGN, EVEN, INCLUDE, INCBIN, SECTION.
//!
//! Directives are handled during both passes:
//! - **Pass 1**: Define symbols (EQU), calculate sizes (ALIGN, INCBIN), track sections
//! - **Pass 2**: Generate padding (ALIGN, EVEN), embed binary data (INCBIN)

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use m68k_core::errors::AsmError;

use crate::assembler::{AssembledInstruction, SymbolTable};

// ---------------------------------------------------------------------------
// Section types
// ---------------------------------------------------------------------------

/// Named section for code/data organization.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// Code section (readable, executable).
    Text,
    /// Initialized data section.
    Data,
    /// Uninitialized data section (BSS).
    Bss,
    /// Custom named section.
    Named(String),
}

impl SectionKind {
    pub fn from_name(name: &str) -> Self {
        match name {
            "text" | "TEXT" | "code" | "CODE" => SectionKind::Text,
            "data" | "DATA" => SectionKind::Data,
            "bss" | "BSS" => SectionKind::Bss,
            _ => SectionKind::Named(name.to_string()),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            SectionKind::Text => "text",
            SectionKind::Data => "data",
            SectionKind::Bss => "bss",
            SectionKind::Named(n) => n,
        }
    }
}

/// A section with its own location counter and assembled instructions.
#[derive(Debug, Clone)]
pub struct Section {
    pub kind: SectionKind,
    pub origin: u32,
    pub pc: u32,
    pub instructions: Vec<AssembledInstruction>,
}

impl Section {
    pub fn new(kind: SectionKind, origin: u32) -> Self {
        Self {
            kind,
            origin,
            pc: origin,
            instructions: Vec::new(),
        }
    }

    pub fn size_bytes(&self) -> usize {
        self.instructions.iter().map(|i| i.size_bytes()).sum()
    }

    /// Base address of this section's actual content: the lowest `pc` among
    /// its instructions. Distinct from `origin`, which only reflects an
    /// explicit `SECTION name,origin` argument and is left stale (at its
    /// `SectionManager::new`/`switch_section` default) if the source instead
    /// used `ORG` to relocate the section's location counter.
    pub fn base_addr(&self) -> u32 {
        self.instructions
            .iter()
            .map(|i| i.pc)
            .min()
            .unwrap_or(self.origin)
    }

    /// Render this section's instructions to bytes, laid out relative to
    /// [`Self::base_addr`] with any gaps (e.g. from `DS`, which advances the
    /// location counter without emitting an instruction) filled with zero
    /// bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        if self.instructions.is_empty() {
            return Vec::new();
        }
        let base_addr = self.base_addr();
        let last_addr = self
            .instructions
            .iter()
            .map(|i| i.pc + i.size_bytes() as u32)
            .max()
            .unwrap_or(base_addr);
        let mut bytes = vec![0u8; (last_addr - base_addr) as usize];
        for instr in &self.instructions {
            let mut pos = (instr.pc - base_addr) as usize;
            for word in &instr.words {
                bytes[pos] = (word >> 8) as u8;
                bytes[pos + 1] = (word & 0xFF) as u8;
                pos += 2;
            }
        }
        bytes
    }
}

// ---------------------------------------------------------------------------
// Directive processing result
// ---------------------------------------------------------------------------

/// Result of processing a directive in pass 1 or pass 2.
#[derive(Debug)]
pub struct DirectiveResult {
    /// Bytes to emit (0 for directives like EQU that produce no code).
    pub bytes_emitted: u32,
    /// Whether this directive changes the current PC (e.g., ORG, SECTION).
    pub pc_changed: bool,
    /// New PC value if `pc_changed` is true.
    pub new_pc: Option<u32>,
}

impl DirectiveResult {
    pub fn none() -> Self {
        Self {
            bytes_emitted: 0,
            pc_changed: false,
            new_pc: None,
        }
    }

    pub fn with_bytes(bytes: u32) -> Self {
        Self {
            bytes_emitted: bytes,
            pc_changed: false,
            new_pc: None,
        }
    }

    pub fn with_pc(pc: u32) -> Self {
        Self {
            bytes_emitted: 0,
            pc_changed: true,
            new_pc: Some(pc),
        }
    }
}

// ---------------------------------------------------------------------------
// Section manager
// ---------------------------------------------------------------------------

/// Manages multiple sections with independent location counters.
#[derive(Debug, Default)]
pub struct SectionManager {
    sections: HashMap<SectionKind, Section>,
    current_section: Option<SectionKind>,
    default_origin: u32,
}

impl SectionManager {
    pub fn new(default_origin: u32) -> Self {
        let mut mgr = Self {
            sections: HashMap::new(),
            current_section: None,
            default_origin,
        };
        // Create default text section
        mgr.sections.insert(
            SectionKind::Text,
            Section::new(SectionKind::Text, default_origin),
        );
        mgr.current_section = Some(SectionKind::Text);
        mgr
    }

    pub fn current_section(&self) -> Option<&Section> {
        self.current_section
            .as_ref()
            .and_then(|k| self.sections.get(k))
    }

    pub fn current_section_mut(&mut self) -> Option<&mut Section> {
        let key = self.current_section.clone();
        key.and_then(|k| self.sections.get_mut(&k))
    }

    pub fn current_pc(&self) -> u32 {
        self.current_section()
            .map(|s| s.pc)
            .unwrap_or(self.default_origin)
    }

    pub fn set_current_pc(&mut self, pc: u32) {
        if let Some(section) = self.current_section_mut() {
            section.pc = pc;
        }
    }

    pub fn switch_section(&mut self, kind: SectionKind) {
        if !self.sections.contains_key(&kind) {
            self.sections.insert(
                kind.clone(),
                Section::new(kind.clone(), self.default_origin),
            );
        }
        self.current_section = Some(kind);
    }

    pub fn add_instruction(&mut self, instr: AssembledInstruction) {
        if let Some(section) = self.current_section_mut() {
            section.instructions.push(instr);
        }
    }

    pub fn get_section(&self, kind: &SectionKind) -> Option<&Section> {
        self.sections.get(kind)
    }

    pub fn iter_sections(&self) -> impl Iterator<Item = (&SectionKind, &Section)> {
        self.sections.iter()
    }
}

// ---------------------------------------------------------------------------
// File inclusion tracking
// ---------------------------------------------------------------------------

/// Tracks included files to prevent infinite recursion.
#[derive(Debug, Default)]
pub struct IncludeStack {
    files: Vec<PathBuf>,
}

impl IncludeStack {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn push(&mut self, path: PathBuf) -> Result<(), AsmError> {
        if self.files.contains(&path) {
            return Err(AsmError::new(format!(
                "circular INCLUDE detected: {}",
                path.display()
            )));
        }
        self.files.push(path);
        Ok(())
    }

    pub fn pop(&mut self) {
        self.files.pop();
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.files.iter().any(|p| p == path)
    }
}

// ---------------------------------------------------------------------------
// Directive handlers
// ---------------------------------------------------------------------------

/// Handle EQU directive in pass 1.
///
/// Syntax: `LABEL EQU expression`
/// Defines `LABEL` with the value of `expression`. Produces no code.
pub fn handle_equ(
    label: &Option<String>,
    args: &[String],
    symbols: &mut SymbolTable,
    _pc: u32,
    line_no: usize,
) -> Result<DirectiveResult, AsmError> {
    if args.is_empty() {
        return Err(AsmError::with_line("EQU requires a value", line_no));
    }

    let value = parse_simple_expr(&args[0], symbols, _pc)
        .map_err(|e| AsmError::with_line(format!("invalid EQU expression: {}", e), line_no))?;

    let name = label.as_ref().ok_or_else(|| {
        AsmError::with_line("EQU requires a label (e.g., 'MYCONST EQU $10')", line_no)
    })?;

    symbols
        .define(name, value as u32, Some(line_no))
        .map_err(|e| AsmError::with_line(format!("EQU: {}", e.message), line_no))?;

    Ok(DirectiveResult::none())
}

/// Handle SET directive (like EQU but allows redefinition).
///
/// Syntax: `LABEL SET expression`
pub fn handle_set(
    label: &Option<String>,
    args: &[String],
    symbols: &mut SymbolTable,
    pc: u32,
    line_no: usize,
) -> Result<DirectiveResult, AsmError> {
    if args.is_empty() {
        return Err(AsmError::with_line("SET requires a value", line_no));
    }

    let value = parse_simple_expr(&args[0], symbols, pc)
        .map_err(|e| AsmError::with_line(format!("invalid SET expression: {}", e), line_no))?;

    let name = label
        .as_ref()
        .ok_or_else(|| AsmError::with_line("SET requires a label", line_no))?;

    // SET allows redefinition
    symbols.force_set(name, value as u32, Some(line_no));

    Ok(DirectiveResult::none())
}

/// Handle ALIGN directive in pass 1 (size estimation) and pass 2 (code generation).
///
/// Syntax: `ALIGN alignment[, fill]`
/// Aligns the location counter to `alignment` bytes. Optional `fill` value for padding.
pub fn handle_align_pass1(
    args: &[String],
    symbols: &SymbolTable,
    pc: u32,
    line_no: usize,
) -> Result<DirectiveResult, AsmError> {
    if args.is_empty() {
        return Err(AsmError::with_line(
            "ALIGN requires alignment value",
            line_no,
        ));
    }

    let alignment = parse_simple_expr(&args[0], symbols, pc)
        .map_err(|e| AsmError::with_line(format!("invalid ALIGN expression: {}", e), line_no))?
        as u32;

    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(AsmError::with_line(
            format!("ALIGN requires a power-of-2 value, got {}", alignment),
            line_no,
        ));
    }

    let padding = if pc.is_multiple_of(alignment) {
        0
    } else {
        alignment - (pc % alignment)
    };

    // Round up to even if needed
    let bytes = if padding % 2 != 0 {
        padding + 1
    } else {
        padding
    };

    Ok(DirectiveResult::with_bytes(bytes))
}

/// Handle ALIGN in pass 2 - generate padding words.
pub fn handle_align_pass2(
    args: &[String],
    symbols: &SymbolTable,
    pc: u32,
    line_no: usize,
    source: &str,
) -> Result<(DirectiveResult, Option<AssembledInstruction>), AsmError> {
    let result = handle_align_pass1(args, symbols, pc, line_no)?;

    if result.bytes_emitted == 0 {
        return Ok((result, None));
    }

    let fill_value = if args.len() > 1 {
        parse_simple_expr(&args[1], symbols, pc)
            .map_err(|e| AsmError::with_line(format!("invalid ALIGN fill value: {}", e), line_no))?
            as u16
    } else {
        0x4E71 // NOP as default fill
    };

    let word_count = (result.bytes_emitted / 2) as usize;
    let words = vec![fill_value; word_count];

    let instr = AssembledInstruction {
        pc,
        words,
        line_no: Some(line_no),
        source: Some(source.to_string()),
    };

    Ok((result, Some(instr)))
}

/// Handle EVEN directive - pad to word boundary if needed.
pub fn handle_even_pass1(pc: u32) -> DirectiveResult {
    if !pc.is_multiple_of(2) {
        DirectiveResult::with_bytes(2)
    } else {
        DirectiveResult::none()
    }
}

/// Handle EVEN in pass 2 - generate padding if needed.
pub fn handle_even_pass2(
    pc: u32,
    line_no: usize,
    source: &str,
) -> (DirectiveResult, Option<AssembledInstruction>) {
    if !pc.is_multiple_of(2) {
        let instr = AssembledInstruction {
            pc,
            words: vec![0x4E71], // NOP padding
            line_no: Some(line_no),
            source: Some(source.to_string()),
        };
        (DirectiveResult::with_bytes(2), Some(instr))
    } else {
        (DirectiveResult::none(), None)
    }
}

/// Handle SECTION directive.
///
/// Syntax: `SECTION name` or `SECTION name,origin`
/// Switches to the named section.
pub fn handle_section(
    args: &[String],
    symbols: &SymbolTable,
    pc: u32,
    sections: &mut SectionManager,
    line_no: usize,
) -> Result<DirectiveResult, AsmError> {
    if args.is_empty() {
        return Err(AsmError::with_line("SECTION requires a name", line_no));
    }

    let section_name = args[0].trim();
    let kind = SectionKind::from_name(section_name);

    // Optional origin argument
    if args.len() > 1 {
        let origin = parse_simple_expr(&args[1], symbols, pc)
            .map_err(|e| AsmError::with_line(format!("invalid SECTION origin: {}", e), line_no))?
            as u32;

        // If section doesn't exist, create with this origin
        if sections.get_section(&kind).is_none() {
            sections
                .sections
                .insert(kind.clone(), Section::new(kind.clone(), origin));
        } else if let Some(section) = sections.sections.get_mut(&kind) {
            section.origin = origin;
            if section.instructions.is_empty() {
                section.pc = origin;
            }
        }
    }

    sections.switch_section(kind);

    Ok(DirectiveResult::with_pc(sections.current_pc()))
}

/// Handle ORG directive.
pub fn handle_org(
    args: &[String],
    symbols: &SymbolTable,
    _pc: u32,
    line_no: usize,
) -> Result<DirectiveResult, AsmError> {
    if args.is_empty() {
        return Err(AsmError::with_line("ORG requires an address", line_no));
    }

    let addr = parse_simple_expr(&args[0], symbols, _pc)
        .map_err(|e| AsmError::with_line(format!("invalid ORG address: {}", e), line_no))?
        as u32;

    Ok(DirectiveResult::with_pc(addr))
}

/// Handle INCBIN directive in pass 1.
///
/// Syntax: `INCBIN "filename"[, offset[, length]]`
/// Embeds binary file data.
pub fn handle_incbin_pass1(
    args: &[String],
    symbols: &SymbolTable,
    _pc: u32,
    source_root: &Path,
    line_no: usize,
) -> Result<DirectiveResult, AsmError> {
    if args.is_empty() {
        return Err(AsmError::with_line("INCBIN requires a filename", line_no));
    }

    let filename = strip_quotes(&args[0]);
    let path = resolve_include_path(filename, source_root)?;

    let data = fs::read(&path).map_err(|e| {
        AsmError::with_line(
            format!("cannot read file '{}': {}", path.display(), e),
            line_no,
        )
    })?;

    let offset = if args.len() > 1 {
        parse_simple_expr(&args[1], symbols, _pc)
            .map_err(|e| AsmError::with_line(format!("invalid INCBIN offset: {}", e), line_no))?
            as usize
    } else {
        0
    };

    let length = if args.len() > 2 {
        parse_simple_expr(&args[2], symbols, _pc)
            .map_err(|e| AsmError::with_line(format!("invalid INCBIN length: {}", e), line_no))?
            as usize
    } else {
        data.len().saturating_sub(offset)
    };

    let actual_len = length.min(data.len().saturating_sub(offset));
    // Round up to even
    let bytes = ((actual_len + 1) & !1) as u32;

    Ok(DirectiveResult::with_bytes(bytes))
}

/// Handle INCBIN in pass 2 - emit the actual binary data.
pub fn handle_incbin_pass2(
    args: &[String],
    symbols: &SymbolTable,
    pc: u32,
    source_root: &Path,
    line_no: usize,
    source: &str,
) -> Result<(DirectiveResult, Option<AssembledInstruction>), AsmError> {
    if args.is_empty() {
        return Err(AsmError::with_line("INCBIN requires a filename", line_no));
    }

    let filename = strip_quotes(&args[0]);
    let path = resolve_include_path(filename, source_root)?;

    let data = fs::read(&path).map_err(|e| {
        AsmError::with_line(
            format!("cannot read file '{}': {}", path.display(), e),
            line_no,
        )
    })?;

    let offset = if args.len() > 1 {
        parse_simple_expr(&args[1], symbols, pc)
            .map_err(|e| AsmError::with_line(format!("invalid INCBIN offset: {}", e), line_no))?
            as usize
    } else {
        0
    };

    let length = if args.len() > 2 {
        parse_simple_expr(&args[2], symbols, pc)
            .map_err(|e| AsmError::with_line(format!("invalid INCBIN length: {}", e), line_no))?
            as usize
    } else {
        data.len().saturating_sub(offset)
    };

    let actual_len = length.min(data.len().saturating_sub(offset));
    let sliced = &data[offset..offset + actual_len];

    // Convert bytes to words (big-endian), padding with 0 if odd
    let mut words = Vec::new();
    let mut i = 0;
    while i < sliced.len() {
        let hi = sliced[i] as u16;
        let lo = if i + 1 < sliced.len() {
            sliced[i + 1] as u16
        } else {
            0x00
        };
        words.push((hi << 8) | lo);
        i += 2;
    }

    let bytes = ((sliced.len() + 1) & !1) as u32;

    let instr = AssembledInstruction {
        pc,
        words,
        line_no: Some(line_no),
        source: Some(source.to_string()),
    };

    Ok((DirectiveResult::with_bytes(bytes), Some(instr)))
}

/// Handle INCLUDE directive - returns the included source text.
///
/// Syntax: `INCLUDE "filename"`
/// Recursively includes another source file.
pub fn handle_include(
    args: &[String],
    include_stack: &mut IncludeStack,
    source_root: &Path,
    line_no: usize,
) -> Result<String, AsmError> {
    if args.is_empty() {
        return Err(AsmError::with_line("INCLUDE requires a filename", line_no));
    }

    let filename = strip_quotes(&args[0]);
    let path = resolve_include_path(filename, source_root)?;

    if include_stack.contains(&path) {
        return Err(AsmError::with_line(
            format!("circular INCLUDE: {}", path.display()),
            line_no,
        ));
    }

    let content = fs::read_to_string(&path).map_err(|e| {
        AsmError::with_line(
            format!("cannot read file '{}': {}", path.display(), e),
            line_no,
        )
    })?;

    include_stack.push(path)?;
    Ok(content)
}

// ---------------------------------------------------------------------------
// Expression evaluation (simple, for directives)
// ---------------------------------------------------------------------------

/// Parse a simple expression string into an i64 value.
///
/// Supports: $hex, %binary, decimal, symbols, basic operators (+, -, *, /, &, |, ^, <<, >>),
/// unary minus, HIGH()/LOW() functions, and `$` for current PC.
pub fn parse_simple_expr(text: &str, symbols: &SymbolTable, pc: u32) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty expression".to_string());
    }

    // Tokenize the expression
    let tokens = tokenize_expr(text, pc)?;
    let mut pos = 0;
    parse_expr_or(&tokens, &mut pos, symbols)
}

#[derive(Debug, Clone)]
enum ExprToken {
    Num(i64),
    Ident(String),
    Op(String),
    LParen,
    RParen,
    Comma,
}

fn tokenize_expr(text: &str, pc: u32) -> Result<Vec<ExprToken>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        // Skip whitespace
        if ch.is_whitespace() {
            i += 1;
            continue;
        }

        // String literal - shouldn't appear in numeric expressions but handle gracefully
        if ch == '"' || ch == '\'' {
            let quote = ch;
            i += 1;
            let mut s = String::new();
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    match chars[i] {
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        '0' => s.push('\0'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        '\'' => s.push('\''),
                        _ => {
                            s.push('\\');
                            s.push(chars[i]);
                        }
                    }
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i < chars.len() {
                i += 1; // skip closing quote
            }
            // Return first character as number for DC.B "string" handling
            if !s.is_empty() {
                tokens.push(ExprToken::Num(s.as_bytes()[0] as i64));
            }
            continue;
        }

        // Hex: $FF or standalone $ (current PC)
        if ch == '$' {
            if i + 1 < chars.len() && chars[i + 1].is_ascii_hexdigit() {
                let start = i + 1;
                i += 1;
                while i < chars.len() && chars[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let hex_str: String = chars[start..i].iter().collect();
                let value = i64::from_str_radix(&hex_str, 16)
                    .map_err(|_| format!("invalid hex: ${}", hex_str))?;
                tokens.push(ExprToken::Num(value));
            } else {
                // Standalone $ = current PC
                tokens.push(ExprToken::Num(pc as i64));
                i += 1;
            }
            continue;
        }

        // Hex: 0xFF
        if ch == '0' && i + 1 < chars.len() && chars[i + 1] == 'x' {
            let start = i + 2;
            i += 2;
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            let hex_str: String = chars[start..i].iter().collect();
            let value = i64::from_str_radix(&hex_str, 16)
                .map_err(|_| format!("invalid hex: 0x{}", hex_str))?;
            tokens.push(ExprToken::Num(value));
            continue;
        }

        // Binary: %1010
        if ch == '%' && i + 1 < chars.len() && (chars[i + 1] == '0' || chars[i + 1] == '1') {
            let start = i + 1;
            i += 1;
            while i < chars.len() && (chars[i] == '0' || chars[i] == '1') {
                i += 1;
            }
            let bin_str: String = chars[start..i].iter().collect();
            let value = i64::from_str_radix(&bin_str, 2)
                .map_err(|_| format!("invalid binary: %{}", bin_str))?;
            tokens.push(ExprToken::Num(value));
            continue;
        }

        // Number
        if ch.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            let value: i64 = num_str
                .parse()
                .map_err(|_| format!("invalid number: {}", num_str))?;
            tokens.push(ExprToken::Num(value));
            continue;
        }

        // Identifier or keyword
        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric()
                    || chars[i] == '_'
                    || chars[i] == '$'
                    || chars[i] == '.')
            {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            tokens.push(ExprToken::Ident(ident));
            continue;
        }

        // Multi-char operators
        if ch == '<' && i + 1 < chars.len() && chars[i + 1] == '<' {
            tokens.push(ExprToken::Op("<<".to_string()));
            i += 2;
            continue;
        }
        if ch == '>' && i + 1 < chars.len() && chars[i + 1] == '>' {
            tokens.push(ExprToken::Op(">>".to_string()));
            i += 2;
            continue;
        }
        if ch == '<' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push(ExprToken::Op("<=".to_string()));
            i += 2;
            continue;
        }
        if ch == '>' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push(ExprToken::Op(">=".to_string()));
            i += 2;
            continue;
        }
        if ch == '=' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push(ExprToken::Op("==".to_string()));
            i += 2;
            continue;
        }
        if ch == '!' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push(ExprToken::Op("!=".to_string()));
            i += 2;
            continue;
        }
        if ch == '&' && i + 1 < chars.len() && chars[i + 1] == '&' {
            tokens.push(ExprToken::Op("&&".to_string()));
            i += 2;
            continue;
        }
        if ch == '|' && i + 1 < chars.len() && chars[i + 1] == '|' {
            tokens.push(ExprToken::Op("||".to_string()));
            i += 2;
            continue;
        }

        // '*' as current PC (location counter) when in primary position, i.e.
        // at the start of the expression or right after '(', ',', or another
        // operator -- otherwise it's the multiplication operator.
        if ch == '*' {
            let in_operand_position = matches!(
                tokens.last(),
                Some(ExprToken::Num(_)) | Some(ExprToken::Ident(_)) | Some(ExprToken::RParen)
            );
            if !in_operand_position {
                tokens.push(ExprToken::Num(pc as i64));
                i += 1;
                continue;
            }
        }

        // Single-char operators (exclude ( ) which are handled separately)
        if "+-*/&|^~=<>!".contains(ch) {
            tokens.push(ExprToken::Op(ch.to_string()));
            i += 1;
            continue;
        }

        // Parentheses
        if ch == '(' {
            tokens.push(ExprToken::LParen);
            i += 1;
            continue;
        }
        if ch == ')' {
            tokens.push(ExprToken::RParen);
            i += 1;
            continue;
        }

        // Comma
        if ch == ',' {
            tokens.push(ExprToken::Comma);
            i += 1;
            continue;
        }

        return Err(format!("unexpected character: '{}'", ch));
    }

    Ok(tokens)
}

/// Recursive descent expression parser with proper precedence.
///
/// Precedence (lowest to highest):
/// ||, &&
/// |
/// ^
/// &
/// ==, !=, <, <=, >, >=
/// <<, >>
/// +, -
/// *, /, %
/// unary -, ~, !
/// HIGH(), LOW()
/// primary (numbers, identifiers, parenthesized expressions)
fn parse_expr_or(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    let mut left = parse_expr_and(tokens, pos, symbols)?;

    loop {
        if *pos < tokens.len()
            && let ExprToken::Op(op) = &tokens[*pos]
            && op == "||"
        {
            *pos += 1;
            let right = parse_expr_and(tokens, pos, symbols)?;
            left = if left != 0 || right != 0 { 1 } else { 0 };
            continue;
        }
        break;
    }

    Ok(left)
}

fn parse_expr_and(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    let mut left = parse_expr_bit_or(tokens, pos, symbols)?;

    loop {
        if *pos < tokens.len()
            && let ExprToken::Op(op) = &tokens[*pos]
            && op == "&&"
        {
            *pos += 1;
            let right = parse_expr_bit_or(tokens, pos, symbols)?;
            left = if left != 0 && right != 0 { 1 } else { 0 };
            continue;
        }
        break;
    }

    Ok(left)
}

fn parse_expr_bit_or(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    let mut left = parse_expr_xor(tokens, pos, symbols)?;

    loop {
        if *pos < tokens.len()
            && let ExprToken::Op(op) = &tokens[*pos]
            && op == "|"
        {
            *pos += 1;
            let right = parse_expr_xor(tokens, pos, symbols)?;
            left |= right;
            continue;
        }
        break;
    }

    Ok(left)
}

fn parse_expr_xor(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    let mut left = parse_expr_bit_and(tokens, pos, symbols)?;

    loop {
        if *pos < tokens.len()
            && let ExprToken::Op(op) = &tokens[*pos]
            && op == "^"
        {
            *pos += 1;
            let right = parse_expr_bit_and(tokens, pos, symbols)?;
            left ^= right;
            continue;
        }
        break;
    }

    Ok(left)
}

fn parse_expr_bit_and(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    let mut left = parse_expr_cmp(tokens, pos, symbols)?;

    loop {
        if *pos < tokens.len()
            && let ExprToken::Op(op) = &tokens[*pos]
            && op == "&"
        {
            *pos += 1;
            let right = parse_expr_cmp(tokens, pos, symbols)?;
            left &= right;
            continue;
        }
        break;
    }

    Ok(left)
}

fn parse_expr_cmp(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    let mut left = parse_expr_shift(tokens, pos, symbols)?;

    loop {
        if *pos < tokens.len()
            && let ExprToken::Op(op) = &tokens[*pos]
        {
            match op.as_str() {
                "==" => {
                    *pos += 1;
                    let right = parse_expr_shift(tokens, pos, symbols)?;
                    left = if left == right { 1 } else { 0 };
                    continue;
                }
                "!=" => {
                    *pos += 1;
                    let right = parse_expr_shift(tokens, pos, symbols)?;
                    left = if left != right { 1 } else { 0 };
                    continue;
                }
                "<=" => {
                    *pos += 1;
                    let right = parse_expr_shift(tokens, pos, symbols)?;
                    left = if left <= right { 1 } else { 0 };
                    continue;
                }
                ">=" => {
                    *pos += 1;
                    let right = parse_expr_shift(tokens, pos, symbols)?;
                    left = if left >= right { 1 } else { 0 };
                    continue;
                }
                "<" => {
                    *pos += 1;
                    let right = parse_expr_shift(tokens, pos, symbols)?;
                    left = if left < right { 1 } else { 0 };
                    continue;
                }
                ">" => {
                    *pos += 1;
                    let right = parse_expr_shift(tokens, pos, symbols)?;
                    left = if left > right { 1 } else { 0 };
                    continue;
                }
                _ => {}
            }
        }
        break;
    }

    Ok(left)
}

fn parse_expr_shift(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    let mut left = parse_expr_add(tokens, pos, symbols)?;

    loop {
        if *pos < tokens.len()
            && let ExprToken::Op(op) = &tokens[*pos]
        {
            match op.as_str() {
                "<<" => {
                    *pos += 1;
                    let right = parse_expr_add(tokens, pos, symbols)?;
                    left = left.wrapping_shl(right as u32);
                    continue;
                }
                ">>" => {
                    *pos += 1;
                    let right = parse_expr_add(tokens, pos, symbols)?;
                    left = left.wrapping_shr(right as u32);
                    continue;
                }
                _ => {}
            }
        }
        break;
    }

    Ok(left)
}

fn parse_expr_add(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    let mut left = parse_expr_mul(tokens, pos, symbols)?;

    loop {
        if *pos < tokens.len()
            && let ExprToken::Op(op) = &tokens[*pos]
        {
            match op.as_str() {
                "+" => {
                    *pos += 1;
                    let right = parse_expr_mul(tokens, pos, symbols)?;
                    left = left.wrapping_add(right);
                    continue;
                }
                "-" => {
                    *pos += 1;
                    let right = parse_expr_mul(tokens, pos, symbols)?;
                    left = left.wrapping_sub(right);
                    continue;
                }
                _ => {}
            }
        }
        break;
    }

    Ok(left)
}

fn parse_expr_mul(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    let mut left = parse_expr_unary(tokens, pos, symbols)?;

    loop {
        if *pos < tokens.len()
            && let ExprToken::Op(op) = &tokens[*pos]
        {
            match op.as_str() {
                "*" => {
                    *pos += 1;
                    let right = parse_expr_unary(tokens, pos, symbols)?;
                    left = left.wrapping_mul(right);
                    continue;
                }
                "/" => {
                    *pos += 1;
                    let right = parse_expr_unary(tokens, pos, symbols)?;
                    if right == 0 {
                        return Err("division by zero".to_string());
                    }
                    left = left.wrapping_div(right);
                    continue;
                }
                "%" => {
                    *pos += 1;
                    let right = parse_expr_unary(tokens, pos, symbols)?;
                    if right == 0 {
                        return Err("modulo by zero".to_string());
                    }
                    left = left.wrapping_rem(right);
                    continue;
                }
                _ => {}
            }
        }
        break;
    }

    Ok(left)
}

fn parse_expr_unary(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    if *pos < tokens.len() {
        if let ExprToken::Op(op) = &tokens[*pos] {
            match op.as_str() {
                "-" => {
                    *pos += 1;
                    let val = parse_expr_unary(tokens, pos, symbols)?;
                    return Ok(-val);
                }
                "~" | "!" => {
                    *pos += 1;
                    let val = parse_expr_unary(tokens, pos, symbols)?;
                    return Ok(!val);
                }
                _ => {}
            }
        }

        // HIGH(), LOW() function calls
        if let ExprToken::Ident(name) = &tokens[*pos]
            && (name == "HIGH" || name == "high" || name == "LOW" || name == "low")
        {
            let fname = name.clone();
            *pos += 1;
            // Expect (
            if *pos < tokens.len() && matches!(tokens[*pos], ExprToken::LParen) {
                *pos += 1;
                let val = parse_expr_or(tokens, pos, symbols)?;
                // Expect )
                if *pos < tokens.len() && matches!(tokens[*pos], ExprToken::RParen) {
                    *pos += 1;
                    if fname.to_uppercase() == "HIGH" {
                        return Ok((val >> 8) & 0xFF);
                    } else {
                        return Ok(val & 0xFF);
                    }
                }
            }
            return Err(format!("expected '(' after {}", fname));
        }

        // DEFINED(symbol) - 1 if symbol has a resolved value, 0 otherwise.
        // The argument must be a bare identifier, not a sub-expression, since
        // an undefined symbol would otherwise make evaluation fail.
        if let ExprToken::Ident(name) = &tokens[*pos]
            && (name == "DEFINED" || name == "defined")
        {
            *pos += 1;
            if *pos < tokens.len() && matches!(tokens[*pos], ExprToken::LParen) {
                *pos += 1;
                let sym_name = match tokens.get(*pos) {
                    Some(ExprToken::Ident(sym)) => sym.clone(),
                    _ => return Err("expected symbol name in DEFINED()".to_string()),
                };
                *pos += 1;
                if *pos < tokens.len() && matches!(tokens[*pos], ExprToken::RParen) {
                    *pos += 1;
                    let defined = symbols.get(&sym_name).is_some_and(|e| e.defined);
                    return Ok(if defined { 1 } else { 0 });
                }
                return Err("missing ')' in DEFINED()".to_string());
            }
            return Err("expected '(' after DEFINED".to_string());
        }
    }

    parse_expr_primary(tokens, pos, symbols)
}

fn parse_expr_primary(
    tokens: &[ExprToken],
    pos: &mut usize,
    symbols: &SymbolTable,
) -> Result<i64, String> {
    if *pos >= tokens.len() {
        return Err("unexpected end of expression".to_string());
    }

    match &tokens[*pos] {
        ExprToken::Num(n) => {
            *pos += 1;
            Ok(*n)
        }
        ExprToken::Ident(name) => {
            *pos += 1;
            symbols
                .resolve(name)
                .map(|v| v as i64)
                .map_err(|e| e.message)
        }
        ExprToken::LParen => {
            *pos += 1;
            let val = parse_expr_or(tokens, pos, symbols)?;
            if *pos < tokens.len() && matches!(tokens[*pos], ExprToken::RParen) {
                *pos += 1;
                Ok(val)
            } else {
                Err("missing ')' in expression".to_string())
            }
        }
        _ => Err("unexpected token in expression".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Strip surrounding quotes from a string.
pub fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        if (bytes[0] == b'"' && bytes[s.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[s.len() - 1] == b'\'')
        {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Resolve an include file path relative to the source root.
pub fn resolve_include_path(filename: &str, source_root: &Path) -> Result<PathBuf, AsmError> {
    let path = Path::new(filename);

    // If absolute or starts with ./ or ../
    if path.is_absolute() || filename.starts_with("./") || filename.starts_with("../") {
        return Ok(path.to_path_buf());
    }

    // Relative to source root
    let full = source_root.join(filename);
    if full.exists() {
        return Ok(full);
    }

    // Try as-is (might be in current working directory)
    let cwd_path = PathBuf::from(filename);
    if cwd_path.exists() {
        return Ok(cwd_path);
    }

    Err(AsmError::new(format!(
        "file not found: '{}' (searched in '{}')",
        filename,
        source_root.display()
    )))
}

/// Parse a string literal with escape sequences.
pub fn parse_string_literal(s: &str) -> Result<String, String> {
    let s = strip_quotes(s);
    let mut result = String::new();
    let mut chars = s.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('0') => result.push('\0'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('\'') => result.push('\''),
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
                None => return Err("unexpected end of string after backslash".to_string()),
            }
        } else {
            result.push(ch);
        }
    }

    Ok(result)
}

/// Parse DC string argument into bytes.
pub fn parse_dc_string(s: &str) -> Result<Vec<u8>, String> {
    let parsed = parse_string_literal(s)?;
    Ok(parsed.as_bytes().to_vec())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_quotes() {
        assert_eq!(strip_quotes("\"hello\""), "hello");
        assert_eq!(strip_quotes("'hello'"), "hello");
        assert_eq!(strip_quotes("hello"), "hello");
        assert_eq!(strip_quotes("\"$1234\""), "$1234");
    }

    #[test]
    fn test_parse_string_literal() {
        assert_eq!(parse_string_literal("\"hello\"").unwrap(), "hello");
        assert_eq!(
            parse_string_literal("\"hello\\nworld\"").unwrap(),
            "hello\nworld"
        );
        assert_eq!(parse_string_literal("\"tab\\there\"").unwrap(), "tab\there");
    }

    #[test]
    fn test_parse_dc_string() {
        assert_eq!(parse_dc_string("\"hello\"").unwrap(), b"hello");
        assert_eq!(parse_dc_string("\"A\"").unwrap(), b"A");
    }

    #[test]
    fn test_parse_simple_expr_number() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("42", &symbols, 0).unwrap(), 42);
        assert_eq!(parse_simple_expr("$FF", &symbols, 0).unwrap(), 255);
        assert_eq!(parse_simple_expr("%1010", &symbols, 0).unwrap(), 10);
        assert_eq!(parse_simple_expr("0xDEAD", &symbols, 0).unwrap(), 0xDEAD);
    }

    #[test]
    fn test_parse_simple_expr_current_pc() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("$", &symbols, 0x1000).unwrap(), 0x1000);
        assert_eq!(
            parse_simple_expr("$ + 4", &symbols, 0x2000).unwrap(),
            0x2004
        );
        assert_eq!(
            parse_simple_expr("$ - $100", &symbols, 0x1200).unwrap(),
            0x1100
        );
        assert_eq!(parse_simple_expr("$ * 2", &symbols, 0x100).unwrap(), 0x200);
    }

    #[test]
    fn test_parse_simple_expr_star_current_pc() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("*", &symbols, 0x1000).unwrap(), 0x1000);
        assert_eq!(parse_simple_expr("*+4", &symbols, 0x2000).unwrap(), 0x2004);
        assert_eq!(
            parse_simple_expr("(*+2)*2", &symbols, 0x1000).unwrap(),
            0x2004
        );
        // '*' after an operand is still multiplication.
        assert_eq!(parse_simple_expr("3*4", &symbols, 0).unwrap(), 12);
        assert_eq!(parse_simple_expr("(1+1)*4", &symbols, 0).unwrap(), 8);
    }

    #[test]
    fn test_parse_simple_expr_defined() {
        let mut symbols = SymbolTable::new();
        symbols.define("FOO", 42, None).unwrap();
        symbols.declare("FORWARD_ONLY", None);

        assert_eq!(parse_simple_expr("DEFINED(FOO)", &symbols, 0).unwrap(), 1);
        assert_eq!(parse_simple_expr("defined(foo)", &symbols, 0).unwrap(), 0);
        assert_eq!(
            parse_simple_expr("DEFINED(NOT_A_SYMBOL)", &symbols, 0).unwrap(),
            0
        );
        // Declared but not yet given a value (forward reference) is not "defined".
        assert_eq!(
            parse_simple_expr("DEFINED(FORWARD_ONLY)", &symbols, 0).unwrap(),
            0
        );
        assert_eq!(
            parse_simple_expr("DEFINED(FOO) + DEFINED(NOT_A_SYMBOL)", &symbols, 0).unwrap(),
            1
        );
    }

    #[test]
    fn test_parse_simple_expr_arithmetic() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("1 + 2", &symbols, 0).unwrap(), 3);
        assert_eq!(parse_simple_expr("10 - 3", &symbols, 0).unwrap(), 7);
        assert_eq!(parse_simple_expr("4 * 5", &symbols, 0).unwrap(), 20);
        assert_eq!(parse_simple_expr("20 / 4", &symbols, 0).unwrap(), 5);
    }

    #[test]
    fn test_parse_simple_expr_bitwise() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("0xF0 & 0x0F", &symbols, 0).unwrap(), 0);
        assert_eq!(parse_simple_expr("0xF0 | 0x0F", &symbols, 0).unwrap(), 0xFF);
        assert_eq!(parse_simple_expr("0xFF ^ 0x0F", &symbols, 0).unwrap(), 0xF0);
        assert_eq!(parse_simple_expr("~0", &symbols, 0).unwrap(), -1);
    }

    #[test]
    fn test_parse_simple_expr_shift() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("1 << 8", &symbols, 0).unwrap(), 256);
        assert_eq!(parse_simple_expr("256 >> 4", &symbols, 0).unwrap(), 16);
    }

    #[test]
    fn test_parse_simple_expr_high_low() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("HIGH($1234)", &symbols, 0).unwrap(), 0x12);
        assert_eq!(parse_simple_expr("LOW($1234)", &symbols, 0).unwrap(), 0x34);
    }

    #[test]
    fn test_parse_simple_expr_precedence() {
        let symbols = SymbolTable::new();
        // * before +
        assert_eq!(parse_simple_expr("2 + 3 * 4", &symbols, 0).unwrap(), 14);
        // << before |
        assert_eq!(
            parse_simple_expr("1 << 4 | 0xF", &symbols, 0).unwrap(),
            0x1F
        );
    }

    #[test]
    fn test_parse_simple_expr_parentheses() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("(2 + 3) * 4", &symbols, 0).unwrap(), 20);
        assert_eq!(parse_simple_expr("((1 + 2) * 3)", &symbols, 0).unwrap(), 9);
    }

    #[test]
    fn test_parse_simple_expr_symbols() {
        let mut symbols = SymbolTable::new();
        symbols.define("BASE", 0x1000, Some(1)).unwrap();
        symbols.define("OFFSET", 0x10, Some(2)).unwrap();
        assert_eq!(
            parse_simple_expr("BASE + OFFSET", &symbols, 0).unwrap(),
            0x1010
        );
    }

    #[test]
    fn test_parse_simple_expr_unary_minus() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("-42", &symbols, 0).unwrap(), -42);
        assert_eq!(parse_simple_expr("-$10", &symbols, 0).unwrap(), -16);
    }

    #[test]
    fn test_section_kind_from_name() {
        assert_eq!(SectionKind::from_name("text"), SectionKind::Text);
        assert_eq!(SectionKind::from_name("CODE"), SectionKind::Text);
        assert_eq!(SectionKind::from_name("data"), SectionKind::Data);
        assert_eq!(SectionKind::from_name("bss"), SectionKind::Bss);
        assert_eq!(
            SectionKind::from_name("mysection"),
            SectionKind::Named("mysection".to_string())
        );
    }

    #[test]
    fn test_handle_even() {
        assert_eq!(handle_even_pass1(0x1001).bytes_emitted, 2);
        assert_eq!(handle_even_pass1(0x1000).bytes_emitted, 0);
        assert_eq!(handle_even_pass1(0x1002).bytes_emitted, 0);
    }

    #[test]
    fn test_handle_align_pass1() {
        let symbols = SymbolTable::new();
        // Already aligned
        assert_eq!(
            handle_align_pass1(&["4".to_string()], &symbols, 0x1000, 1)
                .unwrap()
                .bytes_emitted,
            0
        );
        // Need 2 bytes padding to align to 4
        assert_eq!(
            handle_align_pass1(&["4".to_string()], &symbols, 0x1002, 1)
                .unwrap()
                .bytes_emitted,
            2
        );
        // Need 4 bytes padding to align to 8
        assert_eq!(
            handle_align_pass1(&["8".to_string()], &symbols, 0x1004, 1)
                .unwrap()
                .bytes_emitted,
            4
        );
    }

    #[test]
    fn test_handle_align_not_power_of_two() {
        let symbols = SymbolTable::new();
        assert!(handle_align_pass1(&["3".to_string()], &symbols, 0x1000, 1).is_err());
        assert!(handle_align_pass1(&["0".to_string()], &symbols, 0x1000, 1).is_err());
    }

    #[test]
    fn test_handle_org() {
        let symbols = SymbolTable::new();
        let result = handle_org(&["$2000".to_string()], &symbols, 0x1000, 1).unwrap();
        assert!(result.pc_changed);
        assert_eq!(result.new_pc, Some(0x2000));
    }

    #[test]
    fn test_section_manager() {
        let mut mgr = SectionManager::new(0x1000);
        assert_eq!(mgr.current_pc(), 0x1000);

        mgr.switch_section(SectionKind::Data);
        assert_eq!(mgr.current_pc(), 0x1000);

        mgr.set_current_pc(0x3000);
        assert_eq!(mgr.current_pc(), 0x3000);

        mgr.switch_section(SectionKind::Text);
        assert_eq!(mgr.current_pc(), 0x1000); // Text section unchanged
    }

    #[test]
    fn test_expression_comparison() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("5 > 3", &symbols, 0).unwrap(), 1);
        assert_eq!(parse_simple_expr("3 > 5", &symbols, 0).unwrap(), 0);
        assert_eq!(parse_simple_expr("5 >= 5", &symbols, 0).unwrap(), 1);
        assert_eq!(parse_simple_expr("5 == 5", &symbols, 0).unwrap(), 1);
        assert_eq!(parse_simple_expr("5 != 3", &symbols, 0).unwrap(), 1);
    }

    #[test]
    fn test_expression_logical_and_or() {
        let symbols = SymbolTable::new();
        assert_eq!(parse_simple_expr("1 && 1", &symbols, 0).unwrap(), 1);
        assert_eq!(parse_simple_expr("1 && 0", &symbols, 0).unwrap(), 0);
        assert_eq!(parse_simple_expr("0 || 1", &symbols, 0).unwrap(), 1);
        assert_eq!(parse_simple_expr("0 || 0", &symbols, 0).unwrap(), 0);
    }
}
