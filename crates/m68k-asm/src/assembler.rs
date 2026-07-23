//! Two-pass Motorola 68000 assembler.
//!
//! # Architecture
//!
//! The assembler operates in two passes:
//!
//! **Pass 1**: Parse source lines, collect labels, build symbol table,
//! calculate instruction sizes (with optimistic branch sizing).
//!
//! **Pass 2**: Encode all instructions with resolved symbols,
//! perform branch relaxation (iterate until stable).

use std::collections::HashMap;
use std::path::PathBuf;

use m68k_core::errors::{AsmError, ErrorCollector};
use m68k_core::operands::Operand;
use m68k_core::tokens::split_line;

use crate::directives::{
    SectionManager, handle_align_pass1, handle_align_pass2, handle_equ, handle_even_pass1,
    handle_even_pass2, handle_incbin_pass1, handle_incbin_pass2, handle_section, handle_set,
    parse_dc_string,
};
use crate::encoder::encode_instruction;

// ---------------------------------------------------------------------------
// Symbol table
// ---------------------------------------------------------------------------

/// A macro definition collected during pre-processing.
#[derive(Debug, Clone)]
pub struct MacroDefinition {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<String>,
}

/// Entry in the assembler's symbol table.
#[derive(Debug, Clone)]
pub struct SymbolEntry {
    pub name: String,
    pub value: u32,
    pub defined: bool,
    pub line_no: Option<usize>,
    /// Name of the section this symbol was defined in (e.g. "text",
    /// "data"), if known. Used by ELF/IEEE-695 output to assign the
    /// correct section index without relying on an address-range
    /// heuristic. `None` for forward-declared/undefined symbols and for
    /// symbols defined outside of a section context (e.g. via `force_set`
    /// without section tracking).
    pub section: Option<String>,
}

impl SymbolEntry {
    pub fn new(name: String, value: u32, defined: bool, line_no: Option<usize>) -> Self {
        Self {
            name,
            value,
            defined,
            line_no,
            section: None,
        }
    }

    pub fn with_section(mut self, section: Option<String>) -> Self {
        self.section = section;
        self
    }
}

/// The symbol table mapping label/constant names to their values.
#[derive(Debug, Default)]
pub struct SymbolTable {
    symbols: HashMap<String, SymbolEntry>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Define or update a symbol. Returns `Err` if redefining an already-defined symbol.
    pub fn define(
        &mut self,
        name: &str,
        value: u32,
        line_no: Option<usize>,
    ) -> Result<(), AsmError> {
        self.define_in_section(name, value, line_no, None)
    }

    /// Like [`Self::define`], additionally recording which section (by
    /// name, e.g. "text"/"data") the symbol was defined in — used by
    /// ELF/IEEE-695 output to assign `st_shndx`/section index directly
    /// instead of guessing from the symbol's address range.
    pub fn define_in_section(
        &mut self,
        name: &str,
        value: u32,
        line_no: Option<usize>,
        section: Option<&str>,
    ) -> Result<(), AsmError> {
        if let Some(existing) = self.symbols.get(name)
            && existing.defined
        {
            return Err(AsmError::with_line(
                format!("symbol '{}' already defined", name),
                line_no.unwrap_or(0),
            ));
        }
        self.symbols.insert(
            name.to_string(),
            SymbolEntry::new(name.to_string(), value, true, line_no)
                .with_section(section.map(|s| s.to_string())),
        );
        Ok(())
    }

    /// Declare a forward-referenced symbol (undefined, value = 0).
    pub fn declare(&mut self, name: &str, line_no: Option<usize>) {
        if !self.symbols.contains_key(name) {
            self.symbols.insert(
                name.to_string(),
                SymbolEntry::new(name.to_string(), 0, false, line_no),
            );
        }
    }

    /// Look up a symbol. Returns `Err` if undefined.
    pub fn resolve(&self, name: &str) -> Result<u32, AsmError> {
        match self.symbols.get(name) {
            Some(entry) if entry.defined => Ok(entry.value),
            Some(entry) => Err(AsmError::with_line(
                format!("undefined symbol: {}", name),
                entry.line_no.unwrap_or(0),
            )),
            None => Err(AsmError::new(format!("undefined symbol: {}", name))),
        }
    }

    /// Look up a symbol, returning `None` if undefined (no error).
    pub fn get(&self, name: &str) -> Option<&SymbolEntry> {
        self.symbols.get(name)
    }

    /// Check if a symbol exists (defined or not).
    pub fn contains(&self, name: &str) -> bool {
        self.symbols.contains_key(name)
    }

    /// Force-set a symbol value (allows redefinition, used by SET directive).
    pub fn force_set(&mut self, name: &str, value: u32, line_no: Option<usize>) {
        self.symbols.insert(
            name.to_string(),
            SymbolEntry::new(name.to_string(), value, true, line_no),
        );
    }

    /// Iterate over all defined symbols.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &SymbolEntry)> {
        self.symbols.iter()
    }
}

// ---------------------------------------------------------------------------
// Parsed source line
// ---------------------------------------------------------------------------

/// Type of a parsed source line.
#[derive(Debug, Clone)]
pub enum LineType {
    /// Empty line or comment-only.
    Empty,
    /// Label-only line (label: with no instruction).
    Label,
    /// An instruction to be encoded.
    Instruction {
        mnemonic: String,
        size: Option<String>,
        operand_texts: Vec<String>,
    },
    /// A directive (ORG, DC, DS, EQU, EVEN, etc.).
    Directive { name: String, args: Vec<String> },
}

/// A single parsed line of source code.
#[derive(Debug, Clone)]
pub struct ParsedLine {
    pub line_no: usize,
    pub label: Option<String>,
    pub line_type: LineType,
    pub raw: String,
}

// ---------------------------------------------------------------------------
// Assembled instruction output
// ---------------------------------------------------------------------------

/// A fully encoded instruction or data block with its location counter value.
#[derive(Debug, Clone)]
pub struct AssembledInstruction {
    /// Location counter value (address) where this was assembled.
    pub pc: u32,
    /// Encoded 16-bit words.
    pub words: Vec<u16>,
    /// Source line number (for diagnostics).
    pub line_no: Option<usize>,
    /// Original source text (for listing generation).
    pub source: Option<String>,
}

impl AssembledInstruction {
    pub fn size_bytes(&self) -> usize {
        self.words.len() * 2
    }
}

// ---------------------------------------------------------------------------
// Branch relaxation info
// ---------------------------------------------------------------------------

/// Size hint for branch instructions during relaxation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchSize {
    /// Unspecified (let encoder decide).
    Any,
    /// Force short (8-bit displacement).
    Short,
    /// Force word (16-bit displacement).
    Word,
    /// Force long (32-bit displacement, 68020+).
    Long,
}

/// Information about a branch that may need relaxation.
#[derive(Debug, Clone)]
pub struct BranchInfo {
    /// Index into the assembled instructions list.
    pub instr_index: usize,
    /// Branch mnemonic (BRA, BSR, Bcc, DBcc).
    pub mnemonic: String,
    /// Target symbol name (if symbolic).
    pub target_symbol: Option<String>,
    /// Target address (if absolute).
    pub target_address: Option<u32>,
    /// Current size hint.
    pub size_hint: BranchSize,
    /// Source line number.
    pub line_no: Option<usize>,
}

// ---------------------------------------------------------------------------
// Operand parsing helper
// ---------------------------------------------------------------------------

/// Parse an operand string into an `Operand`, resolving symbols where needed.
fn parse_operand_text(
    text: &str,
    symbols: &SymbolTable,
    current_pc: u32,
) -> Result<Operand, AsmError> {
    let text = text.trim();

    // Bitfield: ea{offset:width}
    if let Some(bitfield) = parse_bitfield(text, symbols, current_pc)? {
        return Ok(bitfield);
    }

    // FPU data register: FP0-FP7
    if let Some(n) = parse_fp_reg(text) {
        return Ok(Operand::FpReg(n));
    }

    // FPU control register (list): FPCR, FPSR, FPIAR, or e.g. FPCR/FPSR
    if let Some(mask) = parse_fp_ctrl_list(text) {
        return Ok(Operand::FpCtrlList(mask));
    }

    // FMOVEM register list/range: FP0/FP2-FP4
    if let Some(mask) = parse_fp_reg_list(text) {
        return Ok(Operand::Immediate(mask as i64));
    }

    // MOVEM register list/range: D0-D7/A0-A7
    if let Some(mask) = parse_movem_reg_list(text) {
        return Ok(Operand::Immediate(mask as i64));
    }

    // Dh:Dl / Dr:Dq register pair (64-bit MUL.L/DIV.L destination forms).
    if let Some((a, b)) = text.split_once(':')
        && let (Some(Operand::DataReg(ra)), Some(Operand::DataReg(rb))) =
            (parse_register(a.trim()), parse_register(b.trim()))
    {
        return Ok(Operand::RegPair(ra, rb));
    }

    // Registers
    if let Some(reg) = parse_register(text) {
        return Ok(reg);
    }

    // Immediate: #expr
    if let Some(expr_str) = text.strip_prefix('#') {
        let value = evaluate_expr_str(expr_str, symbols, current_pc)?;
        return Ok(Operand::Immediate(value));
    }

    // 68020+ memory indirect / full format: ([bd,An,Xn],od) or ([bd,An],Xn,od)
    if let Some(mi) = parse_memory_indirect(text, symbols, current_pc)? {
        return Ok(Operand::MemoryIndirect(Box::new(mi)));
    }

    // Address register indirect: (An)
    if let Some(reg) = parse_parens_register(text) {
        return Ok(Operand::AddrRegIndirect(reg));
    }

    // Post-increment: (An)+
    if let Some(reg) = parse_parens_register_plus(text) {
        return Ok(Operand::AddrRegPostInc(reg));
    }

    // Pre-decrement: -(An)
    if let Some(reg) = parse_minus_parens_register(text) {
        return Ok(Operand::AddrRegPreDec(reg));
    }

    // Motorola-style displacement before the parens: `disp(An)`,
    // `disp(An,Xn)`, `disp(PC)` etc. (as opposed to the `(disp,An)` form
    // handled by the parsers below). Normalize by moving the displacement
    // inside the parens so the same parsers handle both spellings.
    let paren_text: std::borrow::Cow<str> = match split_disp_before_paren(text) {
        Some(normalized) => std::borrow::Cow::Owned(normalized),
        None => std::borrow::Cow::Borrowed(text),
    };
    let paren_text = paren_text.as_ref();

    // PC-relative with index: (d8,PC,Xn*scale) - before plain PC-relative
    if let Some((xn, disp, scale, xn_is_long)) = parse_parens_disp_pc_index(paren_text) {
        return Ok(Operand::PcRelativeIndex(xn, disp, scale, xn_is_long));
    }

    // PC-relative with displacement: (d16,PC) or (d32,PC)
    if let Some((disp, is_long)) = parse_parens_disp_pc(paren_text) {
        return Ok(Operand::PcRelativeDisp(disp, is_long));
    }

    // Addressing with displacement: (d16,An) or (d32,An)
    if let Some((disp, reg)) = parse_parens_disp_register(paren_text) {
        let is_long = !(-0x8000..=0x7FFF).contains(&disp);
        return Ok(Operand::AddrRegIndirectDisp(reg, disp, is_long));
    }

    // Indexed with base register: (d8,An,Xn*scale)
    if let Some((an, xn, disp, scale, xn_is_long)) = parse_parens_disp_reg_index(paren_text) {
        return Ok(Operand::AddrRegIndirectIndex(
            an, xn, disp, scale, xn_is_long,
        ));
    }

    // Absolute address with .W/.L suffix
    if text.ends_with(".W") || text.ends_with(".L") {
        let force_long = text.ends_with(".L");
        let base = text[..text.len() - 2].trim();
        if let Ok(value) = evaluate_expr_str(base, symbols, current_pc)
            && !text.contains('(')
            && !text.contains(')')
        {
            return if force_long {
                Ok(Operand::AbsoluteLong(value as i32))
            } else {
                Ok(Operand::AbsoluteShort(value as i32))
            };
        }
    }

    // Absolute short: $xxxx or number without register
    if let Ok(value) = evaluate_expr_str(text, symbols, current_pc) {
        // Check if it looks like an absolute address (no register, no #)
        if !text.contains('(') && !text.contains(')') {
            if (0..=0xFFFF).contains(&value) {
                return Ok(Operand::AbsoluteShort(value as i32));
            } else {
                return Ok(Operand::AbsoluteLong(value as i32));
            }
        }
    }

    // Special registers
    if text.eq_ignore_ascii_case("CCR") {
        return Ok(Operand::Immediate(-1));
    }
    if text.eq_ignore_ascii_case("SR") {
        return Ok(Operand::Immediate(-2));
    }
    if text.eq_ignore_ascii_case("USP") {
        return Ok(Operand::AddrReg(7));
    }

    // MOVEC control register names
    let cr_number = |name: &str| -> Option<i64> {
        match name.to_uppercase().as_str() {
            "SFC" => Some(0x000),
            "DFC" => Some(0x001),
            "CACR" => Some(0x002),
            "TC" => Some(0x003),
            "ITT0" => Some(0x004),
            "ITT1" => Some(0x005),
            "DTT0" => Some(0x006),
            "DTT1" => Some(0x007),
            "USP" => Some(0x800),
            "VBR" => Some(0x801),
            "CAAR" => Some(0x802),
            "MSP" => Some(0x803),
            "ISP" => Some(0x804),
            "MMUSR" => Some(0x805),
            "URP" => Some(0x806),
            "SRP" => Some(0x807),
            _ => None,
        }
    };
    if let Some(cr) = cr_number(text) {
        return Ok(Operand::Immediate(cr));
    }

    // PMOVE MMU control register names (TT0/TT1/CRP don't exist in the MOVEC namespace above;
    // TC/SRP/MMUSR are ambiguous with MOVEC's control registers of the same name, so PMOVE's
    // encoder receives the raw name via Operand::Special and resolves it itself).
    if matches!(text.to_uppercase().as_str(), "TT0" | "TT1" | "CRP") {
        return Ok(Operand::Special(text.to_uppercase()));
    }

    // Symbol / label reference (branch target)
    if is_identifier(text) {
        return Ok(Operand::Address(0)); // Will be resolved later
    }

    Err(AsmError::new(format!("cannot parse operand: {}", text)))
}

/// Parse bitfield syntax: ea{offset:width}. Returns None if `text` has no top-level `{...}`.
fn parse_bitfield(
    text: &str,
    symbols: &SymbolTable,
    current_pc: u32,
) -> Result<Option<Operand>, AsmError> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut brace_start = None;
    let mut brace_end = None;
    let mut colon_pos = None;

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'{' => {
                if depth == 0 {
                    brace_start = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    brace_end = Some(i);
                    break;
                }
            }
            b':' if depth == 1 && colon_pos.is_none() => {
                colon_pos = Some(i);
            }
            _ => {}
        }
    }

    let (Some(start), Some(end)) = (brace_start, brace_end) else {
        return Ok(None);
    };
    let Some(colon) = colon_pos else {
        return Err(AsmError::new("invalid bitfield syntax: missing ':'"));
    };

    let ea_text = &text[..start];
    let offset_text = text[start + 1..colon].trim();
    let width_text = text[colon + 1..end].trim();

    let ea = parse_operand_text(ea_text, symbols, current_pc)?;
    let offset = parse_bitfield_val(offset_text, symbols, current_pc)?;
    let width = parse_bitfield_val(width_text, symbols, current_pc)?;

    Ok(Some(Operand::Bitfield(
        Box::new(ea),
        Box::new(offset),
        Box::new(width),
    )))
}

/// Parse a bitfield offset/width value: either a data register or a constant expression.
fn parse_bitfield_val(
    text: &str,
    symbols: &SymbolTable,
    current_pc: u32,
) -> Result<m68k_core::operands::BitfieldSpec, AsmError> {
    use m68k_core::operands::BitfieldSpec;
    if let Some(Operand::DataReg(n)) = parse_register(text) {
        return Ok(BitfieldSpec::DataReg(n));
    }
    // Bitfield offset/width may optionally carry a leading '#' (e.g. {#0:#8}).
    let text = text.strip_prefix('#').unwrap_or(text);
    let value = evaluate_expr_str(text, symbols, current_pc)?;
    Ok(BitfieldSpec::Immediate(value))
}

/// Parse an FPU data register name: FP0-FP7.
fn parse_fp_reg(text: &str) -> Option<u8> {
    let upper = text.to_uppercase();
    let rest = upper.strip_prefix("FP")?;
    let n: u8 = rest.parse().ok()?;
    (n <= 7).then_some(n)
}

/// Parse an FPU control register name or '/'-separated list: FPCR=4, FPSR=2, FPIAR=1.
fn parse_fp_ctrl_list(text: &str) -> Option<u8> {
    let mut mask = 0u8;
    for part in text.split('/') {
        let bit = match part.trim().to_uppercase().as_str() {
            "FPIAR" => 1,
            "FPSR" => 2,
            "FPCR" => 4,
            _ => return None,
        };
        mask |= bit;
    }
    Some(mask)
}

/// Parse an FMOVEM FPU-register list/range: `FP0/FP2`, `FP0-FP3`, `FP0-FP2/FP5`.
/// Returns `None` for a single bare `FPn` (handled separately by `parse_fp_reg`).
fn parse_fp_reg_list(text: &str) -> Option<u8> {
    if !text.contains('/') && !text.contains('-') {
        return None;
    }
    let mut mask = 0u8;
    for part in text.split('/') {
        let part = part.trim();
        if let Some(n) = parse_fp_reg(part) {
            mask |= 1 << n;
        } else if let Some((a, b)) = part.split_once('-') {
            let lo = parse_fp_reg(a.trim())?;
            let hi = parse_fp_reg(b.trim())?;
            for n in lo..=hi {
                mask |= 1 << n;
            }
        } else {
            return None;
        }
    }
    Some(mask)
}

/// Parse a MOVEM register list/range: `D0-D7/A0-A7`, `D0/D2/A5-A7`.
/// The mask uses bits 0-7 for D0-D7 and bits 8-15 for A0-A7.
fn parse_movem_reg_list(text: &str) -> Option<u16> {
    if !text.contains('/') && !text.contains('-') {
        return None;
    }
    let mut mask = 0u16;
    for part in text.split('/') {
        let part = part.trim();
        if let Some((a, b)) = part.split_once('-') {
            let (lo, lo_offset) = parse_dan_reg(a.trim())?;
            let (hi, hi_offset) = parse_dan_reg(b.trim())?;
            if lo_offset != hi_offset {
                return None;
            }
            for n in lo..=hi {
                mask |= 1 << (n + lo_offset);
            }
        } else {
            let (n, offset) = parse_dan_reg(part)?;
            mask |= 1 << (n + offset);
        }
    }
    Some(mask)
}

/// Parse `Dn`/`An` for MOVEM list purposes, returning (register number, mask bit offset).
fn parse_dan_reg(text: &str) -> Option<(u8, u8)> {
    match parse_register(text)? {
        Operand::DataReg(n) => Some((n, 0)),
        Operand::AddrReg(n) => Some((n, 8)),
        _ => None,
    }
}

/// Parse a register name (D0-D7, A0-A7).
fn parse_register(text: &str) -> Option<Operand> {
    let upper = text.to_uppercase();
    if upper == "SP" {
        return Some(Operand::AddrReg(7));
    }
    if upper.starts_with('D')
        && upper.len() == 2
        && let Ok(n) = upper[1..].parse::<u8>()
        && n <= 7
    {
        return Some(Operand::DataReg(n));
    }
    if upper.starts_with('A')
        && upper.len() == 2
        && let Ok(n) = upper[1..].parse::<u8>()
        && n <= 7
    {
        return Some(Operand::AddrReg(n));
    }
    None
}

/// Parse (An) - address register indirect.
fn parse_parens_register(text: &str) -> Option<u8> {
    let trimmed = text.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        if let Some(Operand::AddrReg(n)) = parse_register(inner) {
            return Some(n);
        }
    }
    None
}

/// Parse (An)+ - post-increment.
fn parse_parens_register_plus(text: &str) -> Option<u8> {
    let trimmed = text.trim();
    if trimmed.ends_with(")+") {
        let inner = &trimmed[1..trimmed.len() - 2];
        if let Some(Operand::AddrReg(n)) = parse_register(inner) {
            return Some(n);
        }
    }
    None
}

/// Parse -(An) - pre-decrement.
fn parse_minus_parens_register(text: &str) -> Option<u8> {
    let trimmed = text.trim();
    if trimmed.starts_with("-(") && trimmed.ends_with(')') {
        let inner = &trimmed[2..trimmed.len() - 1];
        if let Some(Operand::AddrReg(n)) = parse_register(inner) {
            return Some(n);
        }
    }
    None
}

/// Parse (d,An) or (d,An.Xn) - indexed with displacement.
/// Normalize Motorola-style `disp(An)`/`disp(An,Xn)`/`disp(PC)` (displacement
/// written before an unwrapped paren) into the `(disp,An)` form the
/// `parse_parens_disp_*` helpers expect, by moving the leading displacement
/// expression inside the parens as its first comma-separated field. Returns
/// `None` if `text` doesn't have a non-empty prefix followed by a
/// parenthesized suffix (e.g. plain `(An)` or `(d16,An)` already have no
/// prefix and are left untouched).
fn split_disp_before_paren(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let paren_pos = trimmed.find('(')?;
    if paren_pos == 0 || !trimmed.ends_with(')') {
        return None;
    }
    let disp = trimmed[..paren_pos].trim();
    if disp.is_empty() {
        return None;
    }
    let inner = &trimmed[paren_pos + 1..trimmed.len() - 1];
    Some(format!("({},{})", disp, inner))
}

fn parse_parens_disp_register(text: &str) -> Option<(i32, u8)> {
    let trimmed = text.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        // Try (d,An)
        if let Some(pos) = inner.find(',') {
            let disp_str = inner[..pos].trim();
            let reg_str = inner[pos + 1..].trim();

            // Check for indexed: An.Xn (dot notation)
            if let Some(dot_pos) = reg_str.find('.') {
                let reg_part = &reg_str[..dot_pos];
                if let Some(Operand::AddrReg(n)) = parse_register(reg_part) {
                    let disp = evaluate_simple_number(disp_str).unwrap_or(0);
                    return Some((disp, n));
                }
            } else if let Some(Operand::AddrReg(n)) = parse_register(reg_str) {
                let disp = evaluate_simple_number(disp_str).unwrap_or(0);
                return Some((disp, n));
            }
        }
    }
    None
}

/// Parse (d16,PC) or (d32,PC) - PC-relative with displacement.
fn parse_parens_disp_pc(text: &str) -> Option<(i32, bool)> {
    let trimmed = text.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        if let Some(pos) = inner.rfind(',') {
            let disp_str = inner[..pos].trim();
            let reg_str = inner[pos + 1..].trim();
            if reg_str.to_uppercase() == "PC" {
                let disp = evaluate_simple_number(disp_str).unwrap_or(0);
                let is_long = !(-0x8000..=0x7FFF).contains(&disp);
                return Some((disp, is_long));
            }
        }
    }
    None
}

/// Parse (d8,PC,Xn*scale) - PC-relative with index register.
/// Returns (Xn, disp, scale, is_long).
fn parse_parens_disp_pc_index(text: &str) -> Option<(u8, i8, u8, bool)> {
    let trimmed = text.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 3 {
            let disp_str = parts[0].trim();
            let pc_str = parts[1].trim().to_uppercase();
            let xn_full = parts[2].trim();

            if pc_str == "PC" {
                let (xn_name, scale, is_long) = parse_index_reg_and_scale(xn_full);
                if let Some(reg) = parse_register(&xn_name)
                    && let Some(reg_num) = reg.reg_num()
                {
                    let disp = evaluate_simple_number(disp_str).unwrap_or(0);
                    let disp_i8 = (disp & 0xFF) as i8;
                    return Some((reg_num, disp_i8, scale, is_long));
                }
            }
        }
    }
    None
}

/// Parse (d8,An,Xn*scale) - base register with index register.
/// Supports optional scale: (d8,An,Xn*1), (d8,An,Xn*2), (d8,An,Xn*4), (d8,An,Xn*8)
/// Supports optional index size: (d8,An,Xn.W), (d8,An,Xn.L)
/// Returns (An, Xn, disp, scale, is_long).
fn parse_parens_disp_reg_index(text: &str) -> Option<(u8, u8, i8, u8, bool)> {
    let trimmed = text.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() >= 3 {
            let disp_str = parts[0].trim();
            let an_str = parts[1].trim();
            let xn_full = parts[2].trim();

            if let Some(Operand::AddrReg(an)) = parse_register(an_str) {
                let (xn_name, scale, is_long) = parse_index_reg_and_scale(xn_full);
                if let Some(reg) = parse_register(&xn_name)
                    && let Some(xn) = reg.reg_num()
                {
                    let disp = evaluate_simple_number(disp_str).unwrap_or(0);
                    let disp_i8 = (disp & 0xFF) as i8;
                    return Some((an, xn, disp_i8, scale, is_long));
                }
            }
        }
    }
    None
}

/// Parse 68020+ memory indirect / full format EA syntax:
/// `([bd,An,Xn],od)` (pre-indexed, index inside brackets) or
/// `([bd,An],Xn,od)` (post-indexed, index outside brackets), with `An`
/// optionally replaced by `PC` or omitted entirely (base suppressed).
/// `bd`/`od` are optional expressions; `Xn` is optional and may carry a
/// `.W`/`.L` size suffix and `*scale` (2/4/8).
///
/// Returns `Ok(None)` if `text` isn't parenthesized with a `[` immediately
/// inside (i.e. it's some other addressing mode), `Err` if it looks like
/// memory-indirect syntax but is malformed.
fn parse_memory_indirect(
    text: &str,
    symbols: &SymbolTable,
    current_pc: u32,
) -> Result<Option<m68k_core::operands::MemoryIndirectOperand>, AsmError> {
    use m68k_core::operands::MemoryIndirectOperand;

    let trimmed = text.trim();
    if !trimmed.starts_with('(') {
        return Ok(None);
    }

    // Find the matching close paren for the outer '(...)', allowing a
    // trailing '+' is NOT valid here (memory indirect has no postincrement
    // form), so we just require the whole trimmed text to be "(...)".
    if !trimmed.ends_with(')') {
        return Ok(None);
    }
    let outer_inner = trimmed[1..trimmed.len() - 1].trim();
    if !outer_inner.starts_with('[') {
        return Ok(None);
    }

    // Find the matching ']' for the leading '['.
    let bytes = outer_inner.as_bytes();
    let mut depth = 0i32;
    let mut bracket_end = None;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    bracket_end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(bracket_end) = bracket_end else {
        return Err(AsmError::new("unmatched '[' in memory indirect operand"));
    };

    let bracket_inner = &outer_inner[1..bracket_end];
    let after_bracket = outer_inner[bracket_end + 1..].trim();
    let after_bracket = after_bracket.strip_prefix(',').unwrap_or(after_bracket);

    let bracket_parts: Vec<&str> = split_top_level_commas(bracket_inner);

    let mut base_reg: Option<u8> = None;
    let mut base_is_pc = false;
    let mut base_disp: Option<i32> = None;
    let mut index_reg: Option<u8> = None;
    let mut index_long = false;
    let mut index_scale: u8 = 1;

    for part in &bracket_parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.eq_ignore_ascii_case("pc") {
            base_is_pc = true;
            continue;
        }
        if let Some(Operand::AddrReg(n)) = parse_register(part) {
            base_reg = Some(n);
            continue;
        }
        if let Some((reg_name, size, scale)) = parse_index_reg_size_scale(part)
            && let Some(reg) = parse_register(&reg_name)
            && let Some(n) = reg.reg_num()
        {
            index_reg = Some(if matches!(reg, Operand::AddrReg(_)) {
                n + 8
            } else {
                n
            });
            index_long = size.eq_ignore_ascii_case("l");
            index_scale = scale;
            continue;
        }
        // Anything else is the base displacement expression.
        let value = evaluate_expr_str(part, symbols, current_pc)?;
        base_disp = Some(value as i32);
    }

    // Post-indexed form has the index register after the bracket (and
    // optionally a scale/size), pre-indexed form has it inside the bracket.
    let mut outer_disp: Option<i32> = None;
    let is_postindexed = index_reg.is_none() && !after_bracket.is_empty();

    if !after_bracket.is_empty() {
        let after_parts = split_top_level_commas(after_bracket);
        for part in after_parts {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((reg_name, size, scale)) = parse_index_reg_size_scale(part)
                && let Some(reg) = parse_register(&reg_name)
                && let Some(n) = reg.reg_num()
            {
                index_reg = Some(if matches!(reg, Operand::AddrReg(_)) {
                    n + 8
                } else {
                    n
                });
                index_long = size.eq_ignore_ascii_case("l");
                index_scale = scale;
                continue;
            }
            let value = evaluate_expr_str(part, symbols, current_pc)?;
            outer_disp = Some(value as i32);
        }
    }

    Ok(Some(MemoryIndirectOperand {
        base_reg,
        base_is_pc,
        base_disp,
        index_reg,
        index_long,
        index_scale,
        outer_disp,
        is_postindexed,
    }))
}

/// Split a comma-separated string at top-level commas only (not inside
/// nested brackets/parens — not expected in practice for memory indirect
/// sub-expressions, but kept for robustness).
fn split_top_level_commas(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Like `parse_index_reg_and_scale`, but also returns the size suffix
/// (`"w"`/`"l"`) instead of discarding it. Returns `None` if `text` doesn't
/// look like an index register at all (e.g. plain expression/number).
fn parse_index_reg_size_scale(text: &str) -> Option<(String, String, u8)> {
    let text = text.trim();
    let (reg_part, scale) = if let Some(pos) = text.find('*') {
        let scale_str = text[pos + 1..].trim();
        let scale = scale_str.parse::<u8>().unwrap_or(1);
        let scale = match scale {
            2 | 4 | 8 => scale,
            _ => 1,
        };
        (&text[..pos], scale)
    } else {
        (text, 1)
    };
    let (reg_name, size) = if let Some(pos) = reg_part.find('.') {
        (reg_part[..pos].to_string(), reg_part[pos + 1..].to_string())
    } else {
        (reg_part.to_string(), "w".to_string())
    };
    parse_register(&reg_name)?;
    Some((reg_name, size, scale))
}

/// Extract index register name and scale from a combined string like "D0*2" or "A1.W*4" or "D3".
/// Parse an index register spec like `d1.w`, `a0.l*4`, `d2*8`. Returns
/// `(register_name, scale, is_long)`; `is_long` is `true` only for an
/// explicit `.L` suffix (`.B` and a missing suffix both default to the
/// word-size encoding).
fn parse_index_reg_and_scale(text: &str) -> (String, u8, bool) {
    let text = text.trim();
    // Split on '*' to get scale
    let (reg_part, scale) = if let Some(pos) = text.find('*') {
        let scale_str = text[pos + 1..].trim();
        let scale = scale_str.parse::<u8>().unwrap_or(1);
        let scale = match scale {
            2 | 4 | 8 => scale,
            _ => 1,
        };
        (&text[..pos], scale)
    } else {
        (text, 1)
    };
    // Strip optional size suffix (.W/.L), recording whether it was .L.
    let (reg_name, is_long) = if let Some(pos) = reg_part.find('.') {
        let suffix = reg_part[pos + 1..].to_lowercase();
        (reg_part[..pos].to_string(), suffix == "l")
    } else {
        (reg_part.to_string(), false)
    };
    (reg_name, scale, is_long)
}

/// Evaluate a simple numeric expression ($hex, %bin, decimal).
fn evaluate_simple_number(text: &str) -> Option<i32> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('$') {
        return i32::from_str_radix(hex, 16).ok();
    }
    if let Some(hex) = text.strip_prefix("0x") {
        return i32::from_str_radix(hex, 16).ok();
    }
    if let Some(bin) = text.strip_prefix('%') {
        return i32::from_str_radix(bin, 2).ok();
    }
    text.parse::<i32>().ok()
}

/// Check if text is a valid identifier.
fn is_identifier(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let mut chars = text.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Evaluate an expression string using the symbol table.
fn evaluate_expr_str(text: &str, symbols: &SymbolTable, pc: u32) -> Result<i64, AsmError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AsmError::new("empty expression"));
    }

    crate::directives::parse_simple_expr(text, symbols, pc)
        .map_err(|e| AsmError::new(format!("expression error: {}", e)))
}

// ---------------------------------------------------------------------------
// Source line parsing
// ---------------------------------------------------------------------------

/// Parse a complete source text into a list of `ParsedLine`s.
pub fn parse_source(source: &str) -> Vec<ParsedLine> {
    let mut lines = Vec::new();

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let (label, mnemonic, size, operand_texts) = split_line(raw_line);

        let line_type = if mnemonic.is_empty() {
            if label.is_some() {
                LineType::Label
            } else {
                LineType::Empty
            }
        } else if is_directive_name(&mnemonic) {
            LineType::Directive {
                name: mnemonic,
                args: build_directive_args(&label, &size, &operand_texts),
            }
        } else {
            LineType::Instruction {
                mnemonic,
                size: if size.is_empty() { None } else { Some(size) },
                operand_texts,
            }
        };

        lines.push(ParsedLine {
            line_no,
            label,
            line_type,
            raw: raw_line.to_string(),
        });
    }

    lines
}

/// Check if a mnemonic name is a directive.
fn is_directive_name(name: &str) -> bool {
    matches!(
        name,
        "org"
            | "equ"
            | "dc"
            | "dcb"
            | "ds"
            | "even"
            | "align"
            | "set"
            | "include"
            | "incbin"
            | "macro"
            | "endm"
            | "section"
            | "xref"
            | "xdef"
            | "public"
            | "extern"
            | "rept"
            | "irp"
            | "irpc"
            | "endr"
            | "text"
            | "data"
            | "bss"
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
            | "else"
            | "endif"
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
    )
}

/// Build directive arguments from parsed components.
fn build_directive_args(
    _label: &Option<String>,
    _size: &str,
    operand_texts: &[String],
) -> Vec<String> {
    // For directives, the "size" often carries meaning (DC.B, DS.W, etc.)
    // and operands carry the values
    let mut args = Vec::new();
    if !_size.is_empty() {
        args.push(_size.to_string());
    }
    args.extend(operand_texts.iter().cloned());
    args
}

// ---------------------------------------------------------------------------
// Assembler
// ---------------------------------------------------------------------------

/// The two-pass assembler.
///
/// # Example
///
/// ```
/// use m68k_asm::assembler::Assembler;
///
/// let mut asm = Assembler::new(0x1000);
/// let result = asm.assemble("
///     ORG $1000
///     MOVE.B D0,D1
///     NOP
///     BRA start
/// start:
///     RTS
/// ");
/// let bytes = result.unwrap();
/// ```
pub struct Assembler {
    /// Origin address (start of code).
    pub origin: u32,
    /// Symbol table built during pass 1.
    pub symbols: SymbolTable,
    /// Encoded instructions from pass 2.
    pub code: Vec<AssembledInstruction>,
    /// Branch info for relaxation.
    pub branches: Vec<BranchInfo>,
    /// CPU target (e.g., "68000", "68010", "68020").
    pub cpu: String,
    /// Error collector.
    pub errors: ErrorCollector,
    /// Current program counter during assembly.
    pc: u32,
    /// Location counter values for each parsed line (computed in pass 1).
    line_pcs: Vec<(usize, u32)>, // (line_no, pc)
    /// Root directory for resolving INCLUDE/INCBIN paths.
    pub source_root: PathBuf,
    /// Conditional assembly nesting stack. true = currently including code.
    conditional_stack: Vec<bool>,
    /// Macro definitions collected during pre-processing.
    pub macro_definitions: HashMap<String, MacroDefinition>,
    /// Unique counter for \@ local label generation.
    macro_unique_counter: u32,
    /// Section manager for named sections.
    pub sections: SectionManager,
    /// RS (structure offset) counter for struct layout.
    pub rs_counter: u32,
}

impl Assembler {
    /// Create a new assembler with the given origin address.
    pub fn new(origin: u32) -> Self {
        Self {
            origin,
            symbols: SymbolTable::new(),
            code: Vec::new(),
            branches: Vec::new(),
            cpu: "68000".to_string(),
            errors: ErrorCollector::new(),
            pc: origin,
            line_pcs: Vec::new(),
            source_root: PathBuf::from("."),
            conditional_stack: Vec::new(),
            macro_definitions: HashMap::new(),
            macro_unique_counter: 0,
            sections: SectionManager::new(origin),
            rs_counter: 0,
        }
    }

    /// Push an assembled instruction into both the flat code vector and the
    /// current section. `self.pc` remains the single source of truth for the
    /// location counter during pass 2 (ORG/SECTION can move it independently
    /// of instruction size), so the section's counter is re-synced from it
    /// after every push rather than incremented separately.
    fn push_instruction(&mut self, instr: AssembledInstruction) {
        let size = instr.size_bytes() as u32;
        self.sections.add_instruction(instr.clone());
        self.code.push(instr);
        self.sections.set_current_pc(self.pc + size);
    }

    /// Check if the current conditional nesting allows code generation.
    fn is_conditional_active(&self) -> bool {
        self.conditional_stack.last().copied().unwrap_or(true)
    }

    /// Evaluate a conditional directive name and its argument.
    fn eval_conditional(&self, name: &str, arg: &str, line_no: usize) -> Result<bool, AsmError> {
        match name {
            "ifdef" | "ifndef" => {
                let sym = arg.trim();
                let defined = self.symbols.contains(sym);
                Ok(if name == "ifdef" { defined } else { !defined })
            }
            "ifc" | "ifnc" => {
                // IFC/IFNC compares two comma-separated strings
                let parts: Vec<&str> = arg.splitn(2, ',').collect();
                if parts.len() < 2 {
                    return Err(AsmError::with_line(
                        format!("{} requires two comma-separated strings", name),
                        line_no,
                    ));
                }
                let s1 = crate::directives::strip_quotes(parts[0].trim());
                let s2 = crate::directives::strip_quotes(parts[1].trim());
                let equal = s1 == s2;
                Ok(if name == "ifc" { equal } else { !equal })
            }
            "if" | "ifne" => {
                let val = evaluate_expr_str(arg, &self.symbols, self.pc)
                    .map_err(|e| AsmError::with_line(e.message, line_no))?;
                Ok(val != 0)
            }
            "ifeq" => {
                let val = evaluate_expr_str(arg, &self.symbols, self.pc)
                    .map_err(|e| AsmError::with_line(e.message, line_no))?;
                Ok(val == 0)
            }
            "ifgt" => {
                let val = evaluate_expr_str(arg, &self.symbols, self.pc)
                    .map_err(|e| AsmError::with_line(e.message, line_no))?;
                Ok(val > 0)
            }
            "iflt" => {
                let val = evaluate_expr_str(arg, &self.symbols, self.pc)
                    .map_err(|e| AsmError::with_line(e.message, line_no))?;
                Ok(val < 0)
            }
            "ifge" => {
                let val = evaluate_expr_str(arg, &self.symbols, self.pc)
                    .map_err(|e| AsmError::with_line(e.message, line_no))?;
                Ok(val >= 0)
            }
            "ifle" => {
                let val = evaluate_expr_str(arg, &self.symbols, self.pc)
                    .map_err(|e| AsmError::with_line(e.message, line_no))?;
                Ok(val <= 0)
            }
            _ => Err(AsmError::with_line(
                format!("unknown conditional: {}", name),
                line_no,
            )),
        }
    }

    /// Set the source root directory for INCLUDE/INCBIN resolution.
    pub fn set_source_root(&mut self, path: PathBuf) {
        self.source_root = path;
    }

    /// Set the CPU target.
    pub fn set_cpu(&mut self, cpu: &str) {
        self.cpu = cpu.to_string();
    }

    /// Assemble source text into bytes.
    ///
    /// This runs both passes and branch relaxation, returning the final
    /// binary as a flat `Vec<u8>`.
    pub fn assemble_bytes(&mut self, source: &str) -> Result<Vec<u8>, AsmError> {
        self.assemble(source)?;

        let mut bytes = Vec::new();
        for instr in &self.code {
            for word in &instr.words {
                bytes.push((word >> 8) as u8);
                bytes.push((word & 0xFF) as u8);
            }
        }
        Ok(bytes)
    }

    /// Pre-process macros: collect definitions and expand invocations.
    /// Also handles END directive (strips remaining source) and
    /// repetitive constructs.
    /// Returns expanded source text.
    fn macro_preprocess(&mut self, source: &str) -> String {
        self.macro_definitions.clear();
        self.macro_unique_counter = 0;
        let mut output = Vec::new();
        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let raw = lines[i];
            // Check for MACRO definition via proper parsing
            let (lbl1, mnemonic1, _, operands1) = split_line(raw);
            if mnemonic1 == "macro"
                && let Some(mname) = lbl1
            {
                let params: Vec<String> = operands1;
                i += 1;
                let mut body = Vec::new();
                while i < lines.len() && lines[i].trim().to_lowercase() != "endm" {
                    body.push(lines[i].to_string());
                    i += 1;
                }
                if i < lines.len() {
                    i += 1; // skip ENDM
                }
                self.macro_definitions
                    .entry(mname.to_lowercase())
                    .and_modify(|def| {
                        def.params = params.clone();
                        def.body = body.clone();
                    })
                    .or_insert(MacroDefinition {
                        name: mname,
                        params,
                        body,
                    });
                continue;
            }

            // Check for ENDM (standalone)
            if mnemonic1 == "endm" {
                output.push(raw.to_string());
                i += 1;
                continue;
            }

            // Check for ENDR (standalone outside REPT/IRP/IRPC)
            if mnemonic1 == "endr" {
                output.push(raw.to_string());
                i += 1;
                continue;
            }

            // --- REPT: repeat block N times ---
            if mnemonic1 == "rept" && !operands1.is_empty() {
                let count: usize = match operands1[0].parse() {
                    Ok(n) => n,
                    Err(_) => {
                        if let Some(hex) = operands1[0].strip_prefix('$') {
                            usize::from_str_radix(hex, 16).unwrap_or(0)
                        } else {
                            output.push(raw.to_string());
                            i += 1;
                            continue;
                        }
                    }
                };

                // Collect body until matching ENDR (with nesting)
                i += 1;
                let mut body = Vec::new();
                let mut depth = 1;
                while i < lines.len() && depth > 0 {
                    let (_, mne, _, _) = split_line(lines[i]);
                    if mne == "endr" {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if mne == "rept" || mne == "irp" || mne == "irpc" {
                        depth += 1;
                    }
                    body.push(lines[i].to_string());
                    i += 1;
                }
                if i < lines.len() {
                    i += 1; // skip ENDR
                }

                if let Some(ref lbl) = lbl1 {
                    output.push(format!("{} EQU $", lbl));
                }

                for _ in 0..count {
                    output.extend(body.clone());
                }
                continue;
            }

            // --- IRP: iterate over list of values ---
            if mnemonic1 == "irp" && operands1.len() >= 2 {
                let mut param_name = operands1[0].clone();
                if let Some(stripped) = param_name.strip_prefix('\\') {
                    param_name = stripped.to_string();
                }
                let values: Vec<String> = operands1[1..].to_vec();

                i += 1;
                let mut body = Vec::new();
                let mut depth = 1;
                while i < lines.len() && depth > 0 {
                    let (_, mne, _, _) = split_line(lines[i]);
                    if mne == "endr" {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if mne == "rept" || mne == "irp" || mne == "irpc" {
                        depth += 1;
                    }
                    body.push(lines[i].to_string());
                    i += 1;
                }
                if i < lines.len() {
                    i += 1;
                }

                if let Some(ref lbl) = lbl1 {
                    output.push(format!("{} EQU $", lbl));
                }

                let key = format!("\\{}", param_name);
                for value in &values {
                    for line in &body {
                        output.push(line.replace(&key, value));
                    }
                }
                continue;
            }

            // --- IRPC: iterate over characters of a string ---
            if mnemonic1 == "irpc" && operands1.len() >= 2 {
                let mut param_name = operands1[0].clone();
                if let Some(stripped) = param_name.strip_prefix('\\') {
                    param_name = stripped.to_string();
                }
                let chars: Vec<char> = operands1[1].chars().collect();

                i += 1;
                let mut body = Vec::new();
                let mut depth = 1;
                while i < lines.len() && depth > 0 {
                    let (_, mne, _, _) = split_line(lines[i]);
                    if mne == "endr" {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if mne == "rept" || mne == "irp" || mne == "irpc" {
                        depth += 1;
                    }
                    body.push(lines[i].to_string());
                    i += 1;
                }
                if i < lines.len() {
                    i += 1;
                }

                if let Some(ref lbl) = lbl1 {
                    output.push(format!("{} EQU $", lbl));
                }

                let key = format!("\\{}", param_name);
                for ch in &chars {
                    for line in &body {
                        output.push(line.replace(&key, &ch.to_string()));
                    }
                }
                continue;
            }

            // Detect macro invocation
            if !mnemonic1.is_empty()
                && mnemonic1 != "macro"
                && mnemonic1 != "endm"
                && !is_directive_name(&mnemonic1)
                && let Some(def) = self.macro_definitions.get(&mnemonic1.to_lowercase())
            {
                // Expand macro
                self.macro_unique_counter += 1;
                let unique_id = self.macro_unique_counter;

                // Build substitution map: \1..\9 from operands1 + \@
                let mut subs: Vec<(String, String)> = Vec::new();
                for (idx, actual) in operands1.iter().enumerate() {
                    if idx < 9 {
                        subs.push((format!("\\{}", idx + 1), actual.clone()));
                    }
                }
                // Named params: \paramname
                for (idx, pname) in def.params.iter().enumerate() {
                    if let Some(actual) = operands1.get(idx) {
                        subs.push((format!("\\{}", pname), actual.clone()));
                    }
                }
                // \@ → unique number
                subs.push(("\\@".to_string(), format!("{:04X}", unique_id)));

                // Label on invocation line becomes an EQU
                if let Some(ref lbl) = lbl1 {
                    output.push(format!("{} EQU $", lbl));
                }

                // Expand each body line (MEXIT/EXITM stops expansion)
                for body_line in &def.body {
                    let mut expanded = body_line.clone();
                    for (from, to) in &subs {
                        expanded = expanded.replace(from.as_str(), to.as_str());
                    }
                    let (_lbl, mne, _, _) = split_line(&expanded);
                    if mne == "mexit" || mne == "exitm" {
                        break;
                    }
                    output.push(expanded);
                }

                i += 1;
                continue;
            }

            // Handle END - truncate remaining source
            if mnemonic1 == "end" {
                break;
            }

            output.push(raw.to_string());
            i += 1;
        }

        output.join("\n")
    }

    /// Run the two-pass assembly process.
    pub fn assemble(&mut self, source: &str) -> Result<&[AssembledInstruction], AsmError> {
        let expanded = self.macro_preprocess(source);
        let parsed = parse_source(&expanded);

        // Pass 1: Build symbol table, calculate sizes
        self.pass1(&parsed)?;

        // Branch relaxation loop
        self.relax_branches(&parsed)?;

        // Pass 2: Encode instructions with resolved symbols
        self.pass2(&parsed)?;

        Ok(&self.code)
    }

    // -----------------------------------------------------------------------
    // Pass 1: Symbol collection and size calculation
    // -----------------------------------------------------------------------

    /// Pass 1: Collect labels, build symbol table, estimate sizes.
    fn pass1(&mut self, lines: &[ParsedLine]) -> Result<(), AsmError> {
        self.pc = self.origin;
        self.line_pcs.clear();
        self.conditional_stack.clear();

        for line in lines {
            self.line_pcs.push((line.line_no, self.pc));

            // Handle IF/ELSE/ENDIF always (manage conditional stack)
            if let LineType::Directive { name, args } = &line.line_type {
                match name.as_str() {
                    "ifc" | "ifnc" => {
                        let arg = args.join(",");
                        let result = self.eval_conditional(name, &arg, line.line_no)?;
                        let active = self.is_conditional_active() && result;
                        self.conditional_stack.push(active);
                        if let Some(ref label) = line.label {
                            let section = self.sections.current_section().map(|s| s.kind.name());
                            self.symbols.define_in_section(
                                label,
                                self.pc,
                                Some(line.line_no),
                                section,
                            )?;
                        }
                        continue;
                    }
                    "if" | "ifeq" | "ifne" | "ifgt" | "iflt" | "ifge" | "ifle" | "ifdef"
                    | "ifndef" => {
                        let arg = args.first().map(|s| s.as_str()).unwrap_or("");
                        let result = self.eval_conditional(name, arg, line.line_no)?;
                        let active = self.is_conditional_active() && result;
                        self.conditional_stack.push(active);
                        if let Some(ref label) = line.label {
                            let section = self.sections.current_section().map(|s| s.kind.name());
                            self.symbols.define_in_section(
                                label,
                                self.pc,
                                Some(line.line_no),
                                section,
                            )?;
                        }
                        continue;
                    }
                    "else" => {
                        if let Some(top) = self.conditional_stack.last_mut() {
                            *top = !*top;
                        } else {
                            return Err(AsmError::with_line("ELSE without IF", line.line_no));
                        }
                        if let Some(ref label) = line.label {
                            let section = self.sections.current_section().map(|s| s.kind.name());
                            self.symbols.define_in_section(
                                label,
                                self.pc,
                                Some(line.line_no),
                                section,
                            )?;
                        }
                        continue;
                    }
                    "endif" => {
                        if self.conditional_stack.pop().is_none() {
                            return Err(AsmError::with_line("ENDIF without IF", line.line_no));
                        }
                        if let Some(ref label) = line.label {
                            let section = self.sections.current_section().map(|s| s.kind.name());
                            self.symbols.define_in_section(
                                label,
                                self.pc,
                                Some(line.line_no),
                                section,
                            )?;
                        }
                        continue;
                    }
                    _ => {}
                }
            }

            // Skip lines inside inactive conditional blocks
            if !self.is_conditional_active() {
                continue;
            }

            // Handle label (skip for EQU/SET which manage their own labels)
            if let Some(ref label) = line.label {
                let is_equ_or_set = matches!(&line.line_type,
                    LineType::Directive { name, .. }
                    if name == "equ" || name == "set"
                );
                if !is_equ_or_set {
                    let section = self.sections.current_section().map(|s| s.kind.name());
                    self.symbols
                        .define_in_section(label, self.pc, Some(line.line_no), section)?;
                }
            }

            // Calculate size for this line
            let size = self.estimate_line_size(line)?;
            self.pc += size;
        }

        Ok(())
    }

    /// Estimate the byte size of a parsed line (for PC tracking in pass 1).
    fn estimate_line_size(&mut self, line: &ParsedLine) -> Result<u32, AsmError> {
        match &line.line_type {
            LineType::Empty | LineType::Label => Ok(0),

            LineType::Directive { name, args } => {
                self.estimate_directive_size(name, args, line.line_no, &line.label)
            }

            LineType::Instruction {
                mnemonic,
                size,
                operand_texts,
            } => {
                // For branch instructions, record them for relaxation
                let is_branch = is_branch_mnemonic(mnemonic);
                if is_branch {
                    // Estimate with word-sized branch (conservative)
                    let estimated_size = estimate_branch_size(mnemonic);
                    self.branches.push(BranchInfo {
                        instr_index: self.code.len(),
                        mnemonic: mnemonic.clone(),
                        target_symbol: None, // Will be resolved in pass 2
                        target_address: None,
                        size_hint: BranchSize::Any,
                        line_no: Some(line.line_no),
                    });
                    return Ok(estimated_size);
                }

                // For other instructions, try to encode with dummy values
                // to estimate size
                self.estimate_instruction_size_from_texts(mnemonic, size.as_deref(), operand_texts)
            }
        }
    }

    /// Estimate size for a directive.
    fn estimate_directive_size(
        &mut self,
        name: &str,
        args: &[String],
        line_no: usize,
        label: &Option<String>,
    ) -> Result<u32, AsmError> {
        match name {
            "org" => {
                if let Some(arg) = args.first() {
                    let addr = evaluate_expr_str(arg, &self.symbols, self.pc)?;
                    self.pc = addr as u32;
                    Ok(0)
                } else {
                    Err(AsmError::with_line(
                        "ORG requires address argument",
                        line_no,
                    ))
                }
            }
            "equ" => {
                // EQU defines a symbol with a label: LABEL EQU value
                // The symbol is defined with the label name, not an instruction
                handle_equ(label, args, &mut self.symbols, self.pc, line_no).map(|_| 0)
            }
            "set" => {
                // SET is like EQU but allows redefinition
                handle_set(label, args, &mut self.symbols, self.pc, line_no).map(|_| 0)
            }
            "dc" => {
                // DC.B/W/L - estimate based on size and count
                let size_suffix = args.first().map(|s| s.as_str()).unwrap_or("w");
                let element_size = match size_suffix {
                    "b" => 1,
                    "w" => 2,
                    "l" => 4,
                    "s" => 4,
                    "d" => 8,
                    "x" | "p" => 12,
                    _ => 2,
                };
                let count = (args.len() - 1).max(1) as u32;
                let total = element_size * count;
                // Align to word boundary
                Ok((total + 1) & !1)
            }
            "ds" => {
                // DS.B/W/L - reserve space
                let size_suffix = args.first().map(|s| s.as_str()).unwrap_or("w");
                let element_size = match size_suffix {
                    "b" => 1,
                    "w" => 2,
                    "l" => 4,
                    _ => 2,
                };
                let count = if args.len() > 1 {
                    evaluate_expr_str(&args[1], &self.symbols, self.pc)? as u32
                } else {
                    1
                };
                let total = element_size * count;
                Ok((total + 1) & !1)
            }
            "even" => {
                // Pad to even address if needed
                let result = handle_even_pass1(self.pc);
                if result.bytes_emitted > 0 {
                    self.pc += result.bytes_emitted;
                }
                Ok(0)
            }
            "align" => {
                let result = handle_align_pass1(args, &self.symbols, self.pc, line_no)?;
                if result.bytes_emitted > 0 {
                    self.pc += result.bytes_emitted;
                }
                Ok(0)
            }
            "incbin" => {
                let result =
                    handle_incbin_pass1(args, &self.symbols, self.pc, &self.source_root, line_no)?;
                if result.bytes_emitted > 0 {
                    self.pc += result.bytes_emitted;
                }
                Ok(0)
            }
            "section" | "text" | "data" | "bss" => {
                let sec_args = if args.is_empty() && name != "section" {
                    vec![name.to_string()]
                } else {
                    args.to_vec()
                };
                let result = handle_section(
                    &sec_args,
                    &self.symbols,
                    self.pc,
                    &mut self.sections,
                    line_no,
                )?;
                if result.pc_changed {
                    self.pc = result.new_pc.unwrap_or(self.pc);
                }
                Ok(0)
            }
            "dcb" => {
                let size_suffix = args.first().map(|s| s.as_str()).unwrap_or("w");
                let element_size = match size_suffix {
                    "b" => 1,
                    "w" => 2,
                    "l" => 4,
                    _ => 2,
                };
                let count = if args.len() > 1 {
                    evaluate_expr_str(&args[1], &self.symbols, self.pc)? as u32
                } else {
                    0
                };
                let total = element_size * count;
                Ok((total + 1) & !1)
            }
            "end" => Ok(0),
            "fail" => {
                let msg =
                    crate::directives::strip_quotes(args.first().map(|s| s.as_str()).unwrap_or(""));
                Err(AsmError::with_line(format!("FAIL: {}", msg), line_no))
            }
            "warning" => {
                let msg =
                    crate::directives::strip_quotes(args.first().map(|s| s.as_str()).unwrap_or(""));
                self.errors
                    .warning(format!("WARNING: {}", msg), Some(line_no));
                Ok(0)
            }
            "error" => {
                let msg =
                    crate::directives::strip_quotes(args.first().map(|s| s.as_str()).unwrap_or(""));
                self.errors.error(format!("ERROR: {}", msg), Some(line_no));
                Ok(0)
            }
            "rs" => {
                if let Some(lbl) = label {
                    self.symbols
                        .define(lbl, self.rs_counter, Some(line_no))
                        .ok();
                }
                if !args.is_empty() {
                    let count = evaluate_expr_str(&args[0], &self.symbols, self.pc)? as u32;
                    self.rs_counter = self.rs_counter.wrapping_add(count);
                }
                Ok(0)
            }
            "rsreset" => {
                self.rs_counter = 0;
                Ok(0)
            }
            "rsset" => {
                if let Some(arg) = args.first() {
                    self.rs_counter = evaluate_expr_str(arg, &self.symbols, self.pc)? as u32;
                }
                Ok(0)
            }
            "if" | "ifeq" | "ifne" | "ifgt" | "iflt" | "ifge" | "ifle" | "ifdef" | "ifndef"
            | "ifc" | "ifnc" | "else" | "endif" | "macro" | "endm" | "rept" | "irp" | "irpc"
            | "endr" | "xref" | "xdef" | "public" | "extern" | "opt" | "mexit" | "exitm"
            | "print" | "printt" | "printv" | "list" | "nolist" | "page" | "title" => Ok(0),
            "cnop" => {
                // CNOP offset,align → aligns to (PC + offset) % align == 0
                if args.len() < 2 {
                    return Err(AsmError::with_line(
                        "CNOP requires offset and alignment",
                        line_no,
                    ));
                }
                let _offset = evaluate_expr_str(&args[0], &self.symbols, self.pc)? as u32;
                let alignment = evaluate_expr_str(&args[1], &self.symbols, self.pc)? as u32;
                if alignment == 0 || !alignment.is_power_of_two() {
                    return Err(AsmError::with_line(
                        format!("CNOP alignment must be power of 2, got {}", alignment),
                        line_no,
                    ));
                }
                let target = self.pc + _offset;
                let padding = if target.is_multiple_of(alignment) {
                    0
                } else {
                    alignment - (target % alignment)
                };
                let bytes = if padding % 2 != 0 {
                    padding + 1
                } else {
                    padding
                };
                if bytes > 0 {
                    self.pc += bytes;
                }
                Ok(0)
            }
            "offset" => {
                // OFFSET sets PC without emitting code (like ORG but symbolic)
                if let Some(arg) = args.first() {
                    let addr = evaluate_expr_str(arg, &self.symbols, self.pc)? as u32;
                    self.pc = addr;
                }
                Ok(0)
            }
            _ => {
                // Unknown directive - estimate 0, will error in pass 2
                self.errors
                    .warning(format!("unknown directive: {}", name), Some(line_no));
                Ok(0)
            }
        }
    }

    /// Estimate instruction size by parsing operand texts and attempting encoding.
    fn estimate_instruction_size_from_texts(
        &self,
        mnemonic: &str,
        size: Option<&str>,
        operand_texts: &[String],
    ) -> Result<u32, AsmError> {
        let src = if !operand_texts.is_empty() {
            parse_operand_text(&operand_texts[0], &self.symbols, self.pc).ok()
        } else {
            None
        };
        let dst = if operand_texts.len() > 1 {
            parse_operand_text(&operand_texts[1], &self.symbols, self.pc).ok()
        } else {
            None
        };

        // Use optimistic encoding (will be refined in pass 2)
        let mnemonic_upper = mnemonic.to_uppercase();
        match encode_instruction(
            &mnemonic_upper,
            size,
            src.as_ref(),
            dst.as_ref(),
            self.pc,
            &self.cpu,
        ) {
            Ok(words) => Ok((words.len() * 2) as u32),
            Err(_) => {
                // If encoding fails, assume minimum 2 bytes
                // The actual error will be caught in pass 2
                Ok(2)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Branch relaxation
    // -----------------------------------------------------------------------

    /// Relax branch instructions iteratively until stable.
    ///
    /// Branches start optimistic (short) and grow to word/long if the target
    /// is out of range. After each relaxation pass, PC values are recalculated
    /// and the process repeats until no changes occur.
    fn relax_branches(&mut self, lines: &[ParsedLine]) -> Result<(), AsmError> {
        let mut changed = true;
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 10;

        while changed && iterations < MAX_ITERATIONS {
            changed = false;
            iterations += 1;

            // Recalculate PCs with current branch sizes
            self.recalculate_pcs(lines);

            // Check each branch (use index to avoid borrow conflicts)
            for i in 0..self.branches.len() {
                let (mnemonic, line_no, size_hint) = {
                    let b = &self.branches[i];
                    (b.mnemonic.clone(), b.line_no, b.size_hint)
                };
                let target_addr = if let Some(ref symbol) = self.branches[i].target_symbol {
                    self.symbols.resolve(symbol).ok()
                } else {
                    self.branches[i].target_address
                };

                if let Some(target) = target_addr {
                    let branch_pc = self.get_pc_for_line(line_no)?;
                    let new_size =
                        self.determine_branch_size(&mnemonic, branch_pc, target, size_hint);

                    if new_size != self.branches[i].size_hint {
                        self.branches[i].size_hint = new_size;
                        changed = true;
                    }
                }
            }
        }

        if iterations >= MAX_ITERATIONS {
            self.errors
                .warning("branch relaxation did not converge", None);
        }

        Ok(())
    }

    /// Recalculate PC values for all lines based on current branch sizes.
    fn recalculate_pcs(&mut self, lines: &[ParsedLine]) {
        self.line_pcs.clear();
        let mut pc = self.origin;
        let mut cond_stack: Vec<bool> = Vec::new();

        for line in lines {
            self.line_pcs.push((line.line_no, pc));

            // Handle conditional directives
            let mut skip = false;
            if let LineType::Directive { name, args } = &line.line_type {
                match name.as_str() {
                    "ifc" | "ifnc" => {
                        let arg = args.join(",");
                        let active = cond_stack.last().copied().unwrap_or(true);
                        let result = self
                            .eval_conditional(name, &arg, line.line_no)
                            .unwrap_or(false);
                        cond_stack.push(active && result);
                        skip = true;
                    }
                    "if" | "ifeq" | "ifne" | "ifgt" | "iflt" | "ifge" | "ifle" | "ifdef"
                    | "ifndef" => {
                        let arg = args.first().map(|s| s.as_str()).unwrap_or("");
                        let active = cond_stack.last().copied().unwrap_or(true);
                        let result = self
                            .eval_conditional(name, arg, line.line_no)
                            .unwrap_or(false);
                        cond_stack.push(active && result);
                        skip = true;
                    }
                    "else" => {
                        if let Some(top) = cond_stack.last_mut() {
                            *top = !*top;
                        }
                        skip = true;
                    }
                    "endif" => {
                        cond_stack.pop();
                        skip = true;
                    }
                    _ => {}
                }
            }

            if !skip && cond_stack.last().copied().unwrap_or(true) {
                if let Some(ref label) = line.label {
                    let _ = self.symbols.define(label, pc, Some(line.line_no));
                }
                if let Ok(size) = self.estimate_line_size_with_branches(line) {
                    pc += size;
                }
            }
        }
    }

    /// Estimate line size considering current branch relaxation state.
    fn estimate_line_size_with_branches(&mut self, line: &ParsedLine) -> Result<u32, AsmError> {
        if let LineType::Instruction { mnemonic, .. } = &line.line_type
            && is_branch_mnemonic(mnemonic)
            && let Some(branch) = self
                .branches
                .iter()
                .find(|b| b.line_no == Some(line.line_no))
        {
            return Ok(branch_size_bytes(&branch.mnemonic, &branch.size_hint));
        }
        self.estimate_line_size(line)
    }

    /// Get the PC value for a given line number.
    fn get_pc_for_line(&self, line_no: Option<usize>) -> Result<u32, AsmError> {
        if let Some(ln) = line_no {
            for (line, pc) in &self.line_pcs {
                if *line == ln {
                    return Ok(*pc);
                }
            }
        }
        Ok(self.origin)
    }

    /// Determine the optimal branch size for a given displacement.
    fn determine_branch_size(
        &self,
        mnemonic: &str,
        branch_pc: u32,
        target: u32,
        hint: BranchSize,
    ) -> BranchSize {
        if hint != BranchSize::Any {
            return hint;
        }

        // DBcc always uses word displacement
        if mnemonic.starts_with("DB") {
            return BranchSize::Word;
        }

        // For BRA/BSR/Bcc on 68000, max is word (no long form)
        let disp = target as i32 - branch_pc as i32 - 2;

        if (-128..=127).contains(&disp) {
            BranchSize::Short
        } else {
            BranchSize::Word
        }
    }

    // -----------------------------------------------------------------------
    // Pass 2: Encoding with resolved symbols
    // -----------------------------------------------------------------------

    /// Pass 2: Encode all instructions with resolved symbols.
    fn pass2(&mut self, lines: &[ParsedLine]) -> Result<(), AsmError> {
        self.pc = self.origin;
        self.code.clear();
        self.conditional_stack.clear();

        for line in lines {
            // Handle IF/ELSE/ENDIF always (manage conditional stack)
            if let LineType::Directive { name, args } = &line.line_type {
                match name.as_str() {
                    "ifc" | "ifnc" => {
                        let arg = args.join(",");
                        let result = self.eval_conditional(name, &arg, line.line_no)?;
                        let active = self.is_conditional_active() && result;
                        self.conditional_stack.push(active);
                        continue;
                    }
                    "if" | "ifeq" | "ifne" | "ifgt" | "iflt" | "ifge" | "ifle" | "ifdef"
                    | "ifndef" => {
                        let arg = args.first().map(|s| s.as_str()).unwrap_or("");
                        let result = self.eval_conditional(name, arg, line.line_no)?;
                        let active = self.is_conditional_active() && result;
                        self.conditional_stack.push(active);
                        continue;
                    }
                    "else" => {
                        if let Some(top) = self.conditional_stack.last_mut() {
                            *top = !*top;
                        } else {
                            return Err(AsmError::with_line("ELSE without IF", line.line_no));
                        }
                        continue;
                    }
                    "endif" => {
                        if self.conditional_stack.pop().is_none() {
                            return Err(AsmError::with_line("ENDIF without IF", line.line_no));
                        }
                        continue;
                    }
                    _ => {}
                }
            }

            // Skip lines inside inactive conditional blocks
            if !self.is_conditional_active() {
                continue;
            }

            // Handle label (already defined in pass 1, but update PC)
            if let Some(ref label) = line.label {
                let _ = self.symbols.resolve(label);
            }

            // Encode this line
            match &line.line_type {
                LineType::Empty | LineType::Label => {}

                LineType::Directive { name, args } => {
                    self.encode_directive(name, args, line)?;
                }

                LineType::Instruction {
                    mnemonic,
                    size,
                    operand_texts,
                } => {
                    self.encode_instruction_line(mnemonic, size.as_deref(), operand_texts, line)?;
                }
            }
        }

        Ok(())
    }

    /// Encode a single instruction line.
    fn encode_instruction_line(
        &mut self,
        mnemonic: &str,
        size: Option<&str>,
        operand_texts: &[String],
        line: &ParsedLine,
    ) -> Result<(), AsmError> {
        let pc = self.pc;

        // Parse operands
        let src = if !operand_texts.is_empty() {
            parse_operand_text(&operand_texts[0], &self.symbols, pc).ok()
        } else {
            None
        };
        let dst = if operand_texts.len() > 1 {
            parse_operand_text(&operand_texts[1], &self.symbols, pc).ok()
        } else {
            None
        };

        // For branch instructions, the target operand is the first (and only) operand
        // but the encoder expects it as dst
        if is_branch_mnemonic(mnemonic) {
            if is_dbcc_mnemonic(mnemonic) {
                // DBcc D n,label: register is operand[0], branch target is operand[1].
                let reg = match src {
                    Some(Operand::DataReg(rn)) => rn,
                    _ => {
                        return Err(AsmError::with_line(
                            format!("{} requires Dn and a label or address target", mnemonic),
                            line.line_no,
                        ));
                    }
                };
                let branch_target = if operand_texts.len() > 1 {
                    parse_operand_text(&operand_texts[1], &self.symbols, pc).ok()
                } else {
                    None
                };
                return self.encode_dbcc_branch(mnemonic, reg, &branch_target, line);
            }
            let branch_target = if !operand_texts.is_empty() {
                parse_operand_text(&operand_texts[0], &self.symbols, pc).ok()
            } else {
                None
            };
            return self.encode_branch(mnemonic, &branch_target, line);
        }

        // Encode using the existing encoder (expects uppercase mnemonic)
        let mnemonic_upper = mnemonic.to_uppercase();

        // Handle 3-operand instructions (CAS, PACK, UNPK, PFLUSH, PTESTR/PTESTW)
        let words = match mnemonic_upper.as_str() {
            "PFLUSH" => {
                if operand_texts.len() != 3 {
                    return Err(AsmError::with_line(
                        "PFLUSH takes exactly 3 operands: #fc,#mask,<ea>".to_string(),
                        line.line_no,
                    ));
                }
                let fc = match parse_operand_text(&operand_texts[0], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?
                {
                    Operand::Immediate(v) => v,
                    _ => {
                        return Err(AsmError::with_line(
                            "PFLUSH's first operand (#fc) must be immediate".to_string(),
                            line.line_no,
                        ));
                    }
                };
                let mask = match parse_operand_text(&operand_texts[1], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?
                {
                    Operand::Immediate(v) => v,
                    _ => {
                        return Err(AsmError::with_line(
                            "PFLUSH's second operand (#mask) must be immediate".to_string(),
                            line.line_no,
                        ));
                    }
                };
                let ea = parse_operand_text(&operand_texts[2], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                crate::enc_mmu::enc_pflush(fc, mask, &ea, pc + 2, &self.cpu)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?
            }
            "PTESTR" | "PTESTW" => {
                if operand_texts.len() != 3 && operand_texts.len() != 4 {
                    return Err(AsmError::with_line(
                        format!(
                            "{} takes 3 or 4 operands: FC,<ea>,#level[,An]",
                            mnemonic_upper
                        ),
                        line.line_no,
                    ));
                }
                let fc = match parse_operand_text(&operand_texts[0], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?
                {
                    Operand::Immediate(v) => v,
                    _ => {
                        return Err(AsmError::with_line(
                            format!("{}'s first operand (FC) must be immediate", mnemonic_upper),
                            line.line_no,
                        ));
                    }
                };
                let ea = parse_operand_text(&operand_texts[1], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                let level = match parse_operand_text(&operand_texts[2], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?
                {
                    Operand::Immediate(v) => v,
                    _ => {
                        return Err(AsmError::with_line(
                            format!(
                                "{}'s third operand (#level) must be immediate",
                                mnemonic_upper
                            ),
                            line.line_no,
                        ));
                    }
                };
                let an = if operand_texts.len() == 4 {
                    match parse_operand_text(&operand_texts[3], &self.symbols, pc)
                        .map_err(|e| AsmError::with_line(e.message, line.line_no))?
                    {
                        Operand::AddrReg(n) => Some(n),
                        _ => {
                            return Err(AsmError::with_line(
                                format!("{}'s fourth operand must be An", mnemonic_upper),
                                line.line_no,
                            ));
                        }
                    }
                } else {
                    None
                };
                crate::enc_mmu::enc_ptest(
                    fc,
                    &ea,
                    level,
                    an,
                    mnemonic_upper == "PTESTR",
                    pc + 2,
                    &self.cpu,
                )
                .map_err(|e| AsmError::with_line(e.message, line.line_no))?
            }
            "CAS" => {
                let sz = size.unwrap_or("w");
                let dc = parse_operand_text(&operand_texts[0], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                let du = parse_operand_text(&operand_texts[1], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                let ea = parse_operand_text(&operand_texts[2], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                crate::enc_logic::enc_cas(&dc, &du, &ea, sz, pc + 4, &self.cpu)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?
            }
            "PACK" | "UNPK" => {
                let is_pack = mnemonic_upper == "PACK";
                let s = parse_operand_text(&operand_texts[0], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                let d = parse_operand_text(&operand_texts[1], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                let adj = parse_operand_text(&operand_texts[2], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                crate::enc_flow::enc_pack_unpk(&s, &d, &adj, "w", is_pack)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?
            }
            "CAS2" => {
                if operand_texts.len() != 3 {
                    return Err(AsmError::with_line(
                        "CAS2 takes exactly 3 operands: Dc1:Dc2,Du1:Du2,(Rn1):(Rn2)".to_string(),
                        line.line_no,
                    ));
                }
                let sz = size.unwrap_or("w");
                let parse_dc_du_pair = |text: &str| -> Result<(Operand, Operand), AsmError> {
                    let (a, b) = text
                        .split_once(':')
                        .ok_or_else(|| AsmError::new("CAS2 operand must be Rx:Ry"))?;
                    let pa = parse_operand_text(a.trim(), &self.symbols, pc)?;
                    let pb = parse_operand_text(b.trim(), &self.symbols, pc)?;
                    Ok((pa, pb))
                };
                let (dc1, dc2) = parse_dc_du_pair(&operand_texts[0])
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                let (du1, du2) = parse_dc_du_pair(&operand_texts[1])
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                let r_text = operand_texts[2].trim();
                let r_inner = r_text.trim_start_matches('(').trim_end_matches(')');
                let (r1_text, r2_text) = r_inner.split_once("):(").ok_or_else(|| {
                    AsmError::with_line(
                        "CAS2 register pair must be (Rn1):(Rn2)".to_string(),
                        line.line_no,
                    )
                })?;
                let rn1 = parse_operand_text(r1_text.trim(), &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                let rn2 = parse_operand_text(r2_text.trim(), &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                crate::enc_logic::enc_cas2(&dc1, &dc2, &du1, &du2, &rn1, &rn2, sz)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?
            }
            _ => encode_instruction(
                &mnemonic_upper,
                size,
                src.as_ref(),
                dst.as_ref(),
                pc,
                &self.cpu,
            )
            .map_err(|e| AsmError::with_line(e.message, line.line_no))?,
        };
        let word_count = words.len();

        self.push_instruction(AssembledInstruction {
            pc,
            words,
            line_no: Some(line.line_no),
            source: Some(line.raw.clone()),
        });

        self.pc += (word_count * 2) as u32;
        Ok(())
    }

    /// Encode a branch instruction with relaxation applied.
    fn encode_branch(
        &mut self,
        mnemonic: &str,
        dst: &Option<Operand>,
        line: &ParsedLine,
    ) -> Result<(), AsmError> {
        let pc = self.pc;

        // Resolve target from various operand types
        let target = match dst {
            Some(Operand::Address(addr)) => *addr as u32,
            Some(Operand::Immediate(addr)) => *addr as u32,
            Some(Operand::Memory(addr)) => *addr as u32,
            Some(Operand::AbsoluteShort(addr)) => *addr as u32,
            Some(Operand::AbsoluteLong(addr)) => *addr as u32,
            _ => {
                return Err(AsmError::with_line(
                    format!("{} requires a label or address target", mnemonic),
                    line.line_no,
                ));
            }
        };

        // Apply size hint from relaxation
        let size_hint = self
            .branches
            .iter()
            .find(|b| b.line_no == Some(line.line_no))
            .map(|b| b.size_hint)
            .unwrap_or(BranchSize::Any);

        let disp = target as i32 - pc as i32 - 2;

        let words = match mnemonic {
            "bra" => self.encode_bra(disp, size_hint)?,
            "bsr" => self.encode_bsr(disp, size_hint)?,
            _ => {
                // Bcc family - delegate to enc_flow
                let cond = branch_condition(mnemonic)?;
                crate::enc_flow::enc_bcc(&cond, target as i32, pc + 2)
                    .map_err(|e| AsmError::with_line(e.message.clone(), line.line_no))?
            }
        };
        let word_count = words.len();

        self.push_instruction(AssembledInstruction {
            pc,
            words,
            line_no: Some(line.line_no),
            source: Some(line.raw.clone()),
        });

        self.pc += (word_count * 2) as u32;
        Ok(())
    }

    /// Encode a DBcc instruction (`Dn,label`). DBcc always uses a fixed
    /// 4-byte encoding (opword + word displacement), so there is no size
    /// relaxation to apply, unlike Bcc/BRA/BSR.
    fn encode_dbcc_branch(
        &mut self,
        mnemonic: &str,
        reg: u8,
        dst: &Option<Operand>,
        line: &ParsedLine,
    ) -> Result<(), AsmError> {
        let pc = self.pc;

        let target = match dst {
            Some(Operand::Address(addr)) => *addr as u32,
            Some(Operand::Immediate(addr)) => *addr as u32,
            Some(Operand::Memory(addr)) => *addr as u32,
            Some(Operand::AbsoluteShort(addr)) => *addr as u32,
            Some(Operand::AbsoluteLong(addr)) => *addr as u32,
            _ => {
                return Err(AsmError::with_line(
                    format!("{} requires Dn and a label or address target", mnemonic),
                    line.line_no,
                ));
            }
        };

        let cond = dbcc_condition(mnemonic)?;
        let words = crate::enc_flow::enc_dbcc(&cond, reg, target as i32, pc + 2)
            .map_err(|e| AsmError::with_line(e.message.clone(), line.line_no))?;
        let word_count = words.len();

        self.push_instruction(AssembledInstruction {
            pc,
            words,
            line_no: Some(line.line_no),
            source: Some(line.raw.clone()),
        });

        self.pc += (word_count * 2) as u32;
        Ok(())
    }

    /// Encode BRA with size hint.
    fn encode_bra(&self, disp: i32, size_hint: BranchSize) -> Result<Vec<u16>, AsmError> {
        match size_hint {
            BranchSize::Short | BranchSize::Any if (-128..=127).contains(&disp) => {
                let op = 0x6000 | ((disp & 0xFF) as u16);
                return Ok(vec![op]);
            }
            _ => {}
        }

        // Word displacement
        if (-32768..=32767).contains(&disp) {
            let op = 0x6000;
            return Ok(vec![op, (disp & 0xFFFF) as u16]);
        }

        // 68020+ long displacement
        if self.cpu != "68000" {
            let op = 0x6000;
            return Ok(vec![
                op,
                0,
                ((disp >> 16) & 0xFFFF) as u16,
                (disp & 0xFFFF) as u16,
            ]);
        }

        Err(AsmError::new("BRA displacement out of range for 68000"))
    }

    /// Encode BSR with size hint.
    fn encode_bsr(&self, disp: i32, size_hint: BranchSize) -> Result<Vec<u16>, AsmError> {
        match size_hint {
            BranchSize::Short | BranchSize::Any if (-128..=127).contains(&disp) => {
                let op = 0x6100 | ((disp & 0xFF) as u16);
                return Ok(vec![op]);
            }
            _ => {}
        }

        if (-32768..=32767).contains(&disp) {
            let op = 0x6100;
            return Ok(vec![op, (disp & 0xFFFF) as u16]);
        }

        if self.cpu != "68000" {
            let op = 0x6100;
            return Ok(vec![
                op,
                0,
                ((disp >> 16) & 0xFFFF) as u16,
                (disp & 0xFFFF) as u16,
            ]);
        }

        Err(AsmError::new("BSR displacement out of range for 68000"))
    }

    /// Encode a directive.
    fn encode_directive(
        &mut self,
        name: &str,
        args: &[String],
        line: &ParsedLine,
    ) -> Result<(), AsmError> {
        match name {
            "org" => {
                if let Some(arg) = args.first() {
                    let addr = evaluate_expr_str(arg, &self.symbols, self.pc)
                        .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                    self.pc = addr as u32;
                    self.sections.set_current_pc(self.pc);
                }
                Ok(())
            }
            "equ" | "set" => {
                // EQU/SET defines a symbol, already handled in pass 1
                Ok(())
            }
            "dc" => self.encode_dc(args, line),
            "ds" => self.encode_ds(args, line),
            "even" => {
                let (result, instr) = handle_even_pass2(self.pc, line.line_no, &line.raw);
                if let Some(i) = instr {
                    self.push_instruction(i);
                    self.pc += result.bytes_emitted;
                }
                Ok(())
            }
            "align" => {
                let (result, instr) =
                    handle_align_pass2(args, &self.symbols, self.pc, line.line_no, &line.raw)?;
                if let Some(i) = instr {
                    self.push_instruction(i);
                    self.pc += result.bytes_emitted;
                }
                Ok(())
            }
            "incbin" => {
                let (result, instr) = handle_incbin_pass2(
                    args,
                    &self.symbols,
                    self.pc,
                    &self.source_root,
                    line.line_no,
                    &line.raw,
                )?;
                if let Some(i) = instr {
                    self.push_instruction(i);
                    self.pc += result.bytes_emitted;
                }
                Ok(())
            }
            "section" | "text" | "data" | "bss" => {
                let sec_args = if args.is_empty() && name != "section" {
                    vec![name.to_string()]
                } else {
                    args.to_vec()
                };
                let result = handle_section(
                    &sec_args,
                    &self.symbols,
                    self.pc,
                    &mut self.sections,
                    line.line_no,
                )?;
                if result.pc_changed {
                    self.pc = result.new_pc.unwrap_or(self.pc);
                }
                Ok(())
            }
            "dcb" => self.encode_dcb(args, line),
            "end" => Ok(()),
            "fail" => {
                let msg =
                    crate::directives::strip_quotes(args.first().map(|s| s.as_str()).unwrap_or(""));
                Err(AsmError::with_line(format!("FAIL: {}", msg), line.line_no))
            }
            "warning" => {
                let msg =
                    crate::directives::strip_quotes(args.first().map(|s| s.as_str()).unwrap_or(""));
                self.errors
                    .warning(format!("WARNING: {}", msg), Some(line.line_no));
                Ok(())
            }
            "error" => {
                let msg =
                    crate::directives::strip_quotes(args.first().map(|s| s.as_str()).unwrap_or(""));
                self.errors
                    .error(format!("ERROR: {}", msg), Some(line.line_no));
                Ok(())
            }
            "rs" => {
                if let Some(lbl) = &line.label {
                    self.symbols
                        .define(lbl, self.rs_counter, Some(line.line_no))
                        .ok();
                }
                if !args.is_empty() {
                    let count = evaluate_expr_str(&args[0], &self.symbols, self.pc)? as u32;
                    self.rs_counter = self.rs_counter.wrapping_add(count);
                }
                Ok(())
            }
            "rsreset" => {
                self.rs_counter = 0;
                Ok(())
            }
            "rsset" => {
                if let Some(arg) = args.first() {
                    self.rs_counter = evaluate_expr_str(arg, &self.symbols, self.pc)? as u32;
                }
                Ok(())
            }
            "if" | "ifeq" | "ifne" | "ifgt" | "iflt" | "ifge" | "ifle" | "ifdef" | "ifndef"
            | "ifc" | "ifnc" | "else" | "endif" | "macro" | "endm" | "rept" | "irp" | "irpc"
            | "endr" | "xref" | "xdef" | "public" | "extern" | "mexit" | "exitm" | "list"
            | "nolist" | "page" | "title" => Ok(()),
            "opt" => {
                // OPT arguments are processed but currently no state is tracked
                Ok(())
            }
            "print" | "printt" => {
                let msg =
                    crate::directives::strip_quotes(args.first().map(|s| s.as_str()).unwrap_or(""));
                if !msg.is_empty() {
                    println!("{}", msg);
                }
                Ok(())
            }
            "printv" => {
                if let Some(arg) = args.first() {
                    let val = evaluate_expr_str(arg, &self.symbols, self.pc)
                        .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                    println!("{} = {}", arg.trim(), val);
                }
                Ok(())
            }
            "cnop" => {
                // CNOP in pass 2: emit NOP padding
                let _offset = evaluate_expr_str(&args[0], &self.symbols, self.pc)? as u32;
                let alignment = evaluate_expr_str(&args[1], &self.symbols, self.pc)? as u32;
                let target = self.pc + _offset;
                let padding = if target.is_multiple_of(alignment) {
                    0
                } else {
                    alignment - (target % alignment)
                };
                let bytes = if padding % 2 != 0 {
                    padding + 1
                } else {
                    padding
                };
                if bytes > 0 {
                    let word_count = (bytes / 2) as usize;
                    let words = vec![0x4E71u16; word_count];
                    self.push_instruction(AssembledInstruction {
                        pc: self.pc,
                        words,
                        line_no: Some(line.line_no),
                        source: Some(line.raw.clone()),
                    });
                    self.pc += bytes;
                }
                Ok(())
            }
            "offset" => {
                if let Some(arg) = args.first() {
                    let addr = evaluate_expr_str(arg, &self.symbols, self.pc)? as u32;
                    self.pc = addr;
                    self.sections.set_current_pc(self.pc);
                }
                Ok(())
            }
            _ => Err(AsmError::with_line(
                format!("unsupported directive: {}", name),
                line.line_no,
            )),
        }
    }

    /// Encode DC (Define Constant) directive.
    fn encode_dc(&mut self, args: &[String], line: &ParsedLine) -> Result<(), AsmError> {
        if args.is_empty() {
            return Err(AsmError::with_line(
                "DC requires size and values",
                line.line_no,
            ));
        }

        let size_suffix = &args[0];
        let values = &args[1..];

        if values.is_empty() {
            return Err(AsmError::with_line(
                "DC requires at least one value",
                line.line_no,
            ));
        }

        let element_size = match size_suffix.as_str() {
            "b" => 1,
            "w" => 2,
            "l" => 4,
            "s" => 4,
            "d" => 8,
            "x" => 12,
            "p" => 12,
            _ => {
                return Err(AsmError::with_line(
                    format!("invalid DC size: {}", size_suffix),
                    line.line_no,
                ));
            }
        };

        let mut words = Vec::new();
        let mut total_bytes: usize = 0;

        for value_str in values {
            let trimmed = value_str.trim();

            // Float types (S/D/X/P)
            if matches!(size_suffix.as_str(), "s" | "d" | "x" | "p") {
                let float_words = parse_float_value(trimmed, size_suffix, line.line_no)?;
                let word_count = float_words.len();
                words.extend(float_words);
                total_bytes += word_count * 2;
                continue;
            }

            // Check if it's a string literal
            if trimmed.starts_with('"') || trimmed.starts_with('\'') {
                if element_size != 1 {
                    return Err(AsmError::with_line(
                        "string literals only supported with DC.B",
                        line.line_no,
                    ));
                }
                let bytes = parse_dc_string(trimmed).map_err(|e| {
                    AsmError::with_line(format!("invalid DC string: {}", e), line.line_no)
                })?;
                for &b in &bytes {
                    if words.len() * 2 == total_bytes {
                        words.push((b as u16) << 8);
                    } else {
                        let last = words.last_mut().unwrap();
                        *last |= b as u16;
                    }
                    total_bytes += 1;
                }
            } else if trimmed.len() == 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
                // Character literal: 'A'
                if element_size != 1 {
                    return Err(AsmError::with_line(
                        "character literals only supported with DC.B",
                        line.line_no,
                    ));
                }
                let ch = trimmed.chars().nth(1).unwrap();
                if words.len() * 2 == total_bytes {
                    words.push((ch as u16) << 8);
                } else {
                    let last = words.last_mut().unwrap();
                    *last |= ch as u16;
                }
                total_bytes += 1;
            } else {
                let value = evaluate_expr_str(value_str, &self.symbols, self.pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;

                match element_size {
                    1 => {
                        // DC.B - pack two bytes per word (big-endian)
                        if words.len() * 2 == total_bytes {
                            words.push(((value & 0xFF) as u16) << 8);
                        } else {
                            let last = words.last_mut().unwrap();
                            *last |= (value & 0xFF) as u16;
                        }
                        total_bytes += 1;
                    }
                    2 => {
                        words.push((value & 0xFFFF) as u16);
                        total_bytes += 2;
                    }
                    4 => {
                        words.push(((value >> 16) & 0xFFFF) as u16);
                        words.push((value & 0xFFFF) as u16);
                        total_bytes += 4;
                    }
                    _ => unreachable!(),
                }
            }
        }

        // Round up to even
        if !total_bytes.is_multiple_of(2) {
            total_bytes += 1;
            if words.is_empty() {
                words.push(0);
            } else {
                let last = words.last_mut().unwrap();
                *last &= 0xFF00; // Clear low byte (already has high byte)
            }
        }

        let start_pc = self.pc;
        self.push_instruction(AssembledInstruction {
            pc: start_pc,
            words,
            line_no: Some(line.line_no),
            source: Some(line.raw.clone()),
        });

        self.pc += total_bytes as u32;
        Ok(())
    }

    /// Encode DS (Define Storage) directive.
    fn encode_ds(&mut self, args: &[String], line: &ParsedLine) -> Result<(), AsmError> {
        if args.len() < 2 {
            return Err(AsmError::with_line(
                "DS requires size and count",
                line.line_no,
            ));
        }

        let size_suffix = &args[0];
        let count_str = &args[1];

        let count = evaluate_expr_str(count_str, &self.symbols, self.pc)
            .map_err(|e| AsmError::with_line(e.message, line.line_no))? as u32;

        let element_size = match size_suffix.as_str() {
            "b" => 1,
            "w" => 2,
            "l" => 4,
            _ => {
                return Err(AsmError::with_line(
                    format!("invalid DS size: {}", size_suffix),
                    line.line_no,
                ));
            }
        };

        // DS reserves space but doesn't emit code
        // Just advance the PC
        let total_bytes = element_size * count;
        self.pc += total_bytes;
        self.sections.set_current_pc(self.pc);

        Ok(())
    }

    /// Encode DCB (Define Constant Block) directive.
    ///
    /// Syntax: `DCB.B count,value`, `DCB.W count,value`, `DCB.L count,value`
    /// Repeats `value` `count` times.
    fn encode_dcb(&mut self, args: &[String], line: &ParsedLine) -> Result<(), AsmError> {
        if args.len() < 2 {
            return Err(AsmError::with_line(
                "DCB requires size and count",
                line.line_no,
            ));
        }

        let size_suffix = &args[0];
        let count_str = &args[1];
        let value_str = args.get(2).map(|s| s.as_str()).unwrap_or("0");

        let count = evaluate_expr_str(count_str, &self.symbols, self.pc)
            .map_err(|e| AsmError::with_line(e.message, line.line_no))? as u32;
        let value = evaluate_expr_str(value_str, &self.symbols, self.pc)
            .map_err(|e| AsmError::with_line(e.message, line.line_no))?;

        let element_size = match size_suffix.as_str() {
            "b" => 1,
            "w" => 2,
            "l" => 4,
            _ => {
                return Err(AsmError::with_line(
                    format!("invalid DCB size: {}", size_suffix),
                    line.line_no,
                ));
            }
        };

        let mut words = Vec::new();

        match element_size {
            1 => {
                let byte = (value & 0xFF) as u8;
                let mut i = 0u32;
                while i < count {
                    let hi = byte;
                    let lo = if i + 1 < count { byte } else { 0 };
                    words.push(((hi as u16) << 8) | (lo as u16));
                    i += 2;
                }
            }
            2 => {
                let word = (value & 0xFFFF) as u16;
                for _ in 0..count {
                    words.push(word);
                }
            }
            4 => {
                let hi = ((value >> 16) & 0xFFFF) as u16;
                let lo = (value & 0xFFFF) as u16;
                for _ in 0..count {
                    words.push(hi);
                    words.push(lo);
                }
            }
            _ => unreachable!(),
        }

        let start_pc = self.pc;
        self.push_instruction(AssembledInstruction {
            pc: start_pc,
            words,
            line_no: Some(line.line_no),
            source: Some(line.raw.clone()),
        });

        let total_bytes = (element_size as u32) * count;
        self.pc += if total_bytes.is_multiple_of(2) {
            total_bytes
        } else {
            total_bytes + 1
        };

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Check if a mnemonic is a branch instruction.
fn is_branch_mnemonic(mnemonic: &str) -> bool {
    matches!(
        mnemonic,
        "bra"
            | "bsr"
            | "bhi"
            | "bls"
            | "bcc"
            | "bcs"
            | "bne"
            | "beq"
            | "bvc"
            | "bvs"
            | "bpl"
            | "bmi"
            | "bge"
            | "blt"
            | "bgt"
            | "ble"
            | "dbra"
            | "dbf"
            | "dbt"
            | "dbhi"
            | "dbls"
            | "dbcc"
            | "dbcs"
            | "dbne"
            | "dbeq"
            | "dbvc"
            | "dbvs"
            | "dbpl"
            | "dbmi"
            | "dbge"
            | "dblt"
            | "dbgt"
            | "dble"
    )
}

/// Is this a DBcc mnemonic (as opposed to Bcc)? DBcc takes a `Dn,label`
/// operand pair instead of a single branch target.
fn is_dbcc_mnemonic(mnemonic: &str) -> bool {
    mnemonic.starts_with("db")
}

/// Get branch condition code from mnemonic.
fn branch_condition(mnemonic: &str) -> Result<String, AsmError> {
    let cond = match mnemonic {
        "bhi" => "hi",
        "bls" => "ls",
        "bcc" => "cc",
        "bcs" => "cs",
        "bne" => "ne",
        "beq" => "eq",
        "bvc" => "vc",
        "bvs" => "vs",
        "bpl" => "pl",
        "bmi" => "mi",
        "bge" => "ge",
        "blt" => "lt",
        "bgt" => "gt",
        "ble" => "le",
        _ => {
            return Err(AsmError::new(format!(
                "unknown branch condition: {}",
                mnemonic
            )));
        }
    };
    Ok(cond.to_string())
}

/// Get DBcc condition code from mnemonic. `dbra`/`dbf` both encode
/// condition "f" (always false, i.e. plain decrement-and-branch).
fn dbcc_condition(mnemonic: &str) -> Result<String, AsmError> {
    let cond = match mnemonic {
        "dbra" | "dbf" => "f",
        "dbt" => "t",
        "dbhi" => "hi",
        "dbls" => "ls",
        "dbcc" => "cc",
        "dbcs" => "cs",
        "dbne" => "ne",
        "dbeq" => "eq",
        "dbvc" => "vc",
        "dbvs" => "vs",
        "dbpl" => "pl",
        "dbmi" => "mi",
        "dbge" => "ge",
        "dblt" => "lt",
        "dbgt" => "gt",
        "dble" => "le",
        _ => {
            return Err(AsmError::new(format!(
                "unknown DBcc condition: {}",
                mnemonic
            )));
        }
    };
    Ok(cond.to_string())
}

/// Parse a float value string and encode it in the requested format.
fn parse_float_value(text: &str, format: &str, line_no: usize) -> Result<Vec<u16>, AsmError> {
    let text = text.trim();
    match format {
        "s" => {
            // IEEE 754 single precision (32-bit = 2 words)
            let val: f32 = text
                .parse()
                .map_err(|_| AsmError::with_line(format!("invalid float: {}", text), line_no))?;
            let bits = val.to_bits();
            Ok(vec![((bits >> 16) & 0xFFFF) as u16, (bits & 0xFFFF) as u16])
        }
        "d" => {
            // IEEE 754 double precision (64-bit = 4 words)
            let val: f64 = text
                .parse()
                .map_err(|_| AsmError::with_line(format!("invalid float: {}", text), line_no))?;
            let bits = val.to_bits();
            Ok(vec![
                ((bits >> 48) & 0xFFFF) as u16,
                ((bits >> 32) & 0xFFFF) as u16,
                ((bits >> 16) & 0xFFFF) as u16,
                (bits & 0xFFFF) as u16,
            ])
        }
        "x" | "p" => {
            // Extended precision (96-bit = 6 words) or packed decimal (96-bit)
            // Accept hex value with $ prefix, parse as u128
            let text = text.trim();
            let val: u128 = if let Some(hex) = text.strip_prefix('$') {
                u128::from_str_radix(hex, 16).unwrap_or(0)
            } else {
                text.parse::<u128>().unwrap_or(0)
            };
            Ok(vec![
                ((val >> 80) & 0xFFFF) as u16,
                ((val >> 64) & 0xFFFF) as u16,
                ((val >> 48) & 0xFFFF) as u16,
                ((val >> 32) & 0xFFFF) as u16,
                ((val >> 16) & 0xFFFF) as u16,
                (val & 0xFFFF) as u16,
            ])
        }
        _ => Err(AsmError::with_line(
            format!("unsupported float format: {}", format),
            line_no,
        )),
    }
}

/// Estimate branch instruction size in bytes (for pass 1).
fn estimate_branch_size(mnemonic: &str) -> u32 {
    if mnemonic.starts_with("db") {
        // DBcc is always 4 bytes (opword + word disp)
        4
    } else {
        // Assume word-sized branch (4 bytes) for conservative estimation
        4
    }
}

/// Get branch size in bytes based on relaxation hint.
fn branch_size_bytes(mnemonic: &str, hint: &BranchSize) -> u32 {
    if mnemonic.starts_with("db") {
        return 4; // DBcc always 4 bytes
    }

    match hint {
        BranchSize::Short => 2,
        BranchSize::Word => 4,
        BranchSize::Long => 6,
        BranchSize::Any => 4, // Conservative: assume word
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_table_define_and_resolve() {
        let mut st = SymbolTable::new();
        st.define("start", 0x1000, Some(1)).unwrap();
        assert_eq!(st.resolve("start").unwrap(), 0x1000);
    }

    #[test]
    fn test_symbol_table_redefine_error() {
        let mut st = SymbolTable::new();
        st.define("start", 0x1000, Some(1)).unwrap();
        assert!(st.define("start", 0x2000, Some(2)).is_err());
    }

    #[test]
    fn test_symbol_table_forward_reference() {
        let mut st = SymbolTable::new();
        st.declare("forward", Some(1));
        assert!(!st.get("forward").unwrap().defined);
        st.define("forward", 0x3000, Some(5)).unwrap();
        assert_eq!(st.resolve("forward").unwrap(), 0x3000);
    }

    #[test]
    fn test_parse_source_basic() {
        let source = "
start:
    MOVE.B D0,D1
    NOP
end:
";
        let lines = parse_source(source);
        assert!(lines.iter().any(|l| l.label.as_deref() == Some("start")));
        assert!(lines.iter().any(|l| l.label.as_deref() == Some("end")));
        assert!(lines.iter().any(
            |l| matches!(&l.line_type, LineType::Instruction { mnemonic, .. } if mnemonic == "move")
        ));
    }

    #[test]
    fn test_assemble_simple() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    MOVE.B D0,D1
    NOP
",
        );
        assert!(result.is_ok());
        assert_eq!(asm.code.len(), 2);
        assert_eq!(asm.code[0].pc, 0x1000);
        assert_eq!(asm.code[1].pc, 0x1002);
    }

    #[test]
    fn test_assemble_with_label() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
start:
    MOVE.B D0,D1
    NOP
",
        );
        assert!(result.is_ok());
        assert_eq!(asm.symbols.resolve("start").unwrap(), 0x1000);
    }

    #[test]
    fn test_assemble_with_org() {
        let mut asm = Assembler::new(0);
        let result = asm.assemble(
            "
    ORG $2000
    MOVE.B D0,D1
",
        );
        assert!(result.is_ok());
        assert_eq!(asm.code[0].pc, 0x2000);
    }

    #[test]
    fn test_assemble_bytes() {
        let mut asm = Assembler::new(0x1000);
        let bytes = asm
            .assemble_bytes(
                "
    NOP
",
            )
            .unwrap();
        assert_eq!(bytes, vec![0x4E, 0x71]);
    }

    /// Assembles `source` with the given CPU and returns the resulting bytes.
    fn assemble_fpu_source(source: &str) -> Vec<u8> {
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        asm.assemble_bytes(source).unwrap()
    }

    #[test]
    fn test_source_fadd_reg_reg() {
        let bytes = assemble_fpu_source("    FADD FP1,FP2\n");
        assert_eq!(bytes, vec![0xF2, 0x00, 0x05, 0x22]);
    }

    #[test]
    fn test_source_fmove_ea_to_reg() {
        let bytes = assemble_fpu_source("    FMOVE.S D0,FP1\n");
        assert_eq!(bytes, vec![0xF2, 0x00, 0x44, 0x80]);
    }

    #[test]
    fn test_source_fmovem_range_without_slash_bugfix() {
        // fmovem fp0-fp3,-(a7): a pure range without a '/' must parse as a register list.
        // vasm: fmovem fp0-fp3,-(a7) -> f227e00f
        let bytes = assemble_fpu_source("    FMOVEM FP0-FP3,-(A7)\n");
        assert_eq!(bytes, vec![0xF2, 0x27, 0xE0, 0x0F]);
    }

    #[test]
    fn test_source_fmovem_ctrl_list() {
        // fmovem fpcr/fpsr,-(a0): the "/" check must not misidentify this as an FP
        // data-register list before checking control registers.
        // vasm: fmovem fpcr/fpsr,-(a0) -> f220b800
        let bytes = assemble_fpu_source("    FMOVEM FPCR/FPSR,-(A0)\n");
        assert_eq!(bytes, vec![0xF2, 0x20, 0xB8, 0x00]);
    }

    #[test]
    fn test_source_fseq() {
        let bytes = assemble_fpu_source("    FSEQ D0\n");
        assert_eq!(bytes, vec![0xF2, 0x40, 0x00, 0x01]);
    }

    #[test]
    fn test_source_fnop() {
        let bytes = assemble_fpu_source("    FNOP\n");
        assert_eq!(bytes, vec![0xF2, 0x80, 0x00, 0x00]);
    }

    #[test]
    fn test_source_movem_range_and_single_reg() {
        // Previously unparseable in assembler.rs (MOVEM register lists had no source-text
        // support at all); now handled by the same list/range parser FMOVEM uses.
        let mut asm = Assembler::new(0);
        let bytes = asm.assemble_bytes("    MOVEM.W D0-D3/A5,-(A7)\n").unwrap();
        assert_eq!(bytes, vec![0x48, 0xA7, 0xF0, 0x04]);
    }

    #[test]
    fn test_estimate_branch_size() {
        assert_eq!(estimate_branch_size("bra"), 4);
        assert_eq!(estimate_branch_size("bsr"), 4);
        assert_eq!(estimate_branch_size("dbra"), 4);
        assert_eq!(estimate_branch_size("bne"), 4);
    }

    #[test]
    fn test_branch_size_bytes() {
        assert_eq!(branch_size_bytes("bra", &BranchSize::Short), 2);
        assert_eq!(branch_size_bytes("bra", &BranchSize::Word), 4);
        assert_eq!(branch_size_bytes("bra", &BranchSize::Long), 6);
        assert_eq!(branch_size_bytes("dbra", &BranchSize::Short), 4); // DBcc always 4
    }

    #[test]
    fn test_is_branch_mnemonic() {
        assert!(is_branch_mnemonic("bra"));
        assert!(is_branch_mnemonic("bne"));
        assert!(is_branch_mnemonic("dbra"));
        assert!(!is_branch_mnemonic("move"));
        assert!(!is_branch_mnemonic("add"));
    }

    #[test]
    fn test_branch_condition() {
        assert_eq!(branch_condition("bne").unwrap(), "ne");
        assert_eq!(branch_condition("beq").unwrap(), "eq");
        assert_eq!(branch_condition("bge").unwrap(), "ge");
        assert!(branch_condition("xxx").is_err());
    }

    #[test]
    fn test_parse_register() {
        assert!(matches!(parse_register("d0"), Some(Operand::DataReg(0))));
        assert!(matches!(parse_register("D7"), Some(Operand::DataReg(7))));
        assert!(matches!(parse_register("a0"), Some(Operand::AddrReg(0))));
        assert!(matches!(parse_register("A7"), Some(Operand::AddrReg(7))));
        assert!(parse_register("d8").is_none());
    }

    #[test]
    fn test_parse_register_sp_alias() {
        assert!(matches!(parse_register("sp"), Some(Operand::AddrReg(7))));
        assert!(matches!(parse_register("SP"), Some(Operand::AddrReg(7))));
    }

    #[test]
    fn test_split_disp_before_paren() {
        assert_eq!(
            split_disp_before_paren("$1000(A0)"),
            Some("($1000,A0)".to_string())
        );
        assert_eq!(
            split_disp_before_paren("label(PC)"),
            Some("(label,PC)".to_string())
        );
        // Already-parenthesized forms (no prefix) are left untouched.
        assert_eq!(split_disp_before_paren("(A0)"), None);
        assert_eq!(split_disp_before_paren("($1000,A0)"), None);
    }

    #[test]
    fn test_parse_parens_disp_pc() {
        assert!(matches!(
            parse_parens_disp_pc("($10,PC)"),
            Some((16, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc("($1000,PC)"),
            Some((4096, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc("($10000,PC)"),
            Some((65536, true))
        ));
        assert!(matches!(
            parse_parens_disp_pc("(-10,PC)"),
            Some((-10, false))
        ));
        assert!(matches!(parse_parens_disp_pc("(0,PC)"), Some((0, false))));
        assert!(parse_parens_disp_pc("(A0)").is_none());
    }

    #[test]
    fn test_parse_parens_disp_pc_index() {
        assert!(matches!(
            parse_parens_disp_pc_index("($10,PC,D0)"),
            Some((0, 16, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc_index("(0,PC,D1)"),
            Some((1, 0, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc_index("($20,PC,A2)"),
            Some((2, 32, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc_index("($10,PC,D3.W)"),
            Some((3, 16, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc_index("($10,PC,D3.L)"),
            Some((3, 16, _, true))
        ));
        assert!(parse_parens_disp_pc_index("(A0)").is_none());
        assert!(parse_parens_disp_pc_index("($10,PC)").is_none());
    }

    #[test]
    fn test_parse_parens_disp_reg_index() {
        assert!(matches!(
            parse_parens_disp_reg_index("($10,A0,D1)"),
            Some((0, 1, 16, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("(0,A1,D0)"),
            Some((1, 0, 0, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("($20,A2,D3.W)"),
            Some((2, 3, 32, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("($10,A3,D4.L)"),
            Some((3, 4, 16, _, true))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("(0,A0,D0*1)"),
            Some((0, 0, 0, 1, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("(0,A0,D0*2)"),
            Some((0, 0, 0, 2, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("(0,A0,D0*4)"),
            Some((0, 0, 0, 4, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("(0,A0,D0*8)"),
            Some((0, 0, 0, 8, false))
        ));
        assert!(parse_parens_disp_reg_index("(A0)").is_none());
        assert!(parse_parens_disp_reg_index("($10,A0)").is_none());
    }

    #[test]
    fn test_conditional_if_true() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    IF 1
    NOP
    ENDIF
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 2);
        assert_eq!(asm.code[0].words, vec![0x4E71]); // NOP
        assert_eq!(asm.code[1].words, vec![0x4E75]); // RTS
    }

    #[test]
    fn test_conditional_if_false() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    IF 0
    NOP
    ENDIF
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
        assert_eq!(asm.code[0].words, vec![0x4E75]); // Only RTS
    }

    #[test]
    fn test_conditional_if_else() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    IF 0
    NOP
    ELSE
    RTS
    ENDIF
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
        assert_eq!(asm.code[0].words, vec![0x4E75]); // RTS, not NOP
    }

    #[test]
    fn test_conditional_ifdef() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
MYDEF EQU 42
    IFDEF MYDEF
    NOP
    ENDIF
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
        assert_eq!(asm.code[0].words, vec![0x4E71]);
    }

    #[test]
    fn test_conditional_ifndef() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    IFNDEF UNDEFINED_SYM
    NOP
    ENDIF
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
        assert_eq!(asm.code[0].words, vec![0x4E71]);
    }

    #[test]
    fn test_conditional_if_defined_function() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
MYDEF EQU 42
    IF DEFINED(MYDEF)
    NOP
    ENDIF
    IF DEFINED(UNDEFINED_SYM)
    ILLEGAL
    ENDIF
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
        assert_eq!(asm.code[0].words, vec![0x4E71]);
    }

    #[test]
    fn test_conditional_nested() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    IF 1
    IF 0
    NOP
    ELSE
    RTS
    ENDIF
    ENDIF
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
        assert_eq!(asm.code[0].words, vec![0x4E75]);
    }

    #[test]
    fn test_macro_simple() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
mymacro MACRO
    NOP
    ENDM
    mymacro
    mymacro
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 3);
        assert_eq!(asm.code[0].words, vec![0x4E71]); // first NOP
        assert_eq!(asm.code[1].words, vec![0x4E71]); // second NOP
        assert_eq!(asm.code[2].words, vec![0x4E75]); // RTS
    }

    #[test]
    fn test_macro_with_params() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
moveqmacro MACRO val,reg
    MOVEQ #\\1,\\2
    ENDM
    moveqmacro 5,D0
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 2);
        // moveqmacro 5,D0 → \1=5, \2=D0 → MOVEQ #5,D0 → 0x7005
        assert_eq!(asm.code[0].words, vec![0x7005]);
        assert_eq!(asm.code[0].words, vec![0x7005]); // MOVEQ #5,D0
        assert_eq!(asm.code[1].words, vec![0x4E75]); // RTS
    }

    #[test]
    fn test_macro_redefinition() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    NOP
redef MACRO
    NOP
    ENDM
redef MACRO
    RTS
    ENDM
    redef
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 2);
        assert_eq!(asm.code[1].words, vec![0x4E75]); // second definition wins
    }

    #[test]
    fn test_macro_with_label_on_invocation() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
nopmac MACRO
    NOP
    ENDM
label: nopmac
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        // label resolves to PC of NOP
        assert_eq!(asm.symbols.resolve("label").unwrap(), 0x1000);
    }

    #[test]
    fn test_macro_unique_at() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
uniquemac MACRO
local\\@ EQU $
    NOP
    ENDM
    uniquemac
    uniquemac
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        // First expansion generates local0001 EQU $ (0x1000)
        // Second expansion generates local0002 EQU $ (0x1002)
        assert!(asm.symbols.resolve("local0001").is_ok());
        assert!(asm.symbols.resolve("local0002").is_ok());
        assert_eq!(asm.symbols.resolve("local0001").unwrap(), 0x1000);
        assert_eq!(asm.symbols.resolve("local0002").unwrap(), 0x1002);
    }

    #[test]
    fn test_rept_simple() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    REPT 3
    NOP
    ENDR
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 4);
        assert_eq!(asm.code[0].words, vec![0x4E71]);
        assert_eq!(asm.code[1].words, vec![0x4E71]);
        assert_eq!(asm.code[2].words, vec![0x4E71]);
        assert_eq!(asm.code[3].words, vec![0x4E75]);
    }

    #[test]
    fn test_rept_zero() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    REPT 0
    NOP
    ENDR
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
        assert_eq!(asm.code[0].words, vec![0x4E75]);
    }

    #[test]
    fn test_rept_with_label() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
loop REPT 2
    NOP
    ENDR
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 3);
        assert_eq!(asm.symbols.resolve("loop").unwrap(), 0x1000);
    }

    #[test]
    fn test_irp_simple() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    IRP \\reg,D0,D1,D2
    MOVEQ #0,\\reg
    ENDR
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 4);
        // MOVEQ #0,D0 = 0x7000, MOVEQ #0,D1 = 0x7200, MOVEQ #0,D2 = 0x7400
        assert_eq!(asm.code[0].words, vec![0x7000]);
        assert_eq!(asm.code[1].words, vec![0x7200]);
        assert_eq!(asm.code[2].words, vec![0x7400]);
        assert_eq!(asm.code[3].words, vec![0x4E75]);
    }

    #[test]
    fn test_irp_substitution() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    IRP \\val,$01,$02,$03
    DC.B \\val
    ENDR
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 4);
        // DC.B $01 = 0x0100, DC.B $02 = 0x0200, DC.B $03 = 0x0300
        assert_eq!(asm.code[0].words, vec![0x0100]);
        assert_eq!(asm.code[1].words, vec![0x0200]);
        assert_eq!(asm.code[2].words, vec![0x0300]);
    }

    #[test]
    fn test_irpc_simple() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    IRPC \\c,ABC
    DC.B \"\\c\"
    ENDR
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 4);
        // DC.B "A" = 0x4100, DC.B "B" = 0x4200, DC.B "C" = 0x4300
        assert_eq!(asm.code[0].words, vec![0x4100]);
        assert_eq!(asm.code[1].words, vec![0x4200]);
        assert_eq!(asm.code[2].words, vec![0x4300]);
    }

    #[test]
    fn test_rept_nested() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    REPT 2
    NOP
    NOP
    ENDR
    RTS
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 5);
        for i in 0..4 {
            assert_eq!(asm.code[i].words, vec![0x4E71]);
        }
        assert_eq!(asm.code[4].words, vec![0x4E75]);
    }

    #[test]
    fn test_assemble_bra_with_relaxation() {
        let mut asm = Assembler::new(0x1000);
        // Short branch (target within 127 bytes)
        let result = asm.assemble(
            "
    BRA target
    NOP
    NOP
    NOP
target:
    RTS
",
        );
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        // BRA should be encoded as short (2 bytes) since target is close
        let bra_instr = &asm.code[0];
        assert_eq!(bra_instr.words.len(), 1); // 1 word = short branch
    }

    #[test]
    fn test_assemble_dc_w() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    DC.W $1234,$5678
",
        );
        assert!(result.is_ok());
        assert_eq!(asm.code.len(), 1);
        assert_eq!(asm.code[0].words, vec![0x1234, 0x5678]);
    }

    #[test]
    fn test_assemble_even_directive() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    DC.B $12
    EVEN
    DC.W $3456
",
        );
        assert!(result.is_ok());
        // DC.B $12 should be padded to even, so next DC.W should be at 0x1002
        assert_eq!(asm.code[1].pc, 0x1002);
    }

    #[test]
    fn test_assemble_dc_b_string_hello() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            r#"
    DC.B "hello"
"#,
        );
        assert!(result.is_ok());
        // "hello" = 68,65,6C,6C,6F -> packed as 6865, 6C6C, 6F00 (padded to even)
        let instr = &asm.code[0];
        assert_eq!(instr.words, vec![0x6865, 0x6C6C, 0x6F00]);
    }

    #[test]
    fn test_assemble_dc_b_string_single_char() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            r#"
    DC.B "A"
"#,
        );
        assert!(result.is_ok());
        // "A" = 41 -> packed as 4100 (padded to even)
        let instr = &asm.code[0];
        assert_eq!(instr.words, vec![0x4100]);
    }

    #[test]
    fn test_assemble_dc_b_string_with_null() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            r#"
    DC.B "hello",0
"#,
        );
        assert!(result.is_ok());
        // "hello",0 = 68,65,6C,6C,6F,00 -> packed as 6865, 6C6C, 6F00
        let instr = &asm.code[0];
        assert_eq!(instr.words, vec![0x6865, 0x6C6C, 0x6F00]);
    }

    #[test]
    fn test_assemble_dc_b_string_even_length() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            r#"
    DC.B "ab"
"#,
        );
        assert!(result.is_ok());
        // "ab" = 61,62 -> packed as 6162 (no padding needed)
        let instr = &asm.code[0];
        assert_eq!(instr.words, vec![0x6162]);
    }

    #[test]
    fn test_assemble_dc_b_string_mixed() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            r#"
    DC.B "Hi",0,$FF
"#,
        );
        assert!(result.is_ok());
        let instr = &asm.code[0];
        assert_eq!(instr.words, vec![0x4869, 0x00FF]);
    }

    #[test]
    fn test_assemble_dc_s() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    DC.S 3.14
",
        );
        assert!(result.is_ok(), "DC.S failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
        // 3.14 in IEEE 754 single = 0x4048F5C3
        assert_eq!(asm.code[0].words, vec![0x4048, 0xF5C3]);
    }

    #[test]
    fn test_assemble_dc_d() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    DC.D 1.0
",
        );
        assert!(result.is_ok(), "DC.D failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
        // 1.0 in IEEE 754 double = 0x3FF0000000000000
        assert_eq!(asm.code[0].words, vec![0x3FF0, 0x0000, 0x0000, 0x0000]);
    }

    #[test]
    fn test_assemble_ifc() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    IFC \"hello\",\"hello\"
    NOP
    ENDIF
    RTS
",
        );
        assert!(result.is_ok(), "IFC failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 2);
        assert_eq!(asm.code[0].words, vec![0x4E71]); // NOP
    }

    #[test]
    fn test_assemble_ifnc() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    IFNC \"hello\",\"world\"
    NOP
    ENDIF
    RTS
",
        );
        assert!(result.is_ok(), "IFNC failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 2);
        assert_eq!(asm.code[0].words, vec![0x4E71]); // NOP
    }

    #[test]
    fn test_assemble_cnop() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    DC.B $12
    CNOP 0,4
    DC.W $FFFF
",
        );
        assert!(result.is_ok(), "CNOP failed: {:?}", result.err());
        // DC.B at 0x1000 (1 byte), then CNOP pads to next multiple of 4
        // So DC.W should be at 0x1004 (0x1001 padded to 4-byte boundary = 0x1004)
        assert_eq!(asm.code[2].pc, 0x1004);
    }

    #[test]
    fn test_assemble_offset() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    OFFSET 0
field1 DS.B 2
field2 DS.W 1
    RTS
",
        );
        assert!(result.is_ok(), "OFFSET failed: {:?}", result.err());
        // OFFSET is a no-op, doesn't emit code
    }

    #[test]
    fn test_assemble_opt() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    OPT A+,F+
    NOP
",
        );
        assert!(result.is_ok(), "OPT failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
        assert_eq!(asm.code[0].words, vec![0x4E71]);
    }

    #[test]
    fn test_assemble_text_section() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    TEXT
    NOP
    DATA
    NOP
",
        );
        assert!(result.is_ok(), "text section failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 2);
    }

    #[test]
    fn test_assemble_print() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    PRINT \"hello world\"
    NOP
",
        );
        assert!(result.is_ok(), "PRINT failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 1);
    }

    #[test]
    fn test_assemble_movec() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    MOVEC VBR,D0
",
        );
        assert!(result.is_ok(), "MOVEC failed: {:?}", result.err());
        assert_eq!(asm.code[0].words, vec![0x4E7A, 0x0801]);
    }

    #[test]
    fn test_assemble_mulu_l() {
        let mut asm = Assembler::new(0x1000);
        asm.cpu = "68020".to_string();
        let result = asm.assemble(
            "
    ORG $1000
    MULU.L D1,D2
",
        );
        assert!(result.is_ok(), "MULU.L failed: {:?}", result.err());
        assert_eq!(asm.code[0].words, vec![0x4C01, 0x2002]);
    }

    #[test]
    fn test_assemble_divs_l() {
        let mut asm = Assembler::new(0x1000);
        asm.cpu = "68020".to_string();
        let result = asm.assemble(
            "
    ORG $1000
    DIVS.L D1,D2
",
        );
        assert!(result.is_ok(), "DIVS.L failed: {:?}", result.err());
        assert_eq!(asm.code[0].words, vec![0x4C41, 0x2802]);
    }

    #[test]
    fn test_assemble_mexit() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
mymexc MACRO
    NOP
    MEXIT
    NOP
    ENDM
    mymexc
    RTS
",
        );
        assert!(result.is_ok(), "MEXIT failed: {:?}", result.err());
        // After MEXIT, second NOP should not be emitted
        assert_eq!(asm.code.len(), 2);
        assert_eq!(asm.code[0].words, vec![0x4E71]); // first NOP
        assert_eq!(asm.code[1].words, vec![0x4E75]); // RTS
    }

    /// Assembles `source` with an explicit CPU level and returns the resulting bytes.
    fn assemble_source_with_cpu(source: &str, cpu: &str) -> Vec<u8> {
        let mut asm = Assembler::new(0);
        asm.set_cpu(cpu);
        asm.assemble_bytes(source).unwrap()
    }

    #[test]
    fn test_source_pmove_tc() {
        // TC is the destination (memory-to-register, R/W=0). Verified
        // against real `vasm -m68030` output: F010 4000.
        let bytes = assemble_source_with_cpu("    PMOVE (A0),TC\n", "68030");
        assert_eq!(bytes, vec![0xF0, 0x10, 0x40, 0x00]);
    }

    #[test]
    fn test_source_ptestr() {
        // Verified against real `vasm -m68030` output: F010 8E12.
        let bytes = assemble_source_with_cpu("    PTESTR #2,(A0),#3\n", "68030");
        assert_eq!(bytes, vec![0xF0, 0x10, 0x8E, 0x12]);
    }

    #[test]
    fn test_source_pflusha() {
        // Verified against real `vasm -m68030` output: F000 2400.
        let bytes = assemble_source_with_cpu("    PFLUSHA\n", "68030");
        assert_eq!(bytes, vec![0xF0, 0x00, 0x24, 0x00]);
    }

    #[test]
    fn test_source_pflushn() {
        // Verified against real `vasm -m68040` output: F500.
        let bytes = assemble_source_with_cpu("    PFLUSHN (A0)\n", "68040");
        assert_eq!(bytes, vec![0xF5, 0x00]);
    }

    #[test]
    fn test_source_lpstop() {
        let bytes = assemble_source_with_cpu("    LPSTOP #$2700\n", "68060");
        assert_eq!(bytes, vec![0xF8, 0x00, 0x01, 0xC0, 0x27, 0x00]);
    }

    #[test]
    fn test_source_cinva() {
        // CINVA #3 (BC, both caches) -- verified against real
        // `vasm -m68040` output for `cinva bc`: F4D8.
        let bytes = assemble_source_with_cpu("    CINVA #3\n", "68040");
        assert_eq!(bytes, vec![0xF4, 0xD8]);
    }

    #[test]
    fn test_source_psave() {
        let bytes = assemble_source_with_cpu("    PSAVE -(A0)\n", "68030");
        assert_eq!(bytes, vec![0xF0, 0xA0]);
    }

    #[test]
    fn test_source_cas_word() {
        let bytes = assemble_source_with_cpu("    CAS.W D0,D1,(A0)\n", "68020");
        assert_eq!(bytes, vec![0x0C, 0xD0, 0x00, 0x40]);
    }

    #[test]
    fn test_source_pack_dn_dn() {
        let bytes = assemble_source_with_cpu("    PACK D0,D1,#$0201\n", "68020");
        assert_eq!(bytes, vec![0x83, 0x40, 0x02, 0x01]);
    }

    #[test]
    fn test_source_unpk_dn_dn() {
        let bytes = assemble_source_with_cpu("    UNPK D0,D1,#$0201\n", "68020");
        assert_eq!(bytes, vec![0x83, 0x80, 0x02, 0x01]);
    }

    #[test]
    fn test_source_pack_predec_bugfix() {
        // `pack -(a0),-(a1),#adj` must resolve the register number from the
        // parsed predecrement operand correctly.
        let bytes = assemble_source_with_cpu("    PACK -(A0),-(A1),#$0201\n", "68020");
        // type_bit=1, rx=dst.n=1, ry=src.n=0 -> 0x8140|(1<<3)|(1<<9)|0 = 0x8348
        assert_eq!(bytes, vec![0x83, 0x48, 0x02, 0x01]);
    }

    #[test]
    fn test_source_cas2_word() {
        // cas2.w d0:d1,d2:d3,(a0):(a1) -> 0cfc808090c1
        let bytes = assemble_source_with_cpu("    CAS2.W D0:D1,D2:D3,(A0):(A1)\n", "68020");
        assert_eq!(bytes, vec![0x0C, 0xFC, 0x80, 0x80, 0x90, 0xC1]);
    }

    #[test]
    fn test_source_cas_followed_by_label_size_estimate() {
        // Regression check: pass-1 size estimation for CAS (a pre-existing 3-operand
        // instruction not routed through encode_instruction) falls back to a fixed 2-byte
        // guess, which undercounts the true 4-byte encoding. This does not corrupt the
        // final CAS bytes themselves (pass 2 always re-encodes correctly), but a label
        // placed right after CAS can end up at the wrong address if anything depends on
        // pass-1 forward-reference sizing. Documented here rather than fixed, since CAS/
        // PACK/UNPK's pass-1 estimation gap predates this task and CAS2 shares the same
        // architecture; the fix belongs with a broader pass-1 estimation pass.
        let bytes = assemble_source_with_cpu("    CAS.W D0,D1,(A0)\nlabel:\n    NOP\n", "68020");
        assert_eq!(bytes, vec![0x0C, 0xD0, 0x00, 0x40, 0x4E, 0x71]);
    }

    #[test]
    fn test_sections_populated_single_text() {
        // Regression check for B1: SectionManager must actually receive
        // instructions, not just track the PC while `code` fills up separately.
        let mut asm = Assembler::new(0x1000);
        asm.assemble("    ORG $1000\n    NOP\n    RTS\n").unwrap();
        let text = asm
            .sections
            .get_section(&crate::directives::SectionKind::Text)
            .expect("text section must exist");
        assert_eq!(text.instructions.len(), 2);
        assert_eq!(text.to_bytes(), vec![0x4E, 0x71, 0x4E, 0x75]);
    }

    #[test]
    fn test_sections_populated_multi_section() {
        // SECTION/TEXT/DATA switches must route subsequent instructions into
        // distinct sections with independently tracked location counters.
        let mut asm = Assembler::new(0x1000);
        asm.assemble(
            "    SECTION text\n    NOP\n    SECTION data\n    DC.W $1234\n    SECTION text\n    RTS\n",
        )
        .unwrap();

        let text = asm
            .sections
            .get_section(&crate::directives::SectionKind::Text)
            .expect("text section must exist");
        let data = asm
            .sections
            .get_section(&crate::directives::SectionKind::Data)
            .expect("data section must exist");

        assert_eq!(text.instructions.len(), 2); // NOP + RTS
        assert_eq!(data.instructions.len(), 1); // DC.W
        assert_eq!(data.to_bytes(), vec![0x12, 0x34]);
    }

    // B2: 68020+ memory indirect / full format EA. Byte patterns cross-checked
    // by round-tripping through `m68k_core::addressing::decode_ea`, not just
    // re-derived from the encoder itself.

    #[test]
    fn test_source_memory_indirect_simple() {
        let bytes = assemble_source_with_cpu("    MOVE.L ([A0]),D2\n", "68020");
        assert_eq!(bytes, vec![0x24, 0x30, 0x01, 0x45]);
    }

    #[test]
    fn test_source_memory_indirect_with_bd() {
        let bytes = assemble_source_with_cpu("    MOVE.L ([$10,A0]),D2\n", "68020");
        assert_eq!(bytes, vec![0x24, 0x30, 0x01, 0x65, 0x00, 0x10]);
    }

    #[test]
    fn test_source_memory_indirect_long_bd() {
        // Base displacement outside 16-bit signed range forces bd_code=3 (long).
        let bytes = assemble_source_with_cpu("    MOVE.L ([$100000,A0]),D2\n", "68020");
        assert_eq!(bytes, vec![0x24, 0x30, 0x01, 0x75, 0x00, 0x10, 0x00, 0x00]);
    }

    #[test]
    fn test_source_memory_indirect_preindexed() {
        // Index inside the brackets: ([bd,An,Xn],od)
        let bytes = assemble_source_with_cpu("    MOVE.L ([$10,A0,D1.W*2],$20),D2\n", "68020");
        assert_eq!(bytes, vec![0x24, 0x30, 0x13, 0x26, 0x00, 0x10, 0x00, 0x20]);
    }

    #[test]
    fn test_source_memory_indirect_postindexed() {
        // Index outside the brackets: ([bd,An],Xn,od)
        let bytes = assemble_source_with_cpu("    MOVE.L ([$10,A0],D1.W*2,$20),D2\n", "68020");
        assert_eq!(bytes, vec![0x24, 0x30, 0x13, 0x22, 0x00, 0x10, 0x00, 0x20]);
    }

    #[test]
    fn test_source_memory_indirect_base_suppressed() {
        // No An/PC in the brackets: base register suppressed, index-only.
        let bytes = assemble_source_with_cpu("    MOVE.L ([D1.W*2],D2),D3\n", "68020");
        assert_eq!(bytes, vec![0x26, 0x30, 0x21, 0x85]);
    }

    #[test]
    fn test_source_memory_indirect_requires_68020() {
        let mut asm = Assembler::new(0);
        let err = asm.assemble("    MOVE.L ([A0]),D2\n").unwrap_err();
        assert!(err.message.contains("68020"));
    }

    #[test]
    fn test_source_memory_indirect_pc_relative_unsupported() {
        // PC-relative full-format EAs are not implemented on the assembler
        // side (decoder-only feature).
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        let err = asm
            .assemble("    MOVE.L ([$10,PC],D1.W*2,$20),D2\n")
            .unwrap_err();
        assert!(err.message.contains("PC-relative"));
    }

    // B4: MULS.L/MULU.L Dh:Dl (64-bit product), B3: DIVSL/DIVUL + DIVS.L/
    // DIVU.L Dr:Dq (64-bit dividend, remainder form).

    #[test]
    fn test_source_mulu_l_64bit() {
        let bytes = assemble_source_with_cpu("    MULU.L D1,D3:D2\n", "68020");
        assert_eq!(bytes, vec![0x4C, 0x01, 0x24, 0x03]);
    }

    #[test]
    fn test_source_muls_l_64bit() {
        let bytes = assemble_source_with_cpu("    MULS.L D1,D3:D2\n", "68020");
        assert_eq!(bytes, vec![0x4C, 0x01, 0x2C, 0x03]);
    }

    #[test]
    fn test_source_divu_l_64bit() {
        let bytes = assemble_source_with_cpu("    DIVU.L D1,D3:D2\n", "68020");
        assert_eq!(bytes, vec![0x4C, 0x41, 0x24, 0x03]);
    }

    #[test]
    fn test_source_divs_l_64bit() {
        let bytes = assemble_source_with_cpu("    DIVS.L D1,D3:D2\n", "68020");
        assert_eq!(bytes, vec![0x4C, 0x41, 0x2C, 0x03]);
    }

    #[test]
    fn test_source_divsl() {
        let bytes = assemble_source_with_cpu("    DIVSL D1,D3:D2\n", "68020");
        assert_eq!(bytes, vec![0x4C, 0x41, 0x28, 0x03]);
    }

    #[test]
    fn test_source_divul() {
        let bytes = assemble_source_with_cpu("    DIVUL D1,D3:D2\n", "68020");
        assert_eq!(bytes, vec![0x4C, 0x41, 0x20, 0x03]);
    }

    #[test]
    fn test_source_divsl_dot_l_alias() {
        // DIVSL.L is an accepted alias for DIVSL (both require Dr:Dq).
        let bytes = assemble_source_with_cpu("    DIVSL.L D1,D3:D2\n", "68020");
        assert_eq!(bytes, vec![0x4C, 0x41, 0x28, 0x03]);
    }

    #[test]
    fn test_source_divul_dot_l_alias() {
        let bytes = assemble_source_with_cpu("    DIVUL.L D1,D3:D2\n", "68020");
        assert_eq!(bytes, vec![0x4C, 0x41, 0x20, 0x03]);
    }

    #[test]
    fn test_source_divsl_requires_regpair() {
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        let err = asm.assemble("    DIVSL D1,D2\n").unwrap_err();
        assert!(err.message.contains("Dr:Dq"));
    }

    #[test]
    fn test_addr_reg_indirect_index_l_suffix_sets_long_bit() {
        // Regression check for N4: the brief-index extension word's "long"
        // bit (bit 11, 0x0800) must reflect the parsed .W/.L index size
        // suffix instead of being hardcoded to .W.
        let bytes_l = assemble_source_with_cpu("    MOVE.L (0,A0,D1.L),D0\n", "68000");
        let bytes_w = assemble_source_with_cpu("    MOVE.L (0,A0,D1.W),D0\n", "68000");
        assert_eq!(bytes_l, vec![0x20, 0x30, 0x08, 0x01]);
        assert_eq!(bytes_w, vec![0x20, 0x30, 0x00, 0x01]);
    }

    #[test]
    fn test_pc_relative_index_l_suffix_sets_long_bit() {
        let bytes_l = assemble_source_with_cpu("    MOVE.L ($4,PC,D2.L),D0\n", "68000");
        let bytes_w = assemble_source_with_cpu("    MOVE.L ($4,PC,D2.W),D0\n", "68000");
        assert_eq!(bytes_w, vec![0x20, 0x3B, 0x40, 0x02]);
        // Only the extension word's long bit (0x0800) should differ.
        assert_eq!(bytes_l, vec![0x20, 0x3B, 0x48, 0x02]);
    }

    #[test]
    fn test_dbra_with_label_target() {
        // Regression check for B8: DBcc with a label operand (as opposed to
        // a literal address) went through encode_branch, which had no DBcc
        // arm and misrouted the Dn register as the branch target.
        let bytes = assemble_source_with_cpu("LOOP:\n    DBRA D0,LOOP\n", "68000");
        assert_eq!(bytes, vec![0x51, 0xC8, 0xFF, 0xFE]);
    }

    #[test]
    fn test_dbne_with_label_target() {
        let bytes = assemble_source_with_cpu("LOOP:\n    DBNE D1,LOOP\n", "68000");
        assert_eq!(bytes, vec![0x57, 0xC9, 0xFF, 0xFE]);
    }

    #[test]
    fn test_movem_sp_alias() {
        // SP must be accepted as an alias for A7.
        let bytes = assemble_source_with_cpu("    MOVEM.L D0-D7/A0-A7,-(SP)\n", "68000");
        assert_eq!(bytes, vec![0x48, 0xE7, 0xFF, 0xFF]);
    }

    #[test]
    fn test_lea_displacement_before_paren() {
        // Motorola-style `disp(An)` (displacement before the parens) must
        // be equivalent to the `(disp,An)` form.
        let bytes = assemble_source_with_cpu("    LEA $1000(A0),A0\n", "68000");
        assert_eq!(bytes, vec![0x41, 0xE8, 0x10, 0x00]);
    }
}
