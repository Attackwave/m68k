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
use m68k_core::tokens::{is_local_label, split_line};

use crate::directives::{
    SectionManager, handle_align_pass1, handle_align_pass2, handle_equ, handle_even_pass1,
    handle_even_pass2, handle_incbin_pass1, handle_incbin_pass2, handle_section, handle_set,
    parse_dc_string, resolve_include_path_in, strip_quotes,
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
    /// Global label currently in scope, for resolving local labels.
    ///
    /// Local labels (`.loop`) are stored under a qualified name
    /// (`draw.loop`). Keeping the scope here rather than at the call sites
    /// means every lookup path — expression evaluator, operand parser,
    /// branch relaxation — resolves them the same way without each having
    /// to know about scoping.
    local_scope: Option<String>,
    /// Whether to shorten an absolute address that fits in 16 bits to the
    /// absolute-short addressing mode.
    ///
    /// Off by default, matching what Motorola-syntax assemblers emit
    /// without optimization: an absolute address is encoded long unless
    /// the source says `.W`. Turning it on reproduces their optimizing
    /// mode. Lives here because the operand parser — a free function
    /// reached from ~27 call sites — already receives the symbol table.
    optimize_absolute: bool,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the global label that local labels resolve against.
    pub fn set_local_scope(&mut self, global: Option<String>) {
        self.local_scope = global;
    }

    /// Enable/disable shortening of absolute addresses that fit in 16
    /// bits (see [`Self::optimize_absolute`]).
    pub fn set_optimize_absolute(&mut self, on: bool) {
        self.optimize_absolute = on;
    }

    /// Whether absolute-address shortening is enabled.
    pub fn optimize_absolute(&self) -> bool {
        self.optimize_absolute
    }

    /// Map a name as written in source to its stored name, qualifying
    /// local labels with the current scope. Non-local names, and local
    /// names with no enclosing global label, pass through unchanged.
    pub fn resolve_name(&self, name: &str) -> String {
        if is_local_label(name)
            && let Some(global) = &self.local_scope
        {
            return qualified_local_name(global, name);
        }
        name.to_string()
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

    /// Look up a symbol. Returns `Err` if undefined. Resolves local
    /// labels against the current scope, like [`Self::get`].
    pub fn resolve(&self, name: &str) -> Result<u32, AsmError> {
        match self.get(name) {
            Some(entry) if entry.defined => Ok(entry.value),
            Some(entry) => Err(AsmError::with_line(
                format!("undefined symbol: {}", name),
                entry.line_no.unwrap_or(0),
            )),
            None => Err(AsmError::new(format!("undefined symbol: {}", name))),
        }
    }

    /// Look up a symbol, resolving local labels against the current scope
    /// (see [`Self::set_local_scope`]).
    pub fn get(&self, name: &str) -> Option<&SymbolEntry> {
        self.symbols
            .get(&self.resolve_name(name))
            .or_else(|| self.symbols.get(name))
    }

    /// Check if a symbol exists (defined or not).
    pub fn contains(&self, name: &str) -> bool {
        self.symbols.contains_key(&self.resolve_name(name)) || self.symbols.contains_key(name)
    }

    /// Force-set a symbol value (allows redefinition, used by SET directive).
    pub fn force_set(&mut self, name: &str, value: u32, line_no: Option<usize>) {
        self.symbols.insert(
            name.to_string(),
            SymbolEntry::new(name.to_string(), value, true, line_no),
        );
    }

    /// Like [`Self::force_set`], but also records the defining section
    /// (see [`Self::define_in_section`]) — used by branch relaxation, which
    /// re-derives every label's PC from scratch on each iteration and must
    /// preserve section tracking across those overwrites.
    pub fn force_set_in_section(
        &mut self,
        name: &str,
        value: u32,
        line_no: Option<usize>,
        section: Option<&str>,
    ) {
        self.symbols.insert(
            name.to_string(),
            SymbolEntry::new(name.to_string(), value, true, line_no)
                .with_section(section.map(|s| s.to_string())),
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
    /// Exact byte length, when it isn't `words.len() * 2`.
    ///
    /// `DC.B` can emit an odd number of bytes, and the following
    /// directive must start on the very next byte rather than after a
    /// pad. Instructions leave this `None` — they are always whole words.
    pub byte_len: Option<usize>,
}

impl AssembledInstruction {
    pub fn size_bytes(&self) -> usize {
        self.byte_len.unwrap_or(self.words.len() * 2)
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
    /// Raw target operand text (label or expression), re-evaluated against
    /// the symbol table on each relaxation iteration.
    pub target_text: Option<String>,
    /// Current size hint.
    pub size_hint: BranchSize,
    /// Source line number.
    pub line_no: Option<usize>,
    /// Global label in scope at this branch, so a local target (`.exit`)
    /// resolves against the right one during relaxation — by then the
    /// symbol table's scope has moved to the end of the file.
    pub local_scope: Option<String>,
}

// ---------------------------------------------------------------------------
// FMOVE.P k-factor parsing (`FMOVE.P FPn,<ea>{#k}` / `{Dn}`)
// ---------------------------------------------------------------------------

/// Whether `text` ends in a `{...}` suffix that looks like a k-factor
/// (`{#k}` or `{Dn}`) rather than a bitfield spec (`{offset:width}`,
/// which always contains a `:`). Both use the same `{...}` syntax on an
/// EA operand, so the two must be told apart before parsing the operand
/// as a whole — bitfields are handled generically inside
/// `parse_operand_text`, but a k-factor needs to be split off *before*
/// that (it isn't part of the EA operand at all, it's FMOVE's implicit
/// third operand).
fn has_kfactor_suffix(text: &str) -> bool {
    let text = text.trim();
    text.ends_with('}')
        && text
            .rfind('{')
            .map(|open| !text[open..].contains(':'))
            .unwrap_or(false)
}

/// Split `text` (already confirmed via [`has_kfactor_suffix`]) into the
/// EA portion and the parsed [`crate::enc_fpu::KFactor`].
fn split_kfactor_suffix(text: &str) -> Result<(&str, crate::enc_fpu::KFactor), String> {
    use crate::enc_fpu::KFactor;
    let text = text.trim();
    let open = text.rfind('{').ok_or("missing '{' in k-factor operand")?;
    let ea_text = text[..open].trim();
    let inner = text[open + 1..text.len() - 1].trim();

    if let Some(imm) = inner.strip_prefix('#') {
        let k: i32 = imm
            .trim()
            .parse()
            .map_err(|_| format!("invalid k-factor: {}", inner))?;
        if !(-64..=63).contains(&k) {
            return Err(format!(
                "k-factor {} out of range (-64..=63, 7-bit two's complement)",
                k
            ));
        }
        return Ok((ea_text, KFactor::Static(k as i8)));
    }
    match parse_register(inner) {
        Some(Operand::DataReg(n)) => Ok((ea_text, KFactor::Dynamic(n))),
        _ => Err(format!(
            "k-factor must be #k (static) or Dn (dynamic register): {}",
            inner
        )),
    }
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

    // FPc:FPs register pair — FSINCOS's destination, written
    // `FSINCOS.X <ea>,FPc:FPs`. Reuses `RegPair`, since the FP register
    // number occupies the same field width; the mnemonic tells the
    // encoder which register file the pair refers to.
    if let Some((a, b)) = text.split_once(':')
        && let (Some(fa), Some(fb)) = (parse_fp_reg(a.trim()), parse_fp_reg(b.trim()))
    {
        return Ok(Operand::RegPair(fa, fb));
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
    if let Some((xn, target, scale, xn_is_long)) =
        parse_parens_disp_pc_index(paren_text, symbols, current_pc)
    {
        return Ok(Operand::PcRelativeIndex(xn, target, scale, xn_is_long));
    }

    // PC-relative with displacement: (d16,PC) or (d32,PC)
    if let Some((target, is_long)) = parse_parens_disp_pc(paren_text, symbols, current_pc) {
        return Ok(Operand::PcRelativeDisp(target, is_long));
    }

    // Addressing with displacement: (d16,An) or (d32,An)
    if let Some((disp, reg)) = parse_parens_disp_register(paren_text, symbols, current_pc) {
        let is_long = !(-0x8000..=0x7FFF).contains(&disp);
        return Ok(Operand::AddrRegIndirectDisp(reg, disp, is_long));
    }

    // Indexed with base register: (d8,An,Xn*scale)
    if let Some((an, xn, disp, scale, xn_is_long)) =
        parse_parens_disp_reg_index(paren_text, symbols, current_pc)
    {
        return Ok(Operand::AddrRegIndirectIndex(
            an, xn, disp, scale, xn_is_long,
        ));
    }

    // Absolute address with .W/.L suffix.
    //
    // Case-insensitively: this used to test only for the upper-case forms,
    // so `lea $400.w,a6` silently produced the long encoding while
    // `lea $400.W,a6` produced the short one. Lower-case suffixes are the
    // common spelling in real sources, and the mismatch showed up as a
    // large share of the ROM roundtrip failures.
    let suffix_upper = {
        let bytes = text.as_bytes();
        if bytes.len() >= 2 {
            let n = bytes.len();
            if bytes[n - 2] == b'.' {
                Some(bytes[n - 1].to_ascii_uppercase())
            } else {
                None
            }
        } else {
            None
        }
    };
    if matches!(suffix_upper, Some(b'W') | Some(b'L')) {
        let force_long = suffix_upper == Some(b'L');
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

    // Absolute address: $xxxx or a number/label without a register.
    //
    // Encoded long unless shortening is enabled, which is what
    // Motorola-syntax assemblers do without optimization — `MOVE.W
    // $1234,D0` is `3039 00001234`, not `3038 1234`. An explicit `.W`
    // suffix is handled above and always yields the short form, so the
    // user keeps full control either way. Silently shortening here made
    // every absolute reference two bytes smaller than the reference
    // assemblers', shifting everything after it.
    if let Ok(value) = evaluate_expr_str(text, symbols, current_pc) {
        // Check if it looks like an absolute address (no register, no #)
        if !text.contains('(') && !text.contains(')') {
            if symbols.optimize_absolute() && (0..=0xFFFF).contains(&value) {
                return Ok(Operand::AbsoluteShort(value as i32));
            } else {
                return Ok(Operand::AbsoluteLong(value as i32));
            }
        }
    }

    // Special registers.
    //
    // These are `Operand::Special`, not marker immediates. They used to be
    // `Immediate(-1)`/`Immediate(-2)`/`Immediate(0x800)`, which collide
    // with those values written literally: `MOVE.W #-1,D0` matched the CCR
    // arm and assembled to 42C0 (`MOVE CCR,D0`) — a different instruction,
    // silently. `#-2` became `MOVE SR,D0` the same way, and `MOVE.L
    // #-2,A0` was rejected outright.
    if text.eq_ignore_ascii_case("CCR") {
        return Ok(Operand::Special("CCR".to_string()));
    }
    if text.eq_ignore_ascii_case("SR") {
        return Ok(Operand::Special("SR".to_string()));
    }
    // USP is a control register, not A7: mapping it to AddrReg(7) made
    // `move usp,a0` assemble as a plain `move a7,a0` (0x304F) instead of
    // the privileged 0x4E68 form.
    if text.eq_ignore_ascii_case("USP") {
        return Ok(Operand::Special("USP".to_string()));
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

    // 68040 cache-scope names for CINVA/CPUSHA/CINVL/CINVP/CPUSHL/CPUSHP
    // (`cinva ic`, `cpushl dc,(a0)`), passed through as Special for the
    // encoder to resolve — same approach as the PMOVE names above, since
    // `DC`/`BC`/`IC`/`NC` are only cache scopes in those instructions'
    // operand position and are otherwise ordinary identifiers. A symbol
    // of the same name still wins: user labels must not be shadowed by
    // an instruction-specific keyword.
    if matches!(text.to_uppercase().as_str(), "NC" | "DC" | "IC" | "BC")
        && symbols.get(text).is_none()
    {
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
    // `strip_prefix`/`strip_suffix` (not byte-index slicing) are required
    // here: they only match at an actual character boundary and return
    // `None` otherwise, whereas slicing with a hardcoded byte offset like
    // `&trimmed[1..]` panics if that offset lands inside a multi-byte
    // UTF-8 character (a fuzzing-found crash: a leading multi-byte
    // character made `trimmed.starts_with('(')` false, but the sibling
    // `)+`/-( ` variants below only checked the *end*/*start* substring,
    // not both, so the byte-offset slice for the other side still ran).
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix('(')?.strip_suffix(')')?;
    if let Some(Operand::AddrReg(n)) = parse_register(inner) {
        return Some(n);
    }
    None
}

/// Parse (An)+ - post-increment.
fn parse_parens_register_plus(text: &str) -> Option<u8> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix('(')?.strip_suffix(")+")?;
    if let Some(Operand::AddrReg(n)) = parse_register(inner) {
        return Some(n);
    }
    None
}

/// Parse -(An) - pre-decrement.
fn parse_minus_parens_register(text: &str) -> Option<u8> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix("-(")?.strip_suffix(')')?;
    if let Some(Operand::AddrReg(n)) = parse_register(inner) {
        return Some(n);
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

fn parse_parens_disp_register(text: &str, symbols: &SymbolTable, pc: u32) -> Option<(i32, u8)> {
    let trimmed = text.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        // Try (d,An)
        if let Some(pos) = inner.find(',') {
            let disp_str = inner[..pos].trim();
            let reg_str = inner[pos + 1..].trim();

            // `(An,Xn)` — an address register in the *first* position makes
            // this the indexed form, not a displacement. Falling through to
            // the dot-notation branch below read `(A0,A1.L)` as base A1 with
            // displacement "A0" (evaluating to 0), silently dropping the
            // index register.
            if matches!(parse_register(disp_str), Some(Operand::AddrReg(_))) {
                return None;
            }

            // Check for indexed: An.Xn (dot notation)
            if let Some(dot_pos) = reg_str.find('.') {
                let reg_part = &reg_str[..dot_pos];
                if let Some(Operand::AddrReg(n)) = parse_register(reg_part) {
                    let disp = evaluate_displacement(disp_str, symbols, pc);
                    return Some((disp, n));
                }
            } else if let Some(Operand::AddrReg(n)) = parse_register(reg_str) {
                let disp = evaluate_displacement(disp_str, symbols, pc);
                return Some((disp, n));
            }
        }
    }
    None
}

/// Parse (d16,PC) or (d32,PC) - PC-relative with displacement.
///
/// The returned value is the full target address, not the displacement;
/// `encode_ea` subtracts the extension word's address.
fn parse_parens_disp_pc(text: &str, symbols: &SymbolTable, pc: u32) -> Option<(i32, bool)> {
    let trimmed = text.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        if let Some(pos) = inner.rfind(',') {
            let disp_str = inner[..pos].trim();
            let reg_str = inner[pos + 1..].trim();
            if reg_str.to_uppercase() == "PC" {
                let target = evaluate_pc_target(disp_str, symbols, pc);
                let is_long = !(-0x8000..=0x7FFF).contains(&target);
                return Some((target, is_long));
            }
        }
    }
    None
}

/// Parse (d8,PC,Xn*scale) - PC-relative with index register.
///
/// Returns (Xn, target address, scale, is_long). The second element is the
/// full target address as written, *not* the encoded displacement: the PC
/// is not known here, so `encode_ea` performs the subtraction (same split
/// as the non-indexed `(d,PC)` form).
fn parse_parens_disp_pc_index(
    text: &str,
    symbols: &SymbolTable,
    pc: u32,
) -> Option<(u8, i32, u8, bool)> {
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
                    && let Some(reg_num) = index_reg_num(&reg)
                {
                    let target = evaluate_pc_target(disp_str, symbols, pc);
                    return Some((reg_num, target, scale, is_long));
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
fn parse_parens_disp_reg_index(
    text: &str,
    symbols: &SymbolTable,
    pc: u32,
) -> Option<(u8, u8, i8, u8, bool)> {
    let trimmed = text.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let parts: Vec<&str> = inner.split(',').collect();

        // `(An,Xn)` — the displacement may be omitted entirely, meaning 0.
        // This spelling is common in hand-written and generated Amiga code
        // and was rejected outright; only `(0,An,Xn)` parsed.
        let (disp_str, an_str, xn_full) = match parts.len() {
            2 => ("0", parts[0].trim(), parts[1].trim()),
            n if n >= 3 => (parts[0].trim(), parts[1].trim(), parts[2].trim()),
            _ => return None,
        };

        if let Some(Operand::AddrReg(an)) = parse_register(an_str) {
            let (xn_name, scale, is_long) = parse_index_reg_and_scale(xn_full);
            if let Some(reg) = parse_register(&xn_name)
                && let Some(xn) = index_reg_num(&reg)
            {
                let disp = evaluate_displacement(disp_str, symbols, pc);
                let disp_i8 = (disp & 0xFF) as i8;
                return Some((an, xn, disp_i8, scale, is_long));
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
/// Index-register number as the brief-format extension word encodes it:
/// data registers stay 0-7, address registers become 8-15 (the encoder
/// derives the D/A bit from `>= 8`). `Operand::reg_num()` alone can't be
/// used here because it reports A6 as 6, indistinguishable from D6 —
/// which made every address-register index assemble as a data register,
/// e.g. `(a2,a6.w)` encoding an extension word of 0x6000 instead of
/// 0xE000.
fn index_reg_num(reg: &Operand) -> Option<u8> {
    match reg {
        Operand::DataReg(n) => Some(*n),
        Operand::AddrReg(n) => Some(n + 8),
        _ => None,
    }
}

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
    // A leading sign has to be handled before the radix prefix: `-$6` is
    // a perfectly ordinary displacement (and is exactly what this
    // crate's own disassembler emits for negative offsets, e.g.
    // `jsr -$6(a6)` for an Amiga library call), but stripping only `$`
    // left "-" attached to the digits and `from_str_radix` rejected it,
    // so the whole displacement silently became 0 — turning every
    // negative-offset `d(An)` into `0(An)`.
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, text.strip_prefix('+').unwrap_or(text).trim_start()),
    };
    let magnitude = if let Some(hex) = body.strip_prefix('$') {
        i32::from_str_radix(hex, 16).ok()?
    } else if let Some(hex) = body.strip_prefix("0x") {
        i32::from_str_radix(hex, 16).ok()?
    } else if let Some(bin) = body.strip_prefix('%') {
        i32::from_str_radix(bin, 2).ok()?
    } else if let Some(oct) = body.strip_prefix('@') {
        i32::from_str_radix(oct, 8).ok()?
    } else {
        body.parse::<i32>().ok()?
    };
    Some(if negative {
        magnitude.checked_neg()?
    } else {
        magnitude
    })
}

/// Build the scoped name of a local label: `.loop` under global label
/// `draw` becomes `draw.loop`.
///
/// The qualified name keeps a dot so it can never collide with a global
/// label written in source (a global can't start with a dot, and a name
/// like `draw.loop` written literally would qualify to the same thing —
/// which is the behaviour other Motorola assemblers show as well).
fn qualified_local_name(global: &str, local: &str) -> String {
    format!("{}{}", global, local)
}

/// Evaluate a PC-relative operand's target, which may be a bare number or
/// any expression involving symbols (`(target,PC)`, `(buf+4,PC,D1.W)`).
///
/// `evaluate_simple_number` alone only handles literals, so a label in this
/// position silently became 0 — every `(label,PC)` and `(label,PC,Xn)`
/// resolved to a displacement measured from address 0 instead of from the
/// label. Falls back to the full evaluator, and only then to 0, so an
/// unresolved forward reference in pass 1 still behaves as before.
fn evaluate_pc_target(text: &str, symbols: &SymbolTable, pc: u32) -> i32 {
    evaluate_displacement(text, symbols, pc)
}

/// Evaluate a displacement, which may be a literal (`$96`) or any
/// expression over symbols (`DMACON-CUSTOM_BASE`).
///
/// Amiga sources address the custom chip registers as
/// `MOVE.W #x,DMACON-CUSTOM_BASE(A6)`, so a displacement that only
/// accepts literals silently assembles the whole program against offset
/// 0 — writing to the wrong hardware register every time. Falls back to
/// 0 for anything unresolvable so pass 1 can still size the instruction
/// before every symbol is known.
fn evaluate_displacement(text: &str, symbols: &SymbolTable, pc: u32) -> i32 {
    if let Some(n) = evaluate_simple_number(text) {
        return n;
    }
    evaluate_expr_str(text, symbols, pc)
        .map(|v| v as i32)
        .unwrap_or(0)
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
            | "equr"
            | "reg"
            | "incdir"
            | "machine"
            | "cpu"
            | "fpu"
            | "near"
            | "far"
            | "auto"
            | "inline"
            | "einline"
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
            // `IFD`/`IFND` are the Motorola-syntax short spellings, used
            // by the Amiga system headers for their include guards.
            | "ifd"
            | "ifnd"
            | "ifc"
            | "ifnc"
            | "else"
            | "endif"
            // `ENDC` is the Motorola-syntax spelling of ENDIF, used
            // throughout the Amiga system headers.
            | "endc"
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
    /// `OPT` directive flag state, updated as `OPT` lines are processed.
    pub opt: OptState,
    /// Extra directories searched for `INCLUDE` files (the `-I` paths),
    /// tried after the including file's own directory.
    pub include_paths: Vec<PathBuf>,
    /// Results of `IFD`/`IFND` as decided in pass 1, keyed by source line.
    ///
    /// These test whether a symbol is *defined yet*, which is inherently
    /// position-dependent — but by pass 2 the symbol table holds every
    /// symbol in the file, so re-evaluating flips the answer. That broke
    /// the include-guard idiom every Amiga system header uses:
    ///
    /// ```text
    /// IFND EXEC_TYPES_I
    /// EXEC_TYPES_I SET 1
    ///   ... header body ...
    /// ENDC
    /// ```
    ///
    /// Pass 1 correctly took the branch; pass 2 saw `EXEC_TYPES_I` already
    /// defined and skipped the entire body, so the header contributed
    /// nothing. Recording pass 1's verdict keeps both passes consistent.
    conditional_results: HashMap<usize, bool>,
    /// `EQUR` register aliases and `REG` register lists, keyed by
    /// upper-cased name.
    ///
    /// Both are pure textual substitutions — `CNT EQUR D3` makes `CNT` mean
    /// `D3` everywhere a register may appear, and `SAVE REG D0-D3/A0-A2`
    /// does the same for a `MOVEM` list. Verified against the reference:
    /// the aliased and spelled-out forms assemble to identical bytes.
    pub register_aliases: HashMap<String, String>,
    /// Files already spliced in, so each is included only once.
    included_files: std::collections::HashSet<PathBuf>,
    /// Most recent global (non-local) label, used to scope local labels.
    ///
    /// A local label (`.loop`) belongs to the global label above it, so
    /// the same spelling can repeat in every subroutine. Both passes walk
    /// the lines in the same order, so tracking the current global label
    /// while walking is enough to give each local label a unique name —
    /// see [`Assembler::qualify_label`].
    current_global_label: Option<String>,
}

/// Tracks `OPT` directive flags (`OPT flag[n][+-]`, comma-separated,
/// Devpac-style syntax — e.g. `OPT O+,W5-`). Flags are letter-coded,
/// optionally followed by a numeric sub-option, followed by `+` (enable)
/// or `-` (disable). Most OPT flags configure optimizer/listing behavior
/// this assembler doesn't implement as distinct passes (there's no
/// separate "optimize branches" toggle — relaxation always runs); the one
/// directly actionable here is `Wn` (suppress a specific warning number),
/// since warnings are already a numbered concept in some Motorola
/// assemblers' OPT docs. This assembler's warnings aren't numbered
/// (see `ErrorCollector`), so `Wn-` is tracked but not yet wired to
/// filter specific warnings — the flag state is public and queryable
/// so callers/future warning sites can check it.
#[derive(Debug, Clone, Default)]
pub struct OptState {
    /// Plain letter flags without a numeric suffix, e.g. `OPT O+` -> `flags['O'] = true`.
    pub flags: HashMap<char, bool>,
    /// Numbered flags, e.g. `OPT W5-` -> `numbered[('W', 5)] = false`.
    pub numbered: HashMap<(char, u32), bool>,
}

impl OptState {
    /// Parse and apply one comma-separated `OPT` argument list, returning
    /// a warning message for each malformed entry (e.g. missing `+`/`-`,
    /// empty flag letter) instead of silently ignoring it — previously
    /// `OPT` accepted and discarded every argument unconditionally.
    fn apply(&mut self, args: &[String]) -> Vec<String> {
        let mut warnings = Vec::new();
        for raw in args {
            for entry in raw.split(',') {
                let entry = entry.trim();
                if entry.is_empty() {
                    continue;
                }
                match parse_opt_flag(entry) {
                    Some((letter, number, enabled)) => match number {
                        Some(n) => {
                            self.numbered.insert((letter, n), enabled);
                        }
                        None => {
                            self.flags.insert(letter, enabled);
                        }
                    },
                    None => {
                        warnings.push(format!("OPT: ignoring malformed option '{}'", entry));
                    }
                }
            }
        }
        warnings
    }

    /// Whether a specific numbered warning has been disabled via `OPT
    /// Wn-`. Not yet consulted anywhere (this assembler's warnings don't
    /// carry numbers to check against), but exposed for forward
    /// compatibility and direct testing of the OPT-parsing behavior.
    pub fn warning_disabled(&self, n: u32) -> bool {
        self.numbered.get(&('W', n)) == Some(&false)
    }
}

/// Parse a single `OPT` flag entry like `O+`, `W5-`, `D2+` into
/// `(letter, optional_number, enabled)`. Returns `None` for anything not
/// matching `<letter><digits?><+|->`.
fn parse_opt_flag(entry: &str) -> Option<(char, Option<u32>, bool)> {
    let mut chars = entry.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    let rest: String = chars.collect();
    // Split off the final *character*, not the final byte: `rest.len()`
    // counts bytes, so a multi-byte trailing character (any non-ASCII
    // input reaches here — `OPT` takes arbitrary source text) made
    // `split_at` land mid-codepoint and panic.
    let sign = rest.chars().next_back()?;
    let digits = &rest[..rest.len() - sign.len_utf8()];
    let enabled = match sign {
        '+' => true,
        '-' => false,
        _ => return None,
    };
    let number = if digits.is_empty() {
        None
    } else {
        Some(digits.parse::<u32>().ok()?)
    };
    Some((letter, number, enabled))
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
            include_paths: Vec::new(),
            conditional_results: HashMap::new(),
            register_aliases: HashMap::new(),
            included_files: std::collections::HashSet::new(),
            conditional_stack: Vec::new(),
            macro_definitions: HashMap::new(),
            macro_unique_counter: 0,
            sections: SectionManager::new(origin),
            rs_counter: 0,
            opt: OptState::default(),
            current_global_label: None,
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

    /// Emit a single zero pad byte to restore word alignment.
    ///
    /// Recorded as a one-byte instruction (`byte_len = 1`) so the output
    /// writer places exactly one byte and the location counter stays in
    /// step with pass 1's estimate.
    fn emit_alignment_pad(&mut self, line: &ParsedLine) {
        let pc = self.pc;
        self.push_instruction(AssembledInstruction {
            pc,
            words: vec![0],
            line_no: Some(line.line_no),
            source: None,
            byte_len: Some(1),
        });
        self.pc += 1;
    }

    /// Check if the current conditional nesting allows code generation.
    fn is_conditional_active(&self) -> bool {
        self.conditional_stack.last().copied().unwrap_or(true)
    }

    /// Evaluate a conditional directive name and its argument.
    /// Whether a conditional's answer depends on how far assembly has got.
    ///
    /// `IFD`/`IFND` ask whether a symbol is defined *yet*; every other form
    /// evaluates an expression whose value is the same in both passes.
    fn conditional_is_position_dependent(name: &str) -> bool {
        matches!(name, "ifdef" | "ifndef" | "ifd" | "ifnd")
    }

    /// Evaluate a conditional in pass 1 and remember the answer.
    fn eval_conditional_pass1(
        &mut self,
        name: &str,
        arg: &str,
        line_no: usize,
    ) -> Result<bool, AsmError> {
        let result = self.eval_conditional(name, arg, line_no)?;
        if Self::conditional_is_position_dependent(name) {
            self.conditional_results.insert(line_no, result);
        }
        Ok(result)
    }

    /// Evaluate a conditional in a later pass, reusing pass 1's answer for
    /// the position-dependent forms.
    fn eval_conditional_later(
        &self,
        name: &str,
        arg: &str,
        line_no: usize,
    ) -> Result<bool, AsmError> {
        if Self::conditional_is_position_dependent(name)
            && let Some(cached) = self.conditional_results.get(&line_no)
        {
            return Ok(*cached);
        }
        self.eval_conditional(name, arg, line_no)
    }

    fn eval_conditional(&self, name: &str, arg: &str, line_no: usize) -> Result<bool, AsmError> {
        match name {
            "ifdef" | "ifndef" | "ifd" | "ifnd" => {
                let sym = arg.trim();
                let defined = self.symbols.contains(sym);
                Ok(if name == "ifdef" || name == "ifd" {
                    defined
                } else {
                    !defined
                })
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
    /// Add a directory to the `INCLUDE` search path.
    pub fn add_include_path(&mut self, path: PathBuf) {
        self.include_paths.push(path);
    }

    pub fn set_source_root(&mut self, path: PathBuf) {
        self.source_root = path;
    }

    /// Set the CPU target.
    pub fn set_cpu(&mut self, cpu: &str) {
        self.cpu = cpu.to_string();
    }

    /// Location counter after the last assembled line.
    ///
    /// Needed to size binary output, since a trailing `DS`/`DCB` only
    /// advances the PC and emits no instruction to measure.
    pub fn end_pc(&self) -> u32 {
        self.pc
    }

    /// Enable optimizations that change encoding size — currently
    /// shortening absolute addresses that fit in 16 bits.
    ///
    /// Off by default so output matches what Motorola-syntax assemblers
    /// emit without optimization.
    pub fn set_optimize(&mut self, on: bool) {
        self.symbols.set_optimize_absolute(on);
    }

    /// Resolve a label as written in source to the name stored in the
    /// symbol table, scoping local labels (`.loop`) to the enclosing
    /// global label.
    ///
    /// Non-local names pass through unchanged. A local label with no
    /// global label above it also passes through unchanged, so it still
    /// gets a sensible "undefined symbol" diagnostic rather than being
    /// silently renamed.
    fn qualify_label(&self, name: &str) -> String {
        if is_local_label(name)
            && let Some(global) = &self.current_global_label
        {
            return qualified_local_name(global, name);
        }
        name.to_string()
    }

    /// Note a label definition so later local labels scope to it.
    ///
    /// Must be called for every label as each pass walks the lines, in
    /// source order, so both passes derive the same scope for the same
    /// line. Also updates the symbol table's scope, which is what every
    /// lookup path consults.
    fn track_label_scope(&mut self, name: &str) {
        if !is_local_label(name) {
            self.current_global_label = Some(name.to_string());
            self.symbols
                .set_local_scope(self.current_global_label.clone());
        }
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
    /// Expand macros, REPT/IRP/IRPC, etc. Runs [`Self::macro_preprocess_pass`]
    /// repeatedly: a single pass only expands invocations of macros already
    /// known at the point they're scanned, so a macro that calls another
    /// macro previously came out as a literal "unknown mnemonic" error —
    /// the body of the outer macro was substituted in but never re-scanned
    /// for invocations of its own. Bounded by MAX_MACRO_EXPANSION_DEPTH
    /// (rather than iterating until the output stops changing) as the
    /// termination guard against runaway/self-recursive macro expansion,
    /// since comparing successive outputs for equality doesn't by itself
    /// distinguish "fully expanded" from "still changing forever".
    fn macro_preprocess(&mut self, source: &str) -> String {
        const MAX_MACRO_EXPANSION_DEPTH: usize = 64;
        // Definitions must persist across passes, not just within one:
        // a macro's `MACRO...ENDM` block is consumed (removed from the
        // text) by the pass that defines it, so a later pass re-scanning
        // the *expanded* body for further invocations has no text left
        // to rediscover that definition from — it must still be in
        // self.macro_definitions from the pass that originally parsed it.
        self.macro_definitions.clear();
        self.macro_unique_counter = 0;
        let mut current = source.to_string();
        for _ in 0..MAX_MACRO_EXPANSION_DEPTH {
            let (next, any_macro_invoked) = self.macro_preprocess_pass(&current);
            if !any_macro_invoked {
                return next;
            }
            current = next;
        }
        self.errors.warning(
            format!(
                "macro expansion did not terminate within {} passes (possible unbounded recursion)",
                MAX_MACRO_EXPANSION_DEPTH
            ),
            None,
        );
        current
    }

    /// Single pass of macro/REPT/IRP/IRPC expansion. Returns the expanded
    /// source and whether any macro invocation (not REPT/IRP/IRPC, which
    /// don't need a further pass since they don't introduce new macro
    /// definitions) was expanded, so the caller knows whether another pass
    /// could find newly-exposed invocations.
    fn macro_preprocess_pass(&mut self, source: &str) -> (String, bool) {
        let mut any_macro_invoked = false;
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
                        // A malformed hex count used to `unwrap_or(0)` here,
                        // which expanded the block zero times and dropped its
                        // body without a word — `REPT $ZZ` silently assembled
                        // to nothing while the reference assembler rejects it.
                        // Passing the line through is not enough either: `rept`
                        // is a known name in `is_directive_name`, so it lands in
                        // a no-op arm and the body would be emitted once,
                        // unguarded. Report it instead.
                        match operands1[0]
                            .strip_prefix('$')
                            .and_then(|hex| usize::from_str_radix(hex, 16).ok())
                        {
                            Some(n) => n,
                            None => {
                                self.errors.error(
                                    format!("invalid REPT count: '{}'", operands1[0]),
                                    Some(i + 1),
                                );
                                output.push(raw.to_string());
                                i += 1;
                                continue;
                            }
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
                any_macro_invoked = true;
                self.macro_unique_counter += 1;
                let unique_id = self.macro_unique_counter;

                // Build substitution map: \1..\9 from operands1 + \@
                //
                // All nine slots are always substituted, with the empty
                // string for arguments the invocation didn't supply. That
                // is what makes the standard optional-argument idiom work:
                //
                //     LIBINIT   MACRO   * [baseOffset]
                //               IFC     '\1',''
                //
                // Leaving unsupplied slots as a literal `\1` (the previous
                // behaviour) meant the IFC compared against the raw text,
                // took the wrong branch, and left `SET \1` in the output —
                // "invalid SET expression: unexpected character '\'".
                let mut subs: Vec<(String, String)> = Vec::new();
                for idx in 0..9 {
                    let actual = operands1.get(idx).cloned().unwrap_or_default();
                    subs.push((format!("\\{}", idx + 1), actual));
                }
                // Named params: \paramname — same rule, an unsupplied
                // one substitutes to empty rather than staying literal.
                for (idx, pname) in def.params.iter().enumerate() {
                    let actual = operands1.get(idx).cloned().unwrap_or_default();
                    subs.push((format!("\\{}", pname), actual));
                }
                // \@ → unique label fragment, with a leading underscore so
                // the result is a valid identifier even when `\@` is used
                // as a *prefix*. The Amiga system headers do exactly that
                // (`\@BITDEF SET 1<<\3` in exec/types.i), and a bare
                // `0001BITDEF` is not a legal label — it parsed as an
                // unknown mnemonic and stopped the assembly. The reference
                // substitutes `_000001` for the same reason.
                subs.push(("\\@".to_string(), format!("_{:06}", unique_id)));

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

        (output.join("\n"), any_macro_invoked)
    }

    /// Run the two-pass assembly process.
    pub fn assemble(&mut self, source: &str) -> Result<&[AssembledInstruction], AsmError> {
        // INCLUDE is expanded first: an included file may define the
        // macros, constants and structures the rest of the source uses,
        // so it has to be in place before macro expansion and parsing.
        self.included_files.clear();
        let included = self.expand_includes(source, 0)?;
        let expanded = self.macro_preprocess(&included);
        let parsed = parse_source(&expanded);

        // Pass 1: Build symbol table, calculate sizes
        self.pass1(&parsed)?;

        // Branch relaxation loop
        self.relax_branches(&parsed)?;

        // Pass 2: Encode instructions with resolved symbols
        self.pass2(&parsed)?;

        Ok(&self.code)
    }

    /// Test/diagnostic hook: run include + macro expansion only.
    pub fn debug_expand(&mut self, source: &str) -> Result<String, AsmError> {
        self.included_files.clear();
        let included = self.expand_includes(source, 0)?;
        Ok(self.macro_preprocess(&included))
    }

    /// Recursively splice `INCLUDE 'file'` directives into the source.
    ///
    /// Runs before macro expansion so an included file can define macros.
    /// `depth` bounds recursion; a file that includes itself (directly or
    /// through a chain) would otherwise expand forever.
    ///
    /// Each file is spliced in **at most once** per assembly. The Amiga
    /// system headers include their dependencies unconditionally-looking
    /// but guard the *contents* with `IFND FOO_I` / `ENDC`, relying on the
    /// symbol being set by the first inclusion. That guard can't work here:
    /// this expansion runs before pass 1 evaluates any conditional, so the
    /// text would be spliced in repeatedly and the second copy's `EQU`s
    /// would collide ("symbol 'LN' already defined"). Including once has
    /// the same net effect the guards are written to achieve.
    fn expand_includes(&mut self, source: &str, depth: usize) -> Result<String, AsmError> {
        const MAX_INCLUDE_DEPTH: usize = 64;

        // INCDIR has to be handled here rather than in pass 1: includes are
        // expanded in this separate sweep, so by the time pass 1 sees an
        // `incdir` line the `include` below it has already been resolved —
        // and failed.
        if !source
            .lines()
            .any(|l| matches!(split_line(l).1.as_str(), "include" | "incdir"))
        {
            return Ok(source.to_string());
        }
        if depth >= MAX_INCLUDE_DEPTH {
            return Err(AsmError::new(format!(
                "INCLUDE nested more than {} levels deep (circular include?)",
                MAX_INCLUDE_DEPTH
            )));
        }

        let mut out = String::with_capacity(source.len());
        for (idx, raw) in source.lines().enumerate() {
            let (_, mnemonic, size, operands) = split_line(raw);
            if mnemonic == "incdir" {
                // Register the search path and keep the line: pass 1 sees it
                // again as a no-op, and dropping it here would change line
                // numbers in diagnostics.
                let args = build_directive_args(&None, &size, &operands);
                if let Some(arg) = args.first() {
                    let dir = strip_quotes(arg.trim()).to_string();
                    let path = if std::path::Path::new(&dir).is_absolute() {
                        std::path::PathBuf::from(&dir)
                    } else {
                        self.source_root.join(dir)
                    };
                    if !self.include_paths.contains(&path) {
                        self.include_paths.push(path);
                    }
                }
                out.push_str(raw);
                out.push('\n');
                continue;
            }
            if mnemonic != "include" {
                out.push_str(raw);
                out.push('\n');
                continue;
            }

            let args = build_directive_args(&None, &size, &operands);
            let filename = args
                .first()
                .map(|a| strip_quotes(a).to_string())
                .ok_or_else(|| AsmError::with_line("INCLUDE requires a filename", idx + 1))?;
            let path = resolve_include_path_in(&filename, &self.source_root, &self.include_paths)
                .map_err(|e| AsmError::with_line(e.message, idx + 1))?;

            // Already pulled in? Drop the directive and move on.
            let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !self.included_files.insert(canonical) {
                continue;
            }

            let content = std::fs::read_to_string(&path).map_err(|e| {
                AsmError::with_line(
                    format!("cannot read include '{}': {}", path.display(), e),
                    idx + 1,
                )
            })?;

            // Resolve nested includes relative to the including file's own
            // directory, the way C-style include search works — the Amiga
            // headers include their siblings by bare name.
            let saved_root = self.source_root.clone();
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                self.source_root = parent.to_path_buf();
            }
            let nested = self.expand_includes(&content, depth + 1);
            self.source_root = saved_root;

            out.push_str(&nested?);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        Ok(out)
    }

    // -----------------------------------------------------------------------
    // Pass 1: Symbol collection and size calculation
    // -----------------------------------------------------------------------

    /// Pass 1: Collect labels, build symbol table, estimate sizes.
    fn pass1(&mut self, lines: &[ParsedLine]) -> Result<(), AsmError> {
        self.pc = self.origin;
        self.line_pcs.clear();
        self.conditional_stack.clear();
        self.current_global_label = None;
        self.symbols.set_local_scope(None);

        for line in lines {
            self.line_pcs.push((line.line_no, self.pc));

            // Establish the label scope before the conditional handling
            // below can `continue` past it, mirroring pass 2 so both
            // passes resolve a given line's local labels identically.
            if let Some(ref label) = line.label {
                self.track_label_scope(label);
            }

            // Handle IF/ELSE/ENDIF always (manage conditional stack)
            if let LineType::Directive { name, args } = &line.line_type {
                match name.as_str() {
                    "ifc" | "ifnc" => {
                        let arg = args.join(",");
                        let result = self.eval_conditional_pass1(name, &arg, line.line_no)?;
                        let active = self.is_conditional_active() && result;
                        self.conditional_stack.push(active);
                        if let Some(ref label) = line.label {
                            let name = self.qualify_label(label);
                            let section = self.sections.current_section().map(|s| s.kind.name());
                            self.symbols.define_in_section(
                                &name,
                                self.pc,
                                Some(line.line_no),
                                section,
                            )?;
                        }
                        continue;
                    }
                    "if" | "ifeq" | "ifne" | "ifgt" | "iflt" | "ifge" | "ifle" | "ifdef"
                    | "ifndef" | "ifd" | "ifnd" => {
                        let arg = args.first().map(|s| s.as_str()).unwrap_or("");
                        let result = self.eval_conditional_pass1(name, arg, line.line_no)?;
                        let active = self.is_conditional_active() && result;
                        self.conditional_stack.push(active);
                        if let Some(ref label) = line.label {
                            let name = self.qualify_label(label);
                            let section = self.sections.current_section().map(|s| s.kind.name());
                            self.symbols.define_in_section(
                                &name,
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
                            let name = self.qualify_label(label);
                            let section = self.sections.current_section().map(|s| s.kind.name());
                            self.symbols.define_in_section(
                                &name,
                                self.pc,
                                Some(line.line_no),
                                section,
                            )?;
                        }
                        continue;
                    }
                    "endif" | "endc" => {
                        if self.conditional_stack.pop().is_none() {
                            return Err(AsmError::with_line("ENDIF without IF", line.line_no));
                        }
                        if let Some(ref label) = line.label {
                            let name = self.qualify_label(label);
                            let section = self.sections.current_section().map(|s| s.kind.name());
                            self.symbols.define_in_section(
                                &name,
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

            // Handle label (skip for directives that own their label:
            // EQU/SET define a value, EQUR/REG define a register alias.
            // Defining those as ordinary labels too would put them in the
            // symbol table pointing at the current PC, which is both wrong
            // and confusing in `--sym` output.)
            if let Some(ref label) = line.label {
                let is_equ_or_set = matches!(&line.line_type,
                    LineType::Directive { name, .. }
                    if name == "equ" || name == "set" || name == "equr" || name == "reg"
                );
                if !is_equ_or_set {
                    let name = self.qualify_label(label);
                    let section = self.sections.current_section().map(|s| s.kind.name());
                    self.symbols
                        .define_in_section(&name, self.pc, Some(line.line_no), section)?;
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
                // Mirror encode_instruction_line's alignment pad: an
                // instruction after an odd-length DC.B gets a pad byte, so
                // pass 1 must charge for it too or every following label
                // is one byte off.
                let pad = if self.pc.is_multiple_of(2) { 0 } else { 1 };

                // For branch instructions, record them for relaxation
                let is_branch = is_branch_mnemonic(mnemonic);
                if is_branch {
                    // Estimate with word-sized branch (conservative)
                    let estimated_size = estimate_branch_size(mnemonic);
                    // DBcc takes `Dn,label`, so the target text is the
                    // second operand; plain Bcc/BRA/BSR take the target
                    // as their only operand.
                    let target_text = if is_dbcc_mnemonic(mnemonic) {
                        operand_texts.get(1).cloned()
                    } else {
                        operand_texts.first().cloned()
                    };
                    // An explicit size suffix pins the branch form and
                    // must survive into relaxation: `BRA.S loop` stays
                    // short even with optimization off, and `BRA.W loop`
                    // stays word even when the target is in short range.
                    // Dropping it here made every suffix advisory.
                    let size_hint = match size.as_deref() {
                        Some("s") | Some("b") => BranchSize::Short,
                        Some("w") => BranchSize::Word,
                        Some("l") => BranchSize::Long,
                        _ => BranchSize::Any,
                    };
                    let estimated_size = if size_hint == BranchSize::Any {
                        estimated_size
                    } else {
                        branch_size_bytes(mnemonic, &size_hint)
                    };
                    self.branches.push(BranchInfo {
                        instr_index: self.code.len(),
                        mnemonic: mnemonic.clone(),
                        target_text,
                        size_hint,
                        line_no: Some(line.line_no),
                        local_scope: self.current_global_label.clone(),
                    });
                    return Ok(estimated_size + pad);
                }

                // For other instructions, try to encode with dummy values
                // to estimate size
                self.estimate_instruction_size_from_texts(mnemonic, size.as_deref(), operand_texts)
                    .map(|n| n + pad)
            }
        }
    }

    /// Estimate size for a directive.
    /// Shared CNOP argument evaluation, validation, and padding calculation
    /// for both passes.
    ///
    /// Pass 1 (sizing) and pass 2 (encoding) previously carried two separately
    /// written copies of this. They agreed numerically, but that is exactly
    /// the divergence class that produced six label corruptions in the July
    /// audit, so the arithmetic now exists once.
    ///
    /// The validation deliberately runs on every call rather than being
    /// treated as carried over from pass 1: if the alignment expression
    /// depends on a `SET` symbol whose value changed between passes, pass 1
    /// may have validated a different value, and `target % alignment` would
    /// divide by zero on an alignment of 0.
    fn cnop_padding(&self, args: &[String], line_no: usize) -> Result<u32, AsmError> {
        if args.len() < 2 {
            return Err(AsmError::with_line(
                "CNOP requires offset and alignment",
                line_no,
            ));
        }
        let offset = evaluate_expr_str(&args[0], &self.symbols, self.pc)
            .map_err(|e| AsmError::with_line(e.message, line_no))? as u32;
        let alignment = evaluate_expr_str(&args[1], &self.symbols, self.pc)
            .map_err(|e| AsmError::with_line(e.message, line_no))? as u32;
        if alignment == 0 || !alignment.is_power_of_two() {
            return Err(AsmError::with_line(
                format!("CNOP alignment must be power of 2, got {}", alignment),
                line_no,
            ));
        }
        // CNOP offset,align → aligns to (PC + offset) % align == 0
        let target = self.pc + offset;
        Ok(if target.is_multiple_of(alignment) {
            0
        } else {
            alignment - (target % alignment)
        })
    }

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
            // `NAME EQUR Dn` and `NAME REG D0-D3/A0-A2`: textual aliases for
            // a register and for a register list. Both are substituted
            // wherever a register may appear, so the aliased source encodes
            // exactly like the spelled-out one.
            "equr" | "reg" => {
                let directive = name.to_uppercase();
                let Some(alias) = label else {
                    return Err(AsmError::with_line(
                        format!("{} requires a label", directive),
                        line_no,
                    ));
                };
                let value = args.join(",");
                if value.trim().is_empty() {
                    return Err(AsmError::with_line(
                        format!("{} requires a register or register list", directive),
                        line_no,
                    ));
                }
                self.register_aliases
                    .insert(alias.to_uppercase(), value.trim().to_string());
                Ok(0)
            }
            "dc" => {
                // DC.B/W/L - must match encode_dc's byte counting exactly,
                // since a diverging estimate here desyncs every label that
                // follows (Pass 1 previously counted string/char literal
                // *arguments* instead of the bytes they expand to).
                let size_suffix = args.first().map(|s| s.as_str()).unwrap_or("w");
                let element_size: u32 = match size_suffix {
                    "b" => 1,
                    "w" => 2,
                    "l" => 4,
                    "s" => 4,
                    "d" => 8,
                    "x" | "p" => 12,
                    _ => 2,
                };
                let values = if args.len() > 1 { &args[1..] } else { &[] };
                let mut total: u32 = 0;
                for value_str in values {
                    let trimmed = value_str.trim();
                    if matches!(size_suffix, "s" | "d" | "x" | "p") {
                        total = total.saturating_add(element_size);
                    } else if trimmed.starts_with('"') || trimmed.starts_with('\'') {
                        let bytes = crate::directives::parse_dc_string(trimmed).map_err(|e| {
                            AsmError::with_line(format!("invalid DC string: {}", e), line_no)
                        })?;
                        total = total.saturating_add(bytes.len() as u32);
                    } else {
                        total = total.saturating_add(element_size);
                    }
                }
                // No rounding: `encode_dc` advances the PC by the exact
                // byte count, so this estimate must too. Rounding up to a
                // word made consecutive `DC.B`s each start on an even
                // address — `dc.b $5 / dc.b $EE` assembled to `05 00 EE 00`
                // instead of `05 EE`, and every label after a byte-sized
                // DC drifted. (The `ds` arm below had the same bug fixed
                // earlier; DC kept it.)
                Ok(total)
            }
            "ds" => {
                // DS.B/W/L - reserve space. Must match encode_ds exactly:
                // that function does NOT round up to an even address, so
                // this estimate mustn't either (a prior mismatch here made
                // every label after an odd-sized DS.B off by one byte).
                let size_suffix = args.first().map(|s| s.as_str()).unwrap_or("w");
                let element_size: u32 = match size_suffix {
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
                let total = element_size
                    .checked_mul(count)
                    .ok_or_else(|| AsmError::with_line("DS size too large", line_no))?;
                Ok(total)
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
                let element_size: u32 = match size_suffix {
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
                let total = element_size
                    .checked_mul(count)
                    .ok_or_else(|| AsmError::with_line("DCB size too large", line_no))?;
                Ok(total.saturating_add(1) & !1)
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
            // MACHINE/FPU let a source declare its own target, so a file
            // saying `machine 68020` assembles without `-c 68020` on the
            // command line — which is how the reference behaves, and how
            // sources that carry their own requirements are written.
            "machine" | "cpu" => {
                if let Some(arg) = args.first() {
                    let name = arg.trim().trim_start_matches("mc").to_lowercase();
                    if name == "any" {
                        self.cpu = "68060".to_string();
                    } else {
                        m68k_core::cpu_gate::validate_cpu_name(&name).map_err(|e| {
                            AsmError::with_line(format!("invalid MACHINE argument: {}", e), line_no)
                        })?;
                        self.cpu = name;
                    }
                }
                Ok(0)
            }
            // `FPU <n>`: a non-zero argument enables coprocessor
            // instructions. Those are gated on the CPU level here, so this
            // raises the floor to a model that has an FPU rather than
            // tracking a separate flag.
            "fpu" => {
                let enabled = match args.first() {
                    Some(arg) => evaluate_expr_str(arg, &self.symbols, self.pc).unwrap_or(1) != 0,
                    None => true,
                };
                if enabled && matches!(self.cpu.as_str(), "68000" | "68010" | "68020") {
                    self.cpu = "68030".to_string();
                }
                Ok(0)
            }
            "incdir" => {
                if let Some(arg) = args.first() {
                    let dir = crate::directives::strip_quotes(arg.trim());
                    // Relative to the including file, matching how INCLUDE
                    // itself resolves — an INCDIR in a source moved into a
                    // subdirectory should still find its own headers.
                    let path = if std::path::Path::new(&dir).is_absolute() {
                        std::path::PathBuf::from(&dir)
                    } else {
                        self.source_root.join(dir)
                    };
                    if !self.include_paths.contains(&path) {
                        self.include_paths.push(path);
                    }
                }
                Ok(0)
            }
            "rsset" => {
                if let Some(arg) = args.first() {
                    self.rs_counter = evaluate_expr_str(arg, &self.symbols, self.pc)? as u32;
                }
                Ok(0)
            }
            "if" | "ifeq" | "ifne" | "ifgt" | "iflt" | "ifge" | "ifle" | "ifdef" | "ifndef"
            | "ifc" | "ifnc" | "else" | "endif" | "endc" | "macro" | "endm" | "rept" | "irp"
            | "irpc" | "endr" | "xref" | "xdef" | "public" | "extern" | "opt" | "mexit"
            | "exitm" | "print" | "printt" | "printv" | "list" | "nolist" | "page" | "title" => {
                Ok(0)
            }
            // Optimisation and code-placement hints. This assembler resolves
            // everything to absolute addresses and performs no branch
            // shortening beyond the relaxation pass, so there is nothing for
            // these to control — but rejecting them means a source that
            // merely mentions them will not assemble at all.
            "near" | "far" | "auto" | "inline" | "einline" => Ok(0),
            "cnop" => {
                let padding = self.cnop_padding(args, line_no)?;
                if padding > 0 {
                    self.pc += padding;
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
        // Aliases must be resolved here too: pass 1 sizes the instruction
        // and pass 2 encodes it, so an alias visible to only one of them
        // would desync every label that follows.
        let resolved: Vec<String>;
        let operand_texts: &[String] = if self.register_aliases.is_empty() {
            operand_texts
        } else {
            resolved = operand_texts
                .iter()
                .map(|t| self.apply_register_aliases(t))
                .collect();
            &resolved
        };
        // `Operand::Address(0)` is parse_operand_text's fallback for a bare
        // symbol it couldn't resolve yet (undefined forward reference) — it
        // is NOT a real "address 0" operand. Widening it to AbsoluteLong
        // here forces the conservative (largest) EA encoding size during
        // estimation; without this, a forward-referenced symbol that later
        // resolves above 0xFFFF silently grows by 2 bytes in pass 2 and
        // desyncs every label after it (the value 0 always fits in the
        // short/Absolute.W encoding, so pass 1 would otherwise underestimate).
        let widen_forward_ref = |op: Operand| -> Operand {
            match op {
                Operand::Address(0) => Operand::AbsoluteLong(0),
                other => other,
            }
        };
        let src = if !operand_texts.is_empty() {
            parse_operand_text(&operand_texts[0], &self.symbols, self.pc)
                .ok()
                .map(widen_forward_ref)
        } else {
            None
        };
        let dst = if operand_texts.len() > 1 {
            parse_operand_text(&operand_texts[1], &self.symbols, self.pc)
                .ok()
                .map(widen_forward_ref)
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
                let (mnemonic, line_no, size_hint, target_text, scope) = {
                    let b = &self.branches[i];
                    (
                        b.mnemonic.clone(),
                        b.line_no,
                        b.size_hint,
                        b.target_text.clone(),
                        b.local_scope.clone(),
                    )
                };
                let branch_pc = self.get_pc_for_line(line_no)?;
                // Resolve the target in the scope the branch was written
                // in; `recalculate_pcs` above left the scope at the last
                // label in the file, which is the wrong one for a local
                // target and made every `BRA .local` fall back to a
                // stale/absent address.
                self.symbols.set_local_scope(scope);
                let target_addr = target_text
                    .as_deref()
                    .and_then(|t| evaluate_expr_str(t, &self.symbols, branch_pc).ok())
                    .map(|v| v as u32);

                if let Some(target) = target_addr {
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
        self.pc = self.origin;
        let mut cond_stack: Vec<bool> = Vec::new();
        // Rebuild the local-label scope from the top, as both passes do.
        self.current_global_label = None;
        self.symbols.set_local_scope(None);

        for line in lines {
            self.line_pcs.push((line.line_no, self.pc));
            if let Some(ref label) = line.label {
                self.track_label_scope(label);
            }

            // Handle conditional directives
            let mut skip = false;
            if let LineType::Directive { name, args } = &line.line_type {
                match name.as_str() {
                    "ifc" | "ifnc" => {
                        let arg = args.join(",");
                        let active = cond_stack.last().copied().unwrap_or(true);
                        let result = self
                            .eval_conditional_later(name, &arg, line.line_no)
                            .unwrap_or(false);
                        cond_stack.push(active && result);
                        skip = true;
                    }
                    "if" | "ifeq" | "ifne" | "ifgt" | "iflt" | "ifge" | "ifle" | "ifdef"
                    | "ifndef" | "ifd" | "ifnd" => {
                        let arg = args.first().map(|s| s.as_str()).unwrap_or("");
                        let active = cond_stack.last().copied().unwrap_or(true);
                        let result = self
                            .eval_conditional_later(name, arg, line.line_no)
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
                    "endif" | "endc" => {
                        cond_stack.pop();
                        skip = true;
                    }
                    _ => {}
                }
            }

            if !skip && cond_stack.last().copied().unwrap_or(true) {
                // EQU/SET name a *value*, not a location, so they must be
                // left alone here — overwriting them with the current PC
                // turned every constant into its own address. Pass 1 has
                // the same exemption; this loop was missing it, so any
                // source with an `EQU` (i.e. essentially every real Amiga
                // program, which equates the hardware registers) silently
                // got zeroes wherever it used one.
                // EQUR/REG own their labels too — they name a register, not
                // an address, and must not be entered as ordinary symbols.
                let is_equ_or_set = matches!(&line.line_type,
                    LineType::Directive { name, .. }
                    if name == "equ" || name == "set" || name == "equr" || name == "reg"
                );
                if let Some(ref label) = line.label
                    && !is_equ_or_set
                {
                    // Each relaxation iteration recomputes PCs from scratch,
                    // so re-defining an already-defined symbol here must
                    // overwrite it rather than error out (define() would
                    // silently no-op on iteration 2+, freezing the label at
                    // its iteration-1 value even as branch sizes change).
                    // Section must be preserved too (ELF/IEEE-695 output
                    // rely on it), so use the section-aware overwrite.
                    // Local labels must be qualified the same way pass 1
                    // qualified them, or this writes a second, unscoped
                    // entry (`.loop`) that later lookups find first.
                    let name = self.qualify_label(label);
                    let section = self.sections.current_section().map(|s| s.kind.name());
                    self.symbols
                        .force_set_in_section(&name, self.pc, Some(line.line_no), section);
                }
                // estimate_line_size_with_branches (via estimate_line_size)
                // updates self.pc directly for directives like ORG/SECTION
                // that reposition the location counter rather than just
                // advancing it — so self.pc, not a local copy, must be the
                // single source of truth here.
                if let Ok(size) = self.estimate_line_size_with_branches(line) {
                    self.pc += size;
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
    ///
    /// Sizes only ever grow across relaxation iterations, never shrink:
    /// once a branch has been widened to Word, it stays Word even if a
    /// later recalculation (based on that same widening) would make Short
    /// look sufficient again. Without this monotonicity, a branch whose
    /// target sits exactly at the reserved disp==0/-1 boundary can flip
    /// Short -> Word -> Short -> Word forever, since growing to Word moves
    /// the target closer (shrinking disp back into short range) and
    /// shrinking back to Short reopens the same disp==0/-1 collision.
    fn determine_branch_size(
        &self,
        mnemonic: &str,
        branch_pc: u32,
        target: u32,
        hint: BranchSize,
    ) -> BranchSize {
        // DBcc always uses word displacement. `mnemonic` here is
        // lowercase (as produced by the parser), matching is_dbcc_mnemonic.
        if is_dbcc_mnemonic(mnemonic) {
            return BranchSize::Word;
        }

        // An explicit suffix wins over both the range check and the
        // optimization setting — including Short, which the user asked
        // for by writing `.S`/`.B`.
        if hint != BranchSize::Any {
            return hint;
        }

        // For BRA/BSR/Bcc, disp==0/-1 are excluded from the short range:
        // their low byte (0x00/0xFF) collides with the word/long-form
        // markers, so the encoder falls through to the word form for
        // those displacements (see encode_bra/encode_bsr/enc_bcc) — the
        // size estimate must agree.
        let disp = target as i32 - branch_pc as i32 - 2;

        // Shrinking a branch to its 8-bit form is an optimization, so it
        // only happens when optimization is on — without it, Motorola-
        // syntax assemblers emit the word form for an unsuffixed branch
        // (`BRA label` -> 6000 fffc, not 60fc). An explicit `.S`/`.B`
        // suffix arrives here as a Short hint and is honoured either way.
        if self.symbols.optimize_absolute()
            && (-128..=127).contains(&disp)
            && disp != 0
            && disp != -1
        {
            BranchSize::Short
        } else if (-32768..=32767).contains(&disp) {
            BranchSize::Word
        } else if self.cpu == "68000" {
            // 68000 has no 32-bit branch displacement form; encode_bra/
            // encode_bsr/enc_bcc's word-form fallback will surface this
            // as an out-of-range error in pass 2 (matching the previous
            // behavior of always picking Word here and letting the
            // encoder reject it).
            BranchSize::Word
        } else {
            // Previously this estimate had no long-form branch: a
            // Bcc/BRA/BSR target more than 32KB away always got the Word
            // hint, so pass 1 (and the relaxation loop, which never grows
            // past what determine_branch_size returns) never accounted
            // for the extra 2 bytes the 68020+ 32-bit form needs —
            // desyncing every label after it once such a branch existed.
            BranchSize::Long
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
        // Local labels resolve against the enclosing global label, so the
        // scope has to be rebuilt from the start of the source, exactly as
        // pass 1 built it.
        self.current_global_label = None;
        self.symbols.set_local_scope(None);

        for line in lines {
            // Track the label scope before anything can `continue` past
            // it: a local label referenced on this line must resolve
            // against the global label above it, and the conditional
            // branches below skip the rest of the loop body.
            if let Some(ref label) = line.label {
                self.track_label_scope(label);
            }

            // Handle IF/ELSE/ENDIF always (manage conditional stack)
            if let LineType::Directive { name, args } = &line.line_type {
                match name.as_str() {
                    "ifc" | "ifnc" => {
                        let arg = args.join(",");
                        let result = self.eval_conditional_later(name, &arg, line.line_no)?;
                        let active = self.is_conditional_active() && result;
                        self.conditional_stack.push(active);
                        continue;
                    }
                    "if" | "ifeq" | "ifne" | "ifgt" | "iflt" | "ifge" | "ifle" | "ifdef"
                    | "ifndef" | "ifd" | "ifnd" => {
                        let arg = args.first().map(|s| s.as_str()).unwrap_or("");
                        let result = self.eval_conditional_later(name, arg, line.line_no)?;
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
                    "endif" | "endc" => {
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
    /// Substitute `EQUR`/`REG` aliases in an operand.
    ///
    /// Whole identifiers only: an alias named `A` must not rewrite the `A`
    /// inside `ADDR` or `(A0)`. Substitution is one level deep, which is
    /// what the directives mean — an alias names a register, not another
    /// alias.
    fn apply_register_aliases(&self, text: &str) -> String {
        if self.register_aliases.is_empty() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut ident = String::new();
        let flush = |ident: &mut String, out: &mut String, aliases: &HashMap<String, String>| {
            if !ident.is_empty() {
                match aliases.get(&ident.to_uppercase()) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(ident),
                }
                ident.clear();
            }
        };
        for ch in text.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
                ident.push(ch);
            } else {
                flush(&mut ident, &mut out, &self.register_aliases);
                out.push(ch);
            }
        }
        flush(&mut ident, &mut out, &self.register_aliases);
        out
    }

    fn encode_instruction_line(
        &mut self,
        mnemonic: &str,
        size: Option<&str>,
        operand_texts: &[String],
        line: &ParsedLine,
    ) -> Result<(), AsmError> {
        // Instructions must start on a word boundary — the 68000 faults on
        // a misaligned instruction fetch. An odd-length `DC.B` run leaves
        // the location counter odd, so emit the pad byte other Motorola
        // assemblers insert here rather than encoding at an odd address.
        if !self.pc.is_multiple_of(2) {
            self.emit_alignment_pad(line);
        }
        let pc = self.pc;

        // Resolve EQUR/REG aliases once, up front, so every path below —
        // operand parsing, the branch and MOVEM special cases, the generic
        // encoder — works on real register names and needs no knowledge of
        // aliases at all.
        let resolved: Vec<String>;
        let operand_texts: &[String] = if self.register_aliases.is_empty() {
            operand_texts
        } else {
            resolved = operand_texts
                .iter()
                .map(|t| self.apply_register_aliases(t))
                .collect();
            &resolved
        };

        // Central CPU-gating check, before any mnemonic-specific dispatch:
        // covers the branch path, the 3-operand special forms (CAS/CAS2/
        // PACK/UNPK/PFLUSH/PTESTR/PTESTW), and the generic encoder.rs
        // dispatch in one place, replacing the previously scattered (and
        // inconsistent, some missing entirely) per-encoder cpu checks.
        m68k_core::cpu_gate::check_mnemonic_cpu(&mnemonic.to_uppercase(), &self.cpu)
            .map_err(|msg| AsmError::with_line(msg, line.line_no))?;

        // Parse operands. A parse failure is kept rather than discarded:
        // by pass 2 every symbol is known, so an operand that still fails
        // to parse is a real error — usually an undefined symbol. Reporting
        // "MOVE requires source and destination" for `move.l #UNDEF,d1`
        // sent the reader looking at the wrong thing entirely; the
        // reference names the missing symbol. The error is surfaced only if
        // encoding actually fails, since the branch paths below legitimately
        // work with an unparsed operand.
        let mut operand_error: Option<AsmError> = None;
        let mut parse_operand = |text: &str| match parse_operand_text(text, &self.symbols, pc) {
            Ok(op) => Some(op),
            Err(e) => {
                operand_error.get_or_insert(e);
                None
            }
        };
        let src = operand_texts.first().and_then(|t| parse_operand(t));
        let dst = operand_texts.get(1).and_then(|t| parse_operand(t));

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
                // Three valid spellings across two architectures:
                //   68040+: `PFLUSH (An)`          — one operand
                //   68030 : `PFLUSH #fc,#mask`     — no EA
                //   68030 : `PFLUSH #fc,#mask,<ea>`
                // Requiring three operands rejected the other two.
                if operand_texts.len() == 1 {
                    // 68040+ single-operand form: `PFLUSH (An)`.
                    // Reference encoding: `pflush (a0)` -> F508.
                    let reg_op = parse_operand_text(&operand_texts[0], &self.symbols, pc)
                        .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                    let w = crate::enc_mmu::enc_mmu_single_reg(0xF508, &reg_op, &self.cpu)
                        .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                    self.push_instruction(AssembledInstruction {
                        pc,
                        words: w.clone(),
                        line_no: Some(line.line_no),
                        source: Some(line.raw.clone()),
                        byte_len: None,
                    });
                    self.pc += (w.len() * 2) as u32;
                    return Ok(());
                }
                if operand_texts.len() != 2 && operand_texts.len() != 3 {
                    return Err(AsmError::with_line(
                        "PFLUSH takes (An), #fc,#mask or #fc,#mask,<ea>".to_string(),
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
                if operand_texts.len() == 2 {
                    crate::enc_mmu::enc_pflush_no_ea(fc, mask, &self.cpu)
                        .map_err(|e| AsmError::with_line(e.message, line.line_no))?
                } else {
                    let ea = parse_operand_text(&operand_texts[2], &self.symbols, pc)
                        .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                    crate::enc_mmu::enc_pflush(fc, mask, &ea, pc + 2, &self.cpu)
                        .map_err(|e| AsmError::with_line(e.message, line.line_no))?
                }
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
                if operand_texts.len() != 3 {
                    return Err(AsmError::with_line(
                        "CAS takes exactly 3 operands: Dc,Du,<ea>".to_string(),
                        line.line_no,
                    ));
                }
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
                if operand_texts.len() != 3 {
                    return Err(AsmError::with_line(
                        format!(
                            "{} takes exactly 3 operands: src,dst,#adjustment",
                            mnemonic_upper
                        ),
                        line.line_no,
                    ));
                }
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
            "FMOVE" if operand_texts.len() == 2 && has_kfactor_suffix(&operand_texts[1]) => {
                let (dst_text, kfactor) = split_kfactor_suffix(&operand_texts[1])
                    .map_err(|e| AsmError::with_line(e, line.line_no))?;
                let s = parse_operand_text(&operand_texts[0], &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                let d = parse_operand_text(dst_text, &self.symbols, pc)
                    .map_err(|e| AsmError::with_line(e.message, line.line_no))?;
                crate::enc_fpu::enc_fmove(&s, &d, size, Some(kfactor), pc + 2, &self.cpu)
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
            .map_err(|e| {
                // If an operand failed to parse, that is the real cause;
                // the encoder only sees a missing operand and reports the
                // instruction's shape, which points at the wrong thing.
                let cause = operand_error.take().unwrap_or(e);
                AsmError::with_line(cause.message, line.line_no)
            })?,
        };
        let word_count = words.len();

        self.push_instruction(AssembledInstruction {
            pc,
            words,
            line_no: Some(line.line_no),
            source: Some(line.raw.clone()),
            byte_len: None,
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
                // Bcc family - delegate to enc_flow. The short form is
                // only chosen when the source asked for it (`.S`/`.B`) or
                // optimization is on, matching BRA/BSR above.
                let cond = branch_condition(mnemonic)?;
                let allow_short = size_hint == BranchSize::Short
                    || (size_hint == BranchSize::Any && self.symbols.optimize_absolute());
                crate::enc_flow::enc_bcc_sized(&cond, target as i32, pc + 2, &self.cpu, allow_short)
                    .map_err(|e| AsmError::with_line(e.message.clone(), line.line_no))?
            }
        };
        let word_count = words.len();

        self.push_instruction(AssembledInstruction {
            pc,
            words,
            line_no: Some(line.line_no),
            source: Some(line.raw.clone()),
            byte_len: None,
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
            byte_len: None,
        });

        self.pc += (word_count * 2) as u32;
        Ok(())
    }

    /// Encode BRA with size hint.
    fn encode_bra(&self, disp: i32, size_hint: BranchSize) -> Result<Vec<u16>, AsmError> {
        // disp==0/-1 collide with the word/long-form low-byte markers
        // (0x00/0xFF) and must fall through to the word form instead.
        match size_hint {
            BranchSize::Short | BranchSize::Any
                if (-128..=127).contains(&disp) && disp != 0 && disp != -1 =>
            {
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

        // 68020+ long displacement: low byte 0xFF marks the 32-bit form
        // (0x00 marks the 16-bit form above), immediately followed by the
        // 32-bit displacement - no extra padding word. Verified against
        // reference output for `bra.l far`.
        if self.cpu != "68000" {
            let op = 0x60FF;
            return Ok(vec![
                op,
                ((disp >> 16) & 0xFFFF) as u16,
                (disp & 0xFFFF) as u16,
            ]);
        }

        Err(AsmError::new("BRA displacement out of range for 68000"))
    }

    /// Encode BSR with size hint.
    fn encode_bsr(&self, disp: i32, size_hint: BranchSize) -> Result<Vec<u16>, AsmError> {
        // See encode_bra above: disp==0/-1 must fall through to word form.
        match size_hint {
            BranchSize::Short | BranchSize::Any
                if (-128..=127).contains(&disp) && disp != 0 && disp != -1 =>
            {
                let op = 0x6100 | ((disp & 0xFF) as u16);
                return Ok(vec![op]);
            }
            _ => {}
        }

        if (-32768..=32767).contains(&disp) {
            let op = 0x6100;
            return Ok(vec![op, (disp & 0xFFFF) as u16]);
        }

        // 68020+ long displacement: same 0xFF-low-byte marker as encode_bra above.
        if self.cpu != "68000" {
            let op = 0x61FF;
            return Ok(vec![
                op,
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
            | "ifc" | "ifnc" | "else" | "endif" | "endc" | "macro" | "endm" | "rept" | "irp"
            | "irpc" | "endr" | "xref" | "xdef" | "public" | "extern" | "mexit" | "exitm"
            | "near" | "far" | "auto" | "inline" | "einline" | "machine" | "cpu" | "fpu"
            | "incdir" | "equr" | "reg" | "list" | "nolist" | "page" | "title" => Ok(()),
            "opt" => {
                for msg in self.opt.apply(args) {
                    self.errors.warning(msg, Some(line.line_no));
                }
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
                // Pass 2: emit the padding sized by `cnop_padding`, which is
                // the same code pass 1 used — see its doc comment for why the
                // validation re-runs here rather than being carried over.
                let padding = self.cnop_padding(args, line.line_no)?;
                if padding > 0 {
                    // An odd PC is squared up with a single zero byte
                    // first; only whole words after that become NOPs.
                    // Rounding the byte count up to an even number and
                    // filling it all with 0x4E71 both overshot the target
                    // address and wrote a NOP at an odd offset.
                    // Build the byte sequence, then pack it into words:
                    // the leading zero byte occupies half a word, so the
                    // NOPs that follow straddle word boundaries and can't
                    // simply be pushed as 0x4E71 words.
                    let mut pad_bytes: Vec<u8> = Vec::new();
                    if !self.pc.is_multiple_of(2) {
                        pad_bytes.push(0x00);
                    }
                    while (pad_bytes.len() as u32) + 1 < padding {
                        pad_bytes.push(0x4E);
                        pad_bytes.push(0x71);
                    }
                    while (pad_bytes.len() as u32) < padding {
                        pad_bytes.push(0x00);
                    }
                    let mut words = Vec::new();
                    for chunk in pad_bytes.chunks(2) {
                        let hi = chunk[0] as u16;
                        let lo = chunk.get(1).copied().unwrap_or(0) as u16;
                        words.push((hi << 8) | lo);
                    }
                    self.push_instruction(AssembledInstruction {
                        pc: self.pc,
                        words,
                        line_no: Some(line.line_no),
                        source: Some(line.raw.clone()),
                        byte_len: Some(padding as usize),
                    });
                    self.pc += padding;
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

        // An odd byte count is kept as-is: `byte_len` below records the
        // true length and the output writer stops there, so the next
        // directive starts on the very next byte. Padding to an even
        // length here is what made `dc.b $5 / dc.b $EE` emit
        // `05 00 EE 00` rather than the `05 EE` other Motorola
        // assemblers produce. The trailing half-word still needs its low
        // byte cleared so nothing stale leaks into the image.
        if !total_bytes.is_multiple_of(2)
            && let Some(last) = words.last_mut()
        {
            *last &= 0xFF00;
        }

        let start_pc = self.pc;
        self.push_instruction(AssembledInstruction {
            pc: start_pc,
            words,
            line_no: Some(line.line_no),
            source: Some(line.raw.clone()),
            // DC.B may emit an odd byte count; record it so the next
            // directive lands on the following byte instead of being
            // pushed to the next word boundary. Motorola assemblers pack
            // consecutive DC.B directives tightly.
            byte_len: Some(total_bytes),
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

        let element_size: u32 = match size_suffix.as_str() {
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
        let total_bytes = element_size
            .checked_mul(count)
            .ok_or_else(|| AsmError::with_line("DS size too large", line.line_no))?;
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

        let element_size: u32 = match size_suffix.as_str() {
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
        // Reject before allocating: element_size * count can overflow u32
        // (panicking on a crafted DCB.L $80000000,0) and, even unchecked,
        // a huge count would otherwise drive an unbounded Vec allocation
        // below.
        element_size
            .checked_mul(count)
            .ok_or_else(|| AsmError::with_line("DCB size too large", line.line_no))?;

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
            byte_len: None,
        });

        let total_bytes = element_size * count;
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
            // A raw hex literal ($...) always means "these are the literal
            // 96-bit contents", for either format — bypasses conversion
            // entirely, e.g. for embedding pre-computed constants.
            let text = text.trim();
            if let Some(hex) = text.strip_prefix('$') {
                let val = u128::from_str_radix(hex, 16).map_err(|_| {
                    AsmError::with_line(format!("invalid hex literal: {}", text), line_no)
                })?;
                return Ok(u128_to_96bit_words(val));
            }

            if format == "x" {
                // IEEE-754 80-bit extended precision, stored in the
                // Motorola 96-bit DC.X slot: word0 = sign+exponent,
                // word1 = reserved (0), words 2-5 = 64-bit mantissa with
                // explicit integer bit.
                let val: f64 = text.parse().map_err(|_| {
                    AsmError::with_line(format!("invalid DC.X literal: {}", text), line_no)
                })?;
                Ok(f64_to_extended_words(val))
            } else {
                // Motorola packed-BCD decimal: word0 bit15 = sign,
                // bits 11-0 = signed exponent (BCD, sign in bit 11),
                // words 1-5 = 17 BCD mantissa digits (first digit alone
                // in bits 3-0 of word1, implied decimal point after it).
                packed_decimal_words(text, line_no)
            }
        }
        _ => Err(AsmError::with_line(
            format!("unsupported float format: {}", format),
            line_no,
        )),
    }
}

/// Split a raw 96-bit (12-byte) value into six big-endian 16-bit words, for
/// the DC.X/DC.P raw-hex-literal path.
fn u128_to_96bit_words(val: u128) -> Vec<u16> {
    vec![
        ((val >> 80) & 0xFFFF) as u16,
        ((val >> 64) & 0xFFFF) as u16,
        ((val >> 48) & 0xFFFF) as u16,
        ((val >> 32) & 0xFFFF) as u16,
        ((val >> 16) & 0xFFFF) as u16,
        (val & 0xFFFF) as u16,
    ]
}

/// Convert an `f64` to the 96-bit (6-word) Motorola DC.X representation:
/// IEEE-754 80-bit extended precision (1 sign + 15 exponent bits, then an
/// explicit-integer-bit 64-bit mantissa) padded to 96 bits with a reserved
/// zero word after the sign/exponent word, matching Devpac's DC.X
/// layout (word0 = sign+exp, word1 = reserved, words 2-5 = mantissa).
///
/// Unlike the double -> extended conversion previously removed from
/// `m68k-core::floats` (which round-tripped through a lossy `to_f64` that
/// truncated the mantissa to 7 significant bits), this only ever goes
/// double -> extended: DC.X is assembler-only (write bytes from a text
/// literal), so there is no corresponding "format an extended value back
/// as text" path that would need the inverse conversion.
fn f64_to_extended_words(val: f64) -> Vec<u16> {
    let bits = val.to_bits();
    let sign = (bits >> 63) as u16;
    let exp64 = ((bits >> 52) & 0x7FF) as i32;
    let frac64 = bits & 0x000F_FFFF_FFFF_FFFF;

    let (exp15, mantissa64): (u16, u64) = if exp64 == 0 {
        if frac64 == 0 {
            // Zero (signed).
            (0, 0)
        } else {
            // Subnormal double: normalize into extended's normal range,
            // since extended precision has a wider exponent field and can
            // represent every subnormal double as a normal extended value.
            let leading_zeros = frac64.leading_zeros() - 12; // frac64 is 52 significant bits in a u64
            let shift = leading_zeros + 1;
            let mantissa = (frac64 << shift) & 0x000F_FFFF_FFFF_FFFF;
            let exp80 = 16383 - 1022 - leading_zeros as i32;
            (exp80 as u16, 0x8000_0000_0000_0000 | (mantissa << 11))
        }
    } else if exp64 == 0x7FF {
        // Inf or NaN: extended uses all-1s exponent too, explicit integer
        // bit set, mantissa nonzero (with the original NaN payload,
        // shifted up) for NaN or zero for Inf.
        (0x7FFF, 0x8000_0000_0000_0000 | (frac64 << 11))
    } else {
        // Normal double: rebias the exponent (1023 -> 16383) and shift the
        // 52-bit fraction up to a 63-bit fraction with an explicit leading
        // integer bit (bit 63) set, per the extended-precision format.
        let exp80 = exp64 - 1023 + 16383;
        (exp80 as u16, 0x8000_0000_0000_0000 | (frac64 << 11))
    };

    vec![
        (sign << 15) | exp15,
        0,
        ((mantissa64 >> 48) & 0xFFFF) as u16,
        ((mantissa64 >> 32) & 0xFFFF) as u16,
        ((mantissa64 >> 16) & 0xFFFF) as u16,
        (mantissa64 & 0xFFFF) as u16,
    ]
}

/// Convert a decimal-literal string (e.g. `"3.14"`, `"-123"`, `"1.5e10"`)
/// to the 96-bit (6-word) Motorola packed-BCD decimal representation used
/// by DC.P: word0 bit 15 = mantissa sign, bit 12 = exponent sign, bits
/// 11-0 = 3 BCD exponent digits; word1 bits 3-0 = the single BCD digit
/// before the decimal point; words 2-5 = the remaining 16 BCD mantissa
/// digits (4 per word), most-significant first.
fn packed_decimal_words(text: &str, line_no: usize) -> Result<Vec<u16>, AsmError> {
    let text = text.trim();
    let invalid = || AsmError::with_line(format!("invalid DC.P literal: {}", text), line_no);

    let (mantissa_sign, rest) = match text.strip_prefix('-') {
        Some(r) => (1u16, r),
        None => (0u16, text.strip_prefix('+').unwrap_or(text)),
    };

    // Split off an optional exponent suffix (e/E followed by an optional
    // sign and digits), then the optional fractional part.
    let (mantissa_part, exp_part) = match rest.find(['e', 'E']) {
        Some(pos) => (&rest[..pos], &rest[pos + 1..]),
        None => (rest, ""),
    };

    let (exp_sign, exp_digits) = if let Some(e) = exp_part.strip_prefix('-') {
        (1u16, e)
    } else {
        (0u16, exp_part.strip_prefix('+').unwrap_or(exp_part))
    };
    let explicit_exp: i32 = if exp_digits.is_empty() {
        0
    } else {
        exp_digits.parse().map_err(|_| invalid())?
    };

    let (int_part, frac_part) = match mantissa_part.find('.') {
        Some(pos) => (&mantissa_part[..pos], &mantissa_part[pos + 1..]),
        None => (mantissa_part, ""),
    };
    if !int_part.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
        || (int_part.is_empty() && frac_part.is_empty())
    {
        return Err(invalid());
    }

    // Normalize to exactly 17 significant decimal digits (the packed
    // format's fixed mantissa width) with the decimal point after the
    // first digit, adjusting the exponent accordingly — e.g. "314.159"
    // becomes digits "31415900000000000" with explicit_exp bumped by 2
    // (matching scientific notation 3.14159 x 10^2).
    let mut digits: Vec<u8> = int_part
        .bytes()
        .chain(frac_part.bytes())
        .map(|b| b - b'0')
        .collect();
    // Point position (digits before the decimal) in the un-normalized
    // digit string; used to compute how far the point moves once
    // normalized to a single leading digit.
    let point_pos = int_part.len() as i32;

    // Strip leading zeros (they don't count as significant digits and
    // shift where the normalized point lands), but keep at least one
    // digit so an all-zero literal still normalizes to "0".
    let leading_zeros = digits.iter().take_while(|&&d| d == 0).count();
    let effective_point = point_pos - leading_zeros as i32;
    digits.drain(0..leading_zeros.min(digits.len().saturating_sub(1)));

    let is_zero = digits.iter().all(|&d| d == 0);
    let normalized_exp = if is_zero { 0 } else { effective_point - 1 };
    let total_exp = explicit_exp + normalized_exp;
    let (exp_sign, exp_mag) = if total_exp < 0 {
        (1u16, (-total_exp) as u32)
    } else {
        (exp_sign, total_exp as u32)
    };
    if exp_mag > 999 {
        return Err(AsmError::with_line(
            format!("DC.P exponent out of range (-999..=999): {}", text),
            line_no,
        ));
    }

    digits.truncate(17);
    digits.resize(17, 0);

    let bcd_exp = ((exp_mag / 100) % 10) << 8 | ((exp_mag / 10) % 10) << 4 | (exp_mag % 10);
    let word0 = (mantissa_sign << 15) | (exp_sign << 12) | (bcd_exp as u16);

    let word1 = digits[0] as u16;
    let mut words = vec![word0, word1];
    for chunk in digits[1..17].chunks(4) {
        let w = (chunk[0] as u16) << 12
            | (chunk[1] as u16) << 8
            | (chunk[2] as u16) << 4
            | (chunk[3] as u16);
        words.push(w);
    }
    Ok(words)
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

    /// FMOVE.P k-factor reference bytes, verified against real
    /// reference output (see the commit adding k-factor support for
    /// the exact byte-level derivation of the static-vs-dynamic format
    /// code distinction). `(A0)` is address register indirect mode 2 reg 0.
    #[test]
    fn test_source_fmove_p_static_kfactor_matches_reference() {
        // Reference encoding: fmove.p fp0,(a0){#5} -> f2106c05
        let bytes = assemble_fpu_source("    FMOVE.P FP0,(A0){#5}\n");
        assert_eq!(bytes, vec![0xF2, 0x10, 0x6C, 0x05]);
    }

    #[test]
    fn test_source_fmove_p_static_negative_kfactor_matches_reference() {
        // Reference encoding: fmove.p fp0,(a0){#-5} -> f2106c7b (7-bit two's complement)
        let bytes = assemble_fpu_source("    FMOVE.P FP0,(A0){#-5}\n");
        assert_eq!(bytes, vec![0xF2, 0x10, 0x6C, 0x7B]);
    }

    #[test]
    fn test_source_fmove_p_static_kfactor_extremes_match_reference() {
        // Reference encoding: fmove.p fp0,(a0){#63} -> f2106c3f ; {#-64} -> f2106c40
        assert_eq!(
            assemble_fpu_source("    FMOVE.P FP0,(A0){#63}\n"),
            vec![0xF2, 0x10, 0x6C, 0x3F]
        );
        assert_eq!(
            assemble_fpu_source("    FMOVE.P FP0,(A0){#-64}\n"),
            vec![0xF2, 0x10, 0x6C, 0x40]
        );
    }

    #[test]
    fn test_source_fmove_p_kfactor_out_of_range_is_error() {
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        assert!(asm.assemble("    FMOVE.P FP0,(A0){#64}\n").is_err());
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        assert!(asm.assemble("    FMOVE.P FP0,(A0){#-65}\n").is_err());
    }

    #[test]
    fn test_source_fmove_p_dynamic_kfactor_matches_reference() {
        // Reference encoding: fmove.p fp0,(a0){d3} -> f2107c30 (format code 7, Dn in bits 6-4)
        let bytes = assemble_fpu_source("    FMOVE.P FP0,(A0){D3}\n");
        assert_eq!(bytes, vec![0xF2, 0x10, 0x7C, 0x30]);
    }

    #[test]
    fn test_source_fmove_p_dynamic_kfactor_all_registers_match_reference() {
        // Reference encoding: fmove.p fp0,(a0){dN} for N=0..7 -> f2107c00, 7c10, .., 7c70
        for (n, expected_ext) in (0u16..8).map(|n| (n, 0x7C00 + (n << 4))) {
            let bytes = assemble_fpu_source(&format!("    FMOVE.P FP0,(A0){{D{}}}\n", n));
            let ext = ((bytes[2] as u16) << 8) | bytes[3] as u16;
            assert_eq!(ext, expected_ext, "d{}", n);
        }
    }

    #[test]
    fn test_source_fmove_p_no_kfactor_defaults_to_static_zero() {
        // No {...} suffix at all: k-factor field is simply 0 (static,
        // k=0) since it's the same bit pattern as the pre-k-factor code
        // already emitted.
        let bytes = assemble_fpu_source("    FMOVE.P FP0,(A0)\n");
        assert_eq!(bytes, vec![0xF2, 0x10, 0x6C, 0x00]);
    }

    #[test]
    fn test_source_fmove_kfactor_wrong_size_is_error() {
        // k-factor is only meaningful with .P; on any other size (or the
        // read direction <ea>,FPn) it must error, not be silently dropped.
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        assert!(asm.assemble("    FMOVE.L FP0,(A0){#5}\n").is_err());
    }

    /// Full encode -> decode -> reformat -> reassemble roundtrip for
    /// FMOVE.P k-factor forms, the same class of check
    /// `test_golden_vectors_full_roundtrip` (golden_assembler.rs) does for
    /// the golden vector set — these instructions aren't in that fixed
    /// vector file, so this covers the same ground directly. This is what
    /// caught the disassembler previously not recognizing bits 6-0 as a
    /// k-factor field (it fell back to "unknown FPU arithmetic
    /// opclass/opmode" for any k != 0) when this feature was first added.
    #[test]
    fn test_fmove_p_kfactor_full_roundtrip() {
        for src in [
            "    FMOVE.P FP0,(A0){#5}\n",
            "    FMOVE.P FP0,(A0){#-5}\n",
            "    FMOVE.P FP0,(A0){#0}\n",
            "    FMOVE.P FP0,(A0){D3}\n",
        ] {
            let mut asm = Assembler::new(0x1000);
            asm.set_cpu("68020");
            let original = asm.assemble_bytes(src).unwrap();

            let mut stream = m68k_core::addressing::InstructionStream::new(&original, 0x1000);
            let decoded_text = match m68k_disasm::decoder::decode_next(&mut stream, "68020") {
                Ok((_, m68k_disasm::decoder::DecodeResult::Instruction(inst))) => {
                    inst.format(&std::collections::HashMap::new())
                }
                other => panic!("{}: expected instruction, got {:?}", src, other),
            };

            let mut asm2 = Assembler::new(0x1000);
            asm2.set_cpu("68020");
            let reasm_source = format!("    ORG $1000\n    {}\n", decoded_text);
            let reencoded = asm2.assemble_bytes(&reasm_source).unwrap_or_else(|e| {
                panic!(
                    "{}: decoded '{}' but reassembling it failed: {:?}",
                    src,
                    decoded_text.trim(),
                    e
                )
            });
            assert_eq!(
                reencoded,
                original,
                "{}: decoded '{}' but reencoding it diverged",
                src,
                decoded_text.trim()
            );
        }
    }

    #[test]
    fn test_source_fmovem_range_without_slash_bugfix() {
        // fmovem fp0-fp3,-(a7): a pure range without a '/' must parse as a register list.
        // Reference encoding: fmovem fp0-fp3,-(a7) -> f227e00f
        let bytes = assemble_fpu_source("    FMOVEM FP0-FP3,-(A7)\n");
        assert_eq!(bytes, vec![0xF2, 0x27, 0xE0, 0x0F]);
    }

    #[test]
    fn test_source_fmovem_ctrl_list() {
        // fmovem fpcr/fpsr,-(a0): the "/" check must not misidentify this as an FP
        // data-register list before checking control registers.
        // Reference encoding: fmovem fpcr/fpsr,-(a0) -> f220b800
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
    fn test_encode_bra_long_displacement_matches_reference() {
        // On 68020, `bra.l` for a displacement outside the 16-bit range uses
        // opword 0x60FF (low byte 0xFF marks the 32-bit form) immediately followed
        // by the 32-bit displacement, with no padding word - regression test for a
        // bug where this duplicated (and originally also broken) BRA/BSR encoder
        // emitted 0x6000 plus a spurious extra zero word.
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        let disp = 0x20000i32;
        let words = asm.encode_bra(disp, BranchSize::Long).unwrap();
        assert_eq!(words, vec![0x60FF, 0x0002, 0x0000]);
    }

    #[test]
    fn test_encode_bsr_long_displacement_matches_reference() {
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        let disp = 0x20000i32;
        let words = asm.encode_bsr(disp, BranchSize::Long).unwrap();
        assert_eq!(words, vec![0x61FF, 0x0002, 0x0000]);
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
        let st = SymbolTable::new();
        assert!(matches!(
            parse_parens_disp_pc("($10,PC)", &st, 0),
            Some((16, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc("($1000,PC)", &st, 0),
            Some((4096, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc("($10000,PC)", &st, 0),
            Some((65536, true))
        ));
        assert!(matches!(
            parse_parens_disp_pc("(-10,PC)", &st, 0),
            Some((-10, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc("(0,PC)", &st, 0),
            Some((0, false))
        ));
        assert!(parse_parens_disp_pc("(A0)", &st, 0).is_none());
    }

    #[test]
    fn test_parse_parens_disp_pc_index() {
        let st = SymbolTable::new();
        assert!(matches!(
            parse_parens_disp_pc_index("($10,PC,D0)", &st, 0),
            Some((0, 16, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc_index("(0,PC,D1)", &st, 0),
            Some((1, 0, _, false))
        ));
        // An index register that is an *address* register is reported as
        // 8 + n, the numbering the brief-format extension word uses to
        // derive its D/A bit — so A2 is 10, not 2 (which would be D2).
        assert!(matches!(
            parse_parens_disp_pc_index("($20,PC,A2)", &st, 0),
            Some((10, 32, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc_index("($10,PC,D3.W)", &st, 0),
            Some((3, 16, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_pc_index("($10,PC,D3.L)", &st, 0),
            Some((3, 16, _, true))
        ));
        assert!(parse_parens_disp_pc_index("(A0)", &st, 0).is_none());
        assert!(parse_parens_disp_pc_index("($10,PC)", &st, 0).is_none());
    }

    #[test]
    fn test_parse_parens_disp_reg_index() {
        let st = SymbolTable::new();
        assert!(matches!(
            parse_parens_disp_reg_index("($10,A0,D1)", &st, 0),
            Some((0, 1, 16, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("(0,A1,D0)", &st, 0),
            Some((1, 0, 0, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("($20,A2,D3.W)", &st, 0),
            Some((2, 3, 32, _, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("($10,A3,D4.L)", &st, 0),
            Some((3, 4, 16, _, true))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("(0,A0,D0*1)", &st, 0),
            Some((0, 0, 0, 1, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("(0,A0,D0*2)", &st, 0),
            Some((0, 0, 0, 2, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("(0,A0,D0*4)", &st, 0),
            Some((0, 0, 0, 4, false))
        ));
        assert!(matches!(
            parse_parens_disp_reg_index("(0,A0,D0*8)", &st, 0),
            Some((0, 0, 0, 8, false))
        ));
        assert!(parse_parens_disp_reg_index("(A0)", &st, 0).is_none());
        assert!(parse_parens_disp_reg_index("($10,A0)", &st, 0).is_none());
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

    /// Regression: `(An,Xn)` without a displacement was rejected — only
    /// the explicit `(0,An,Xn)` spelling parsed — and `(A0,A1.L)` was
    /// misread as a displacement form, silently dropping the index
    /// register.
    ///
    /// Reference encodings: `MOVE.W (A4,D2.W),(A1)+` -> 32F4 2000,
    /// `MOVE.W (A0,A1.L),D0` -> 3030 9800.
    #[test]
    fn test_indexed_ea_without_displacement() {
        let mut asm = Assembler::new(0);
        let bytes = asm
            .assemble_bytes("    MOVE.W  (A4,D2.W),(A1)+\n")
            .expect("(An,Xn) without displacement must parse");
        assert_eq!(bytes, vec![0x32, 0xF4, 0x20, 0x00]);

        // An address register as the index, with an explicit size.
        let mut asm = Assembler::new(0);
        let bytes = asm
            .assemble_bytes("    MOVE.W  (A0,A1.L),D0\n")
            .expect("(An,An.L) must keep the index register");
        assert_eq!(bytes, vec![0x30, 0x30, 0x98, 0x00]);

        // The displacement form is unaffected: MOVE.W (4,A0),D0 -> 3028 0004.
        let mut asm = Assembler::new(0);
        let bytes = asm.assemble_bytes("    MOVE.W  (4,A0),D0\n").unwrap();
        assert_eq!(bytes, vec![0x30, 0x28, 0x00, 0x04]);
    }

    /// Regression: ADD/SUB/CMP restricted their source EA to `DATA`,
    /// which excludes address registers — but `An` is a valid source at
    /// word and long size. `SUB.L A1,D1` failed with "addressing mode not
    /// allowed". Only the byte forms exclude it.
    ///
    /// Reference encodings: SUB.L A1,D1 -> 9289, CMP.W A3,D3 -> B64B,
    /// ADD.L A2,D2 -> D48A.
    #[test]
    fn test_address_register_as_arithmetic_source() {
        let mut asm = Assembler::new(0);
        let bytes = asm
            .assemble_bytes("    SUB.L  A1,D1\n    CMP.W  A3,D3\n    ADD.L  A2,D2\n")
            .expect("An is a valid word/long source for ADD/SUB/CMP");
        assert_eq!(bytes, vec![0x92, 0x89, 0xB6, 0x4B, 0xD4, 0x8A]);

        // Byte size still rejects it, as the PRM requires.
        let mut asm = Assembler::new(0);
        assert!(asm.assemble_bytes("    ADD.B  A1,D1\n").is_err());
    }

    /// Local labels (`.loop`) are scoped to the preceding global label, so
    /// the same spelling may repeat once per subroutine.
    ///
    /// Reference encoding for this source: 7000 5240 66FC 4E75 7205 5341
    /// 66FC 4E75 — both `BNE.S` reach their own `.loop`.
    #[test]
    fn test_local_labels_are_scoped_per_global_label() {
        let mut asm = Assembler::new(0x1000);
        asm.set_optimize(true);
        let bytes = asm
            .assemble_bytes(concat!(
                "routine_a:\n",
                "\tMOVEQ\t#0,D0\n",
                ".loop:\n",
                "\tADDQ.W\t#1,D0\n",
                "\tBNE.S\t.loop\n",
                "\tRTS\n",
                "routine_b:\n",
                "\tMOVEQ\t#5,D1\n",
                ".loop:\n",
                "\tSUBQ.W\t#1,D1\n",
                "\tBNE.S\t.loop\n",
                "\tRTS\n",
            ))
            .expect("duplicate local labels in separate scopes must assemble");
        assert_eq!(
            bytes,
            vec![
                0x70, 0x00, 0x52, 0x40, 0x66, 0xFC, 0x4E, 0x75, 0x72, 0x05, 0x53, 0x41, 0x66, 0xFC,
                0x4E, 0x75
            ]
        );
        // Stored under qualified names, so neither shadows the other.
        assert_eq!(asm.symbols.resolve("routine_a.loop").unwrap(), 0x1002);
        assert_eq!(asm.symbols.resolve("routine_b.loop").unwrap(), 0x100A);
    }

    /// End-to-end regression for the four August-2026 audit fixes, each
    /// verified against a reference assembler. These go through the full
    /// parser + two-pass pipeline, not just the encoder, because two of
    /// them (FSINCOS's `FPc:FPs`, PFLUSH's operand count) were blocked in
    /// the parser rather than the encoder.
    #[test]
    fn test_audit_fixes_end_to_end() {
        // MOVE from CCR — used to assemble to 303C FFFF (`MOVE.W #-1,D0`).
        let mut asm = Assembler::new(0);
        asm.set_cpu("68010");
        let bytes = asm.assemble_bytes("    MOVE.W  CCR,D0\n").unwrap();
        assert_eq!(bytes, vec![0x42, 0xC0]);

        // FSINCOS with the colon spelling — used to be rejected.
        let mut asm = Assembler::new(0);
        asm.set_cpu("68040");
        let bytes = asm.assemble_bytes("    FSINCOS.X  FP0,FP1:FP2\n").unwrap();
        assert_eq!(bytes, vec![0xF2, 0x00, 0x01, 0x31]);

        // PFLUSH without an EA — used to be rejected.
        let mut asm = Assembler::new(0);
        asm.set_cpu("68030");
        let bytes = asm.assemble_bytes("    PFLUSH  #0,#0\n").unwrap();
        assert_eq!(bytes, vec![0xF0, 0x00, 0x30, 0x10]);
        // The three-operand form still works.
        let mut asm = Assembler::new(0);
        asm.set_cpu("68030");
        let bytes = asm.assemble_bytes("    PFLUSH  #2,#4,(A0)\n").unwrap();
        assert_eq!(bytes, vec![0xF0, 0x10, 0x38, 0x92]);

        // CONTROL_ALT no longer admits (An)+ / -(An) for bitfields.
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        assert!(
            asm.assemble_bytes("    BFCLR  (A0)+{0:8}\n").is_err(),
            "auto-increment is not a valid bitfield destination"
        );
        // The ordinary control form is unaffected.
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        assert!(asm.assemble_bytes("    BFCLR  (A0){0:8}\n").is_ok());
    }

    /// `IFD`/`IFND`/`ENDC` are the Motorola spellings of
    /// `IFDEF`/`IFNDEF`/`ENDIF`, used by every Amiga header's include
    /// guard. Without them the guard never closed and the rest of the
    /// file was silently skipped.
    #[test]
    fn test_motorola_conditional_spellings() {
        let mut asm = Assembler::new(0x1000);
        let bytes = asm
            .assemble_bytes(concat!(
                "\tIFND\tGUARD\n",
                "GUARD\tSET\t1\n",
                "VAL\tEQU\t7\n",
                "\tENDC\n",
                "\tIFD\tGUARD\n",
                "\tMOVE.W\t#VAL,D0\n",
                "\tENDC\n",
            ))
            .expect("IFND/IFD/ENDC must be recognised");
        assert_eq!(bytes, vec![0x30, 0x3C, 0x00, 0x07]);
    }

    /// Regression: an argument the invocation didn't supply must
    /// substitute to the empty string, not stay a literal `\1`.
    ///
    /// This is what the standard optional-argument idiom relies on —
    /// `IFC '\1',''` picks the default branch. Leaving `\1` in place made
    /// the comparison take the wrong branch and emitted `SET \1`, failing
    /// with "invalid SET expression: unexpected character '\'". The Amiga
    /// `exec/libraries.i` header uses exactly this shape for `LIBINIT`.
    #[test]
    fn test_macro_unsupplied_argument_substitutes_empty() {
        let mut asm = Assembler::new(0x1000);
        let src = concat!(
            "LIBINIT     MACRO   * [baseOffset]\n",
            "            IFC     '\\1',''\n",
            "COUNT       SET     100\n",
            "            ENDC\n",
            "            IFNC    '\\1',''\n",
            "COUNT       SET     \\1\n",
            "            ENDC\n",
            "            ENDM\n",
            "    LIBINIT\n",
            "    MOVE.W  #COUNT,D0\n",
        );
        let bytes = asm.assemble_bytes(src).expect("assembly should succeed");
        // Default branch taken: COUNT = 100 = $64.
        assert_eq!(bytes, vec![0x30, 0x3C, 0x00, 0x64]);

        // And with an argument, the other branch wins.
        let mut asm = Assembler::new(0x1000);
        let bytes = asm
            .assemble_bytes(&src.replace("    LIBINIT\n", "    LIBINIT 7\n"))
            .expect("assembly should succeed");
        assert_eq!(bytes, vec![0x30, 0x3C, 0x00, 0x07]);
    }

    /// `* [text]` after a mnemonic is a comment, not an operand.
    ///
    /// The star-comment rule keyed on a following *letter*, so the Amiga
    /// headers' `MACRO   * [baseOffset]` parameter documentation parsed as
    /// a macro parameter list and broke `\1` substitution.
    #[test]
    fn test_star_comment_before_bracket_is_a_comment() {
        let (label, mnemonic, _, operands) =
            m68k_core::tokens::split_line("LIBINIT\t    MACRO   * [baseOffset]");
        assert_eq!(label.as_deref(), Some("LIBINIT"));
        assert_eq!(mnemonic, "macro");
        assert!(
            operands.is_empty(),
            "the `* [...]` comment must not become a macro parameter, got {:?}",
            operands
        );
    }

    /// A file is spliced in only once, however many times it is included.
    ///
    /// The Amiga headers include their dependencies and guard the contents
    /// with `IFND FOO_I`/`ENDC`. Include expansion runs before pass 1
    /// evaluates conditionals, so without include-once the text lands in
    /// the stream twice and the second copy's `EQU`s collide.
    #[test]
    fn test_include_is_spliced_only_once() {
        let dir = std::env::temp_dir().join(format!("m68k_inc_once_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("defs.i"), "VAL\tEQU\t9\n").unwrap();
        std::fs::write(dir.join("mid.i"), "\tinclude 'defs.i'\n").unwrap();

        let mut asm = Assembler::new(0x1000);
        asm.add_include_path(dir.clone());
        // Both paths reach defs.i; a second splice would redefine VAL.
        let bytes = asm
            .assemble_bytes("\tinclude 'defs.i'\n\tinclude 'mid.i'\n\tMOVE.W\t#VAL,D0\n")
            .expect("duplicate include must not redefine symbols");
        assert_eq!(bytes, vec![0x30, 0x3C, 0x00, 0x09]);

        std::fs::remove_dir_all(&dir).ok();
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
        // `\@` expands to `_000001`, `_000002`, ... — with a leading
        // underscore so the fragment is a valid identifier when used as a
        // *prefix* too, which the Amiga system headers rely on. The
        // reference substitutes the same shape.
        assert!(asm.symbols.resolve("local_000001").is_ok());
        assert!(asm.symbols.resolve("local_000002").is_ok());
        assert_eq!(asm.symbols.resolve("local_000001").unwrap(), 0x1000);
        assert_eq!(asm.symbols.resolve("local_000002").unwrap(), 0x1002);
    }

    /// Regression: a macro invoking another macro previously errored with
    /// "unknown mnemonic" — macro_preprocess only ran a single scan pass,
    /// so an invocation newly exposed by expanding the outer macro's body
    /// was substituted in as literal text but never re-scanned. This also
    /// exercises that macro definitions persist across the now-multiple
    /// preprocessing passes: `inner`'s `MACRO...ENDM` block is consumed
    /// (removed from the text) by the pass that parses it, so a later
    /// pass re-scanning `outer`'s expanded body must still find `inner`
    /// in the retained definition table, not in the text.
    #[test]
    fn test_nested_macro_invocation() {
        let mut asm = Assembler::new(0);
        let result = asm.assemble(
            "
inner   MACRO
    NOP
    ENDM
outer   MACRO
    inner
    RTS
    ENDM
    outer
",
        );
        assert!(result.is_ok(), "assembly failed: {:?}", result.err());
        assert_eq!(asm.code.len(), 2);
        assert_eq!(asm.code[0].words, vec![0x4E71]); // NOP from inner
        assert_eq!(asm.code[1].words, vec![0x4E75]); // RTS from outer
    }

    /// A self-recursive macro must not hang the assembler: the expansion
    /// depth limit should kick in and produce an error (an unexpanded
    /// invocation left over once the limit is hit) rather than looping
    /// forever or exhausting memory.
    #[test]
    fn test_self_recursive_macro_does_not_hang() {
        let mut asm = Assembler::new(0);
        let result = asm.assemble(
            "
recur   MACRO
    NOP
    recur
    ENDM
    recur
",
        );
        assert!(result.is_err());
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
    fn test_rept_malformed_hex_count_is_not_silently_zero() {
        // `$ZZ` is not a valid hex count. This used to `unwrap_or(0)`, which
        // expanded the block zero times and dropped the NOP without any
        // diagnostic — the reference assembler rejects the same input. The
        // line must not assemble to just `MOVEQ`/`RTS`.
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    ORG $1000
    MOVEQ #0,D0
    REPT $ZZ
    NOP
    ENDR
    RTS
",
        );
        let _ = result;
        assert!(
            asm.errors.has_errors(),
            "malformed REPT count was accepted silently; diagnostics: {:?}",
            asm.errors.errors
        );
        assert!(
            asm.errors
                .errors
                .iter()
                .any(|d| d.message.contains("invalid REPT count")),
            "expected an explicit REPT diagnostic, got {:?}",
            asm.errors.errors
        );
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
        // Three DC.B from the IRP, then RTS. The odd byte count means an
        // alignment pad sits between them, so compare the bytes rather
        // than instruction indices.
        assert_eq!(asm.code[0].words, vec![0x0100]);
        assert_eq!(asm.code[1].words, vec![0x0200]);
        assert_eq!(asm.code[2].words, vec![0x0300]);
        let (bytes, _) = crate::output::generate_binary(&asm.code).unwrap();
        assert_eq!(bytes, vec![0x01, 0x02, 0x03, 0x00, 0x4E, 0x75]);
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
        assert_eq!(asm.code[0].words, vec![0x4100]);
        assert_eq!(asm.code[1].words, vec![0x4200]);
        assert_eq!(asm.code[2].words, vec![0x4300]);
        // "ABC" is odd-length, so a pad byte precedes the RTS.
        let (bytes, _) = crate::output::generate_binary(&asm.code).unwrap();
        assert_eq!(bytes, vec![0x41, 0x42, 0x43, 0x00, 0x4E, 0x75]);
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
        // Shrinking to the short form is an optimization, off by default.
        asm.set_optimize(true);
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

    /// Regression for the branch-relaxation "no-op" bug: `BranchInfo`
    /// previously never had its target populated, so pass 1 always assumed
    /// a word-sized branch while pass 2 emitted short whenever it fit,
    /// desyncing every label after the branch from the physical layout.
    #[test]
    fn test_branch_relaxation_label_matches_physical_layout() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
start:
    BRA next
next:
    RTS
",
        );
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        // `next` must resolve to where RTS actually landed, not to a
        // pass-1 estimate that pass 2 never matched.
        let next_addr = asm.symbols.resolve("next").unwrap();
        let rts_instr = asm.code.iter().find(|i| i.words == vec![0x4E75]).unwrap();
        assert_eq!(next_addr, rts_instr.pc);
    }

    /// disp==0 (branch target is the address immediately following the
    /// branch's own opword) must fall through to the word-displacement
    /// form: the low byte 0x00 is reserved to signal that form, so a
    /// 1-word encoding with that byte would be ambiguous with BRA.w
    /// reusing its own opcode word as the displacement. Using a numeric
    /// target (rather than a label placed right after the branch, whose
    /// own address would shift as the branch grows) pins disp==0 exactly.
    #[test]
    fn test_bra_disp_zero_uses_word_form() {
        let mut asm = Assembler::new(0x1000);
        // BRA at $1000 is 2 bytes -> PC after opword = $1002 -> disp==0
        // means target == $1002.
        let result = asm.assemble("    BRA $1002\n");
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        assert_eq!(asm.code[0].words, vec![0x6000, 0x0000]);
    }

    /// DS.B with an odd count must not silently round up in pass 1 while
    /// pass 2 doesn't: previously this offset every following label by 1.
    #[test]
    fn test_ds_b_odd_count_label_matches_pass2() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    DS.B 3
after:
    NOP
",
        );
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        assert_eq!(asm.symbols.resolve("after").unwrap(), 0x1003);
        assert_eq!(asm.code[0].pc, 0x1003);
    }

    /// DC.B with a string literal must count its actual byte length in
    /// pass 1, not the single-argument count: previously "HELLO" was
    /// estimated as 1 byte instead of the 6 bytes (5 chars + pad) pass 2
    /// emits, offsetting every following label by 4 bytes.
    #[test]
    fn test_dc_b_string_label_matches_pass2() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    DC.B \"HELLO\"
after:
    NOP
",
        );
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        // "HELLO" is 5 bytes, so `after` lands on the odd address $1005.
        // The label is *not* rounded up to a word boundary — a DC.B of
        // odd length leaves the location counter odd, and only the
        // following instruction gets a pad byte. Matches the reference
        // assembler, which also reports `after = $1005`.
        assert_eq!(asm.symbols.resolve("after").unwrap(), 0x1005);
        // code[1] is the one-byte alignment pad inserted before the NOP,
        // which therefore starts at $1006.
        assert_eq!(asm.code[1].pc, 0x1005);
        assert_eq!(asm.code[2].pc, 0x1006);
    }

    /// DS.L/DCB.L with a huge count must return a clean error instead of
    /// panicking on `element_size * count` integer overflow (previously
    /// unchecked in both pass 1's size estimate and pass 2's encoder).
    #[test]
    fn test_ds_and_dcb_huge_count_errors_instead_of_panicking() {
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DS.L $80000000\n");
        assert!(result.is_err());

        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DCB.L $80000000,0\n");
        assert!(result.is_err());
    }

    /// Regression: DC.X/DC.P's hex/integer literal parsing silently fell
    /// back to `unwrap_or(0)` on unparseable input (e.g. a decimal float
    /// literal, since neither format is actually implemented), emitting
    /// six zero words with no diagnostic instead of a clear error.
    #[test]
    fn test_dc_x_converts_float_literal_instead_of_emitting_zeros() {
        // Regression for 2.7/4.5: this used to silently emit six zero
        // words via `unwrap_or(0)`. It was then changed to reject float
        // literals outright as an interim fix; now it does the real
        // conversion, so a plausible non-zero, non-error encoding is the
        // right behavior again.
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DC.X 3.14\n");
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        assert_ne!(asm.code[0].words, vec![0, 0, 0, 0, 0, 0]);

        // Hex and plain integer literals must keep working.
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DC.X $1234\n");
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
    }

    #[test]
    fn test_dc_x_extended_precision_reference_value() {
        // 1.0 in IEEE-754 80-bit extended: sign=0, exponent=16383=$3FFF
        // (bias), explicit integer bit set, zero fraction. Motorola's
        // 96-bit DC.X slot pads with a reserved zero word after the
        // sign/exponent word.
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DC.X 1.0\n");
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        assert_eq!(
            asm.code[0].words,
            vec![0x3FFF, 0x0000, 0x8000, 0x0000, 0x0000, 0x0000]
        );
    }

    #[test]
    fn test_dc_x_negative_value_sets_sign_bit() {
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DC.X -1.0\n");
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        // Sign bit (word0 bit 15) set, same exponent/mantissa as +1.0.
        assert_eq!(asm.code[0].words[0], 0x8000 | 0x3FFF);
        assert_eq!(
            &asm.code[0].words[1..],
            &[0x0000, 0x8000, 0x0000, 0x0000, 0x0000]
        );
    }

    #[test]
    fn test_dc_x_zero() {
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DC.X 0.0\n");
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        assert_eq!(asm.code[0].words, vec![0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_dc_p_simple_integer() {
        // DC.P 123: mantissa sign 0, exponent 0 (2 is the normalized
        // exponent for 3 significant digits "123" -> 1.23 x 10^2), 17
        // BCD digits "12300000000000000".
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DC.P 123\n");
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        let words = &asm.code[0].words;
        assert_eq!(words[0], 0x0002); // sign=0, exp_sign=0, exponent=2 (BCD)
        assert_eq!(words[1], 0x0001); // leading digit '1'
        assert_eq!(words[2], 0x2300); // digits '2','3','0','0'
        assert_eq!(words[3], 0x0000);
        assert_eq!(words[4], 0x0000);
        assert_eq!(words[5], 0x0000);
    }

    #[test]
    fn test_dc_p_negative_with_fraction() {
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DC.P -3.14\n");
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        let words = &asm.code[0].words;
        assert_eq!(words[0] & 0x8000, 0x8000, "mantissa sign bit must be set");
        assert_eq!(
            words[0] & 0x0FFF,
            0x0000,
            "exponent should be 0 (3.14 = 3.14 x 10^0)"
        );
        assert_eq!(words[1], 0x0003); // leading digit '3'
        assert_eq!(words[2], 0x1400); // digits '1','4','0','0'
    }

    #[test]
    fn test_dc_p_zero() {
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DC.P 0\n");
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        assert_eq!(asm.code[0].words, vec![0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_dc_p_hex_literal_still_raw() {
        // A $-prefixed literal is still the raw 96-bit contents verbatim,
        // for both DC.X and DC.P, not run through conversion.
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DC.P $000102030405060708090A0B\n");
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        assert_eq!(
            asm.code[0].words,
            vec![0x0001, 0x0203, 0x0405, 0x0607, 0x0809, 0x0A0B]
        );
    }

    #[test]
    fn test_dc_p_invalid_literal_is_error() {
        let mut asm = Assembler::new(0);
        let result = asm.assemble("    DC.P not_a_number\n");
        assert!(result.is_err());
    }

    /// Fuzzing regression (cargo-fuzz `assembler_pipeline` target, found
    /// while extending DC.X/DC.P coverage for 4.5): `UNPK`/`PACK`/`CAS`
    /// indexed straight into `operand_texts[0..2]` without checking the
    /// operand count first, unlike every other multi-operand special form
    /// in this match (CAS2/PFLUSH/PTESTR/PTESTW all validate `len()`
    /// first). `{UNPK` alone (with no operands at all) panicked with an
    /// out-of-bounds index instead of reporting a normal assembler error.
    #[test]
    fn test_unpk_pack_cas_without_operands_is_error_not_panic() {
        let mut asm = Assembler::new(0);
        assert!(asm.assemble("    UNPK\n").is_err());
        let mut asm = Assembler::new(0);
        assert!(asm.assemble("    PACK\n").is_err());
        let mut asm = Assembler::new(0);
        assert!(asm.assemble("    CAS D0\n").is_err());
    }

    /// Fuzzing regression (cargo-fuzz `assembler_pipeline` target):
    /// `parse_parens_register_plus`/`parse_minus_parens_register` sliced
    /// operand text with hardcoded byte offsets (`&trimmed[1..]`,
    /// `&trimmed[2..]`) after checking only *one* end of the string
    /// (`ends_with(")+")` / `starts_with("-(")`) — a multi-byte UTF-8
    /// replacement character (`\u{fffd}`, produced by the fuzz harness's
    /// `String::from_utf8_lossy` on invalid byte sequences) at the
    /// unchecked end made the fixed byte offset land mid-codepoint and
    /// panic, instead of `parse_operand_text` cleanly rejecting the operand.
    #[test]
    fn test_parens_register_multibyte_utf8_does_not_panic() {
        assert_eq!(parse_parens_register_plus("\u{fffd}\u{fffd})+"), None);
        assert_eq!(parse_minus_parens_register("\u{fffd}\u{fffd})"), None);
        assert_eq!(parse_parens_register("(\u{fffd}\u{fffd}"), None);
    }

    /// A forward-referenced symbol used as a memory operand must be
    /// estimated conservatively (Absolute.L) in pass 1: if it later
    /// resolves above 0xFFFF, pass 2 upgrades from the 1-word Absolute.W
    /// form pass 1 assumed for the placeholder value 0, growing the
    /// instruction by 2 bytes and desyncing every following label.
    #[test]
    fn test_forward_ref_absolute_long_label_matches_pass2() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
    MOVE.W D0,faraway
after:
    NOP
    ORG $20000
faraway:
    RTS
",
        );
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        assert_eq!(asm.symbols.resolve("after").unwrap(), 0x1006);
        assert_eq!(asm.code[0].words, vec![0x33C0, 0x0002, 0x0000]);
    }

    /// Bcc short-form boundary: -128 must be the smallest short
    /// displacement accepted (previously the encoder rejected -128 even
    /// though it fits in a signed byte, only determine_branch_size allowed
    /// it — a mismatch that becomes observable once relaxation actually
    /// runs). +127/-129/+128 pin the rest of the boundary.
    #[test]
    fn test_bra_short_range_boundaries() {
        // These boundaries only apply when the assembler is allowed to
        // pick the short form; without optimization every unsuffixed
        // branch is word-sized.
        let mut asm = Assembler::new(0x1000);
        asm.set_optimize(true);
        // disp = target - (pc + 2). BRA opcode is 2 bytes.
        asm.assemble("    BRA $1081\n").unwrap();
        assert_eq!(asm.code[0].words.len(), 1, "disp=127 should be short");

        let mut asm = Assembler::new(0x1000);
        asm.set_optimize(true);
        asm.assemble("    BRA $1082\n").unwrap();
        assert_eq!(asm.code[0].words.len(), 2, "disp=128 should be word");

        let mut asm = Assembler::new(0x1000);
        asm.set_optimize(true);
        asm.assemble("    BRA $0F82\n").unwrap();
        assert_eq!(asm.code[0].words.len(), 1, "disp=-128 should be short");

        let mut asm = Assembler::new(0x1000);
        asm.set_optimize(true);
        asm.assemble("    BRA $0F81\n").unwrap();
        assert_eq!(asm.code[0].words.len(), 2, "disp=-129 should be word");
    }

    /// Regression for 4.8: determine_branch_size previously had no long
    /// (68020+, 32-bit displacement) tier — any Bcc/BRA/BSR target beyond
    /// word range always got the Word size hint, so pass 1's size
    /// estimate (used for label PCs) stayed at 4 bytes even though
    /// encode_bra/enc_bcc would emit the 6-byte long form once the
    /// physical displacement was computed, desyncing every label after
    /// the branch (the exact class of bug 1.1 fixed for the word tier).
    #[test]
    fn test_bcc_long_branch_label_matches_pass2_on_68020() {
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        let result = asm.assemble(
            "
    ORG $0
    BEQ far
    ORG $10000
far:
    RTS
",
        );
        assert!(result.is_ok(), "assemble failed: {:?}", result.err());
        assert_eq!(asm.symbols.resolve("far").unwrap(), 0x10000);
        // BEQ.l opword low byte 0xFF marks the 32-bit form.
        assert_eq!(asm.code[0].words[0] & 0xFF, 0xFF);
        assert_eq!(asm.code[0].words.len(), 3);
    }

    /// On 68000 (no long-branch form), the same out-of-range target must
    /// be a clean assembly error, not a silently-emitted 68020 encoding.
    #[test]
    fn test_bcc_long_branch_rejected_on_68000() {
        let mut asm = Assembler::new(0);
        let result = asm.assemble(
            "
    ORG $0
    BEQ far
    ORG $10000
far:
    RTS
",
        );
        assert!(result.is_err());
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
        // EVEN emits a single pad byte at $1001 (code[1]); the DC.W then
        // starts at $1002. Emitting a whole 0x4E71 NOP word here, as this
        // previously did, both over-advanced by a byte and put executable
        // bytes inside a data block.
        assert_eq!(asm.code[1].pc, 0x1001);
        assert_eq!(asm.code[1].size_bytes(), 1);
        assert_eq!(asm.code[2].pc, 0x1002);
        let (bytes, _) = crate::output::generate_binary(&asm.code).unwrap();
        assert_eq!(bytes, vec![0x12, 0x00, 0x34, 0x56]);
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

    /// Regression: pass 2's CNOP handler previously had no alignment
    /// validation (unlike pass 1), so `target % alignment` with
    /// alignment==0 would divide by zero.
    #[test]
    fn test_cnop_zero_alignment_errors_instead_of_panicking() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble("    CNOP 0,0\n");
        assert!(result.is_err());
    }

    /// Both passes derive CNOP padding from `cnop_padding` now. A `SET`
    /// symbol as the alignment is the case where two separately written
    /// copies could have drifted, since its value is re-evaluated per pass.
    #[test]
    fn test_cnop_alignment_from_set_symbol_agrees_across_passes() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble(
            "
ALIGNVAL SET 4
    ORG $1000
    DC.B 1
    CNOP 0,ALIGNVAL
    DC.W $1234
",
        );
        assert!(result.is_ok(), "CNOP failed: {:?}", result.err());
        // A pass-1/pass-2 disagreement shows up as a shifted address here:
        // 0x1000 + 1 byte, padded up to the next 4-byte boundary.
        let last = asm.code.last().expect("DC.W was emitted");
        assert_eq!(last.pc, 0x1004);
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
        // N2/4.9: OPT flags are now tracked, not just accepted-and-discarded.
        assert_eq!(asm.opt.flags.get(&'A'), Some(&true));
        assert_eq!(asm.opt.flags.get(&'F'), Some(&true));
    }

    #[test]
    fn test_opt_numbered_flag_tracked() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble("    OPT W5-\n");
        assert!(result.is_ok(), "OPT failed: {:?}", result.err());
        assert!(asm.opt.warning_disabled(5));
        assert!(!asm.opt.warning_disabled(6));
    }

    #[test]
    fn test_opt_flag_can_be_toggled_back() {
        let mut asm = Assembler::new(0x1000);
        asm.assemble("    OPT O+\nOPT O-\n").unwrap();
        assert_eq!(asm.opt.flags.get(&'O'), Some(&false));
    }

    /// Regression: `parse_opt_flag` split off the trailing *byte* rather
    /// than the trailing character, so a multi-byte character at the end
    /// of an `OPT` entry made `split_at` land mid-codepoint and panic.
    ///
    /// Found by the `assembler_pipeline` fuzz target. `OPT` takes
    /// arbitrary source text, so any non-ASCII byte sequence reaches it —
    /// this must be a clean rejection, never a crash.
    #[test]
    fn test_opt_non_ascii_entry_does_not_panic() {
        for entry in ["Oä", "W5ä", "ä+", "é", "O\u{FFFD}", "\u{1F600}-"] {
            let mut asm = Assembler::new(0x1000);
            let result = asm.assemble(&format!("    OPT {}\n", entry));
            assert!(
                result.is_ok(),
                "OPT {:?} should be rejected cleanly, got {:?}",
                entry,
                result.err()
            );
        }

        // The exact fuzzer-found input, byte for byte.
        let source = String::from_utf8_lossy(&[
            111, 112, 116, 13, 112, 116, 13, 38, 70, 55, 255, 255, 255, 255, 255, 83, 1, 0, 0, 0,
            255, 255,
        ])
        .into_owned();
        let mut asm = Assembler::new(0x1000);
        asm.set_cpu("68010");
        let _ = asm.assemble_bytes(&source);
    }

    /// Regression: the star-comment scan walked the line by byte index
    /// while iterating characters, so a multi-byte character adjacent to
    /// a `*` sliced mid-codepoint. Sources legitimately carry non-ASCII
    /// text in comments and string literals.
    #[test]
    fn test_star_comment_scan_handles_non_ascii() {
        for line in [
            "    MOVE.W  #1,D0   * Grüße aus Köln",
            "    DC.B    'äöü'   * comment",
            "SIZE EQU WIDTH*HÖHE",
            "    DC.B    \"日本語\"",
            "* Überschrift",
        ] {
            // Must not panic; the exact split is asserted elsewhere.
            let _ = m68k_core::tokens::split_line(line);
        }

        // A star comment after non-ASCII text is still recognised.
        let (_, mnemonic, _, operands) =
            m68k_core::tokens::split_line("    MOVE.W  #1,D0   * Grüße");
        assert_eq!(mnemonic, "move");
        assert_eq!(operands, vec!["#1", "D0"]);
    }

    #[test]
    fn test_opt_malformed_entry_warns_instead_of_silently_ignoring() {
        let mut asm = Assembler::new(0x1000);
        let result = asm.assemble("    OPT bogus\n");
        assert!(result.is_ok(), "OPT failed: {:?}", result.err());
        assert!(
            asm.errors
                .warnings
                .iter()
                .any(|w| w.message.contains("malformed option")),
            "expected a malformed-option warning, got: {:?}",
            asm.errors.warnings
        );
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
        asm.cpu = "68010".to_string();
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

    /// Regression: ADDA/SUBA/CMPA previously shifted the size into bits
    /// 13-12 (like MOVE) instead of setting opmode bit 8 (0x0100) for the
    /// long form. `.l` forms were silently wrong — ADDA.L became an F-line
    /// trap opcode and SUBA.L collided with CMPA.W's opcode entirely. `.w`
    /// forms were coincidentally correct since bit 12 is already set in
    /// the base opcode. Covers all 6 forms (ADDA/SUBA/CMPA x imm/ea) at
    /// both sizes, plus An as a source operand (previously rejected: the
    /// EA category was DATA, which excludes address registers, even
    /// though ADDA/SUBA/CMPA accept any addressing mode as source).
    #[test]
    fn test_adda_suba_cmpa_size_bit_and_areg_source() {
        // ADDA.L (A2),A1 -> D3D2 (not F2D2, which is an F-line trap opcode).
        assert_eq!(
            assemble_source_with_cpu("    ADDA.L (A2),A1\n", "68000"),
            vec![0xD3, 0xD2]
        );
        // ADDA.W (A2),A1 -> D2D2, unchanged.
        assert_eq!(
            assemble_source_with_cpu("    ADDA.W (A2),A1\n", "68000"),
            vec![0xD2, 0xD2]
        );
        // SUBA.L #-1,A1 -> 93FC FFFFFFFF (not B2FC, which is CMPA.W's opcode).
        assert_eq!(
            assemble_source_with_cpu("    SUBA.L #-1,A1\n", "68000"),
            vec![0x93, 0xFC, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        // SUBA.W (A2),A1 -> 92D2, unchanged.
        assert_eq!(
            assemble_source_with_cpu("    SUBA.W (A2),A1\n", "68000"),
            vec![0x92, 0xD2]
        );
        // CMPA.L (A2),A1 -> B3D2 (not B2D2, CMPA.W's opcode).
        assert_eq!(
            assemble_source_with_cpu("    CMPA.L (A2),A1\n", "68000"),
            vec![0xB3, 0xD2]
        );
        // CMPA.W (A2),A1 -> B2D2, unchanged.
        assert_eq!(
            assemble_source_with_cpu("    CMPA.W (A2),A1\n", "68000"),
            vec![0xB2, 0xD2]
        );
        // An as source must be accepted (previously rejected as "addressing
        // mode not allowed" under the too-narrow DATA category).
        assert_eq!(
            assemble_source_with_cpu("    CMPA.L A0,A1\n", "68000"),
            vec![0xB3, 0xC8]
        );
        assert_eq!(
            assemble_source_with_cpu("    ADDA.L A0,A1\n", "68000"),
            vec![0xD3, 0xC8]
        );
        assert_eq!(
            assemble_source_with_cpu("    SUBA.L A0,A1\n", "68000"),
            vec![0x93, 0xC8]
        );
    }

    /// Regression for B10: FBcc/FDBcc previously errored with "requires
    /// an address operand" for any label, since encoder.rs's dispatch
    /// only matched Operand::Address — but parse_operand_text returns
    /// AbsoluteShort/AbsoluteLong/Memory for a label that's already
    /// resolvable at parse time (not the Address(0) forward-reference
    /// placeholder), which FBcc/FDBcc's original dispatch rejected.
    #[test]
    fn test_fbcc_fdbcc_accept_forward_and_backward_labels() {
        // Backward reference (label already defined).
        let bytes = assemble_source_with_cpu("target:\n    FBEQ target\n", "68020");
        assert_eq!(bytes, vec![0xF2, 0x81, 0xFF, 0xFE]);

        // Forward reference (label defined later in the source).
        let bytes =
            assemble_source_with_cpu("    FBEQ forward\n    NOP\nforward:\n    RTS\n", "68020");
        assert_eq!(bytes, vec![0xF2, 0x81, 0x00, 0x04, 0x4E, 0x71, 0x4E, 0x75]);

        // FDBcc forward reference.
        let bytes = assemble_source_with_cpu(
            "    FDBEQ D0,forward\n    NOP\nforward:\n    RTS\n",
            "68020",
        );
        assert_eq!(
            bytes,
            vec![0xF2, 0x48, 0x00, 0x01, 0x00, 0x04, 0x4E, 0x71, 0x4E, 0x75]
        );
    }

    #[test]
    fn test_source_pmove_tc() {
        // TC is the destination (memory-to-register, R/W=0). Verified
        // against reference output: F010 4000.
        let bytes = assemble_source_with_cpu("    PMOVE (A0),TC\n", "68030");
        assert_eq!(bytes, vec![0xF0, 0x10, 0x40, 0x00]);
    }

    #[test]
    fn test_source_ptestr() {
        // verified against reference output: F010 8E12.
        let bytes = assemble_source_with_cpu("    PTESTR #2,(A0),#3\n", "68030");
        assert_eq!(bytes, vec![0xF0, 0x10, 0x8E, 0x12]);
    }

    #[test]
    fn test_source_pflusha() {
        // verified against reference output: F000 2400.
        let bytes = assemble_source_with_cpu("    PFLUSHA\n", "68030");
        assert_eq!(bytes, vec![0xF0, 0x00, 0x24, 0x00]);
    }

    #[test]
    fn test_source_pflushn() {
        // verified against reference output: F500.
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
        // reference output for `cinva bc`: F4D8.
        let bytes = assemble_source_with_cpu("    CINVA #3\n", "68040");
        assert_eq!(bytes, vec![0xF4, 0xD8]);
    }

    #[test]
    fn test_source_psave() {
        // Base 0xF100 per PRM bit diagram (not verified against reference encodings - see
        // enc_psave's doc comment in enc_mmu.rs).
        let bytes = assemble_source_with_cpu("    PSAVE -(A0)\n", "68030");
        assert_eq!(bytes, vec![0xF1, 0x20]);
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
    fn test_section_with_type_keyword() {
        // `SECTION name,TYPE` is how essentially every real Amiga source
        // declares a section. It used to be rejected outright: the second
        // argument was only ever parsed as an origin expression, so this
        // failed with "undefined symbol: CODE".
        for src in [
            "    SECTION code,CODE\n    NOP\n",
            "    SECTION data,DATA\n    DC.W 1\n",
            "    SECTION bss,BSS\n    DS.B 4\n",
            "    SECTION mycode,CODE_C\n    NOP\n",
            "    SECTION mydata,DATA_F\n    DC.W 1\n",
        ] {
            let mut asm = Assembler::new(0);
            assert!(
                asm.assemble(src).is_ok(),
                "SECTION with a type keyword must assemble: {:?}",
                src
            );
        }
    }

    #[test]
    fn test_section_type_keyword_beats_the_name() {
        // `SECTION mydata,DATA` is a data section even though its name is
        // not one of the well-known ones. Without this the hunk writer
        // emitted it as HUNK_CODE — declared data becoming executable code.
        use crate::directives::SectionKind;
        let mut asm = Assembler::new(0);
        asm.assemble("    SECTION mydata,DATA\n    DC.W 1\n")
            .unwrap();
        let section = asm
            .sections
            .get_section(&SectionKind::Named("mydata".to_string()))
            .expect("named section must exist");
        assert_eq!(section.effective_kind(), &SectionKind::Data);
    }

    #[test]
    fn test_section_origin_form_still_works() {
        // The numeric second argument is a local extension the reference
        // assembler does not accept; keep it working regardless.
        let mut asm = Assembler::new(0);
        asm.assemble("    SECTION foo,$1000\n    NOP\n").unwrap();
        assert_eq!(asm.code[0].pc, 0x1000);
    }

    #[test]
    fn test_sections_keep_declaration_order() {
        // Output writers used to sort by address and then by name. With
        // every section based at 0 that became alphabetical order, which
        // can put a data or BSS hunk at index 0 — where LoadSeg() enters.
        let mut asm = Assembler::new(0);
        asm.assemble("    SECTION zdata,DATA\n    DC.W 1\n    SECTION acode,CODE\n    RTS\n")
            .unwrap();
        // The implicit default `text` section exists but stays empty, and
        // output writers filter it out — so compare what actually ships.
        let names: Vec<&str> = asm
            .sections
            .iter_sections()
            .filter(|(_, s)| !s.is_empty())
            .map(|(k, _)| k.name())
            .collect();
        assert_eq!(
            names,
            vec!["zdata", "acode"],
            "sections must iterate in declaration order, not sorted by name"
        );
    }

    #[test]
    fn test_bss_section_with_only_ds_is_not_empty() {
        // A BSS section holding nothing but reservations has no
        // instructions, so it was filtered out of every output format and
        // vanished — code referencing a label in it pointed at nothing.
        use crate::directives::SectionKind;
        let mut asm = Assembler::new(0);
        asm.assemble("    SECTION code,CODE\n    RTS\n    SECTION vars,BSS\nbuf:    DS.B 1024\n")
            .unwrap();
        let bss = asm
            .sections
            .get_section(&SectionKind::Named("vars".to_string()))
            .expect("bss section must exist");
        assert!(bss.instructions.is_empty(), "DS emits no instructions");
        assert!(!bss.is_empty(), "but the section is not empty");
        assert_eq!(bss.reserved_size(), 1024);
    }

    #[test]
    fn test_equr_and_reg_aliases() {
        // EQUR/REG are pure textual substitutions: the aliased source must
        // encode exactly like the spelled-out one. Reference-verified.
        let mut aliased = Assembler::new(0);
        aliased
            .assemble(
                "CNT\tEQUR\tD3\nPTR\tEQUR\tA2\nSAVE\tREG\tD0-D3/A0-A2\n\
                 \tMOVEQ\t#5,CNT\n\tMOVE.L\t(PTR),CNT\n\
                 \tMOVEM.L\tSAVE,-(SP)\n\tMOVEM.L\t(SP)+,SAVE\n",
            )
            .unwrap();
        let mut plain = Assembler::new(0);
        plain
            .assemble(
                "\tMOVEQ\t#5,D3\n\tMOVE.L\t(A2),D3\n\
                 \tMOVEM.L\tD0-D3/A0-A2,-(SP)\n\tMOVEM.L\t(SP)+,D0-D3/A0-A2\n",
            )
            .unwrap();
        let a: Vec<u16> = aliased.code.iter().flat_map(|i| i.words.clone()).collect();
        let p: Vec<u16> = plain.code.iter().flat_map(|i| i.words.clone()).collect();
        assert_eq!(a, p, "aliased source must encode identically");
    }

    #[test]
    fn test_equr_does_not_become_a_symbol() {
        // An alias names a register, not an address. Defining it as an
        // ordinary label too would list it in --sym output pointing at the
        // current PC.
        let mut asm = Assembler::new(0x1000);
        asm.assemble("CNT\tEQUR\tD3\n\tMOVEQ\t#0,CNT\n").unwrap();
        assert!(
            asm.symbols.get("CNT").is_none(),
            "EQUR alias must not enter the symbol table"
        );
    }

    #[test]
    fn test_equr_substitutes_whole_identifiers_only() {
        // An alias named `A` must not rewrite the `A` inside `A0` or a
        // longer identifier.
        let mut asm = Assembler::new(0);
        asm.assemble("A\tEQUR\tD5\nADDR\tEQU\t$20\n\tMOVEQ\t#ADDR,A\n")
            .unwrap();
        // MOVEQ #$20,D5 = 0x7A20
        assert_eq!(asm.code[0].words, vec![0x7A20]);
    }

    #[test]
    fn test_label_taking_directives_keep_their_label() {
        // `tokens::split_line` decides whether `NAME DIR args` is a label
        // plus a directive. Only directives that genuinely take a label
        // belong in that list: a name there is also claimed in the operand
        // position, so `BEQ far` would read as the label `BEQ` followed by
        // the directive `far`.
        for (src, want_label, want_mnemonic) in [
            ("CNT\tEQUR\tD3", Some("CNT"), "equr"),
            ("SAVE\tREG\tD0-D3", Some("SAVE"), "reg"),
            ("VAL\tEQU\t5", Some("VAL"), "equ"),
        ] {
            let (label, mnemonic, _, _) = m68k_core::tokens::split_line(src);
            assert_eq!(label.as_deref(), want_label, "label lost in {:?}", src);
            assert_eq!(mnemonic, want_mnemonic, "wrong mnemonic for {:?}", src);
            assert!(is_directive_name(want_mnemonic));
        }
    }

    #[test]
    fn test_directive_names_stay_usable_as_labels() {
        // The reference assembler accepts all of these as ordinary labels;
        // claiming them unconditionally in split_line broke `BEQ far`.
        for name in ["far", "near", "auto", "cpu", "reg", "inline"] {
            let mut asm = Assembler::new(0);
            let src = format!("\tBEQ\t{}\n{}:\n\tRTS\n", name, name);
            assert!(
                asm.assemble(&src).is_ok(),
                "{:?} must still work as a label",
                name
            );
            assert!(asm.symbols.get(name).is_some(), "{} not defined", name);
        }
    }

    #[test]
    fn test_machine_directive_sets_the_cpu() {
        // A source declaring `machine 68020` must assemble without -c.
        let mut asm = Assembler::new(0);
        asm.assemble("\tMACHINE\t68020\n\tBFCLR\tD0{0:8}\n")
            .expect("MACHINE must raise the CPU level");
        assert_eq!(asm.cpu, "68020");

        // And without it, the same instruction is still gated.
        let mut plain = Assembler::new(0);
        assert!(plain.assemble("\tBFCLR\tD0{0:8}\n").is_err());
    }

    #[test]
    fn test_optimisation_hint_directives_are_accepted() {
        // NEAR/FAR/AUTO/INLINE control optimisations this assembler does
        // not perform, but rejecting them stops an entire source dead.
        for d in ["NEAR", "FAR", "AUTO", "INLINE\n\tNOP\n\tEINLINE"] {
            let mut asm = Assembler::new(0);
            let src = format!("\t{}\n\tNOP\n", d);
            assert!(asm.assemble(&src).is_ok(), "{} must be accepted", d);
        }
    }

    #[test]
    fn test_absolute_short_suffix_is_case_insensitive() {
        // `.w` forces the absolute-short encoding. The check tested only
        // for the upper-case spelling, so `lea $400.w,a6` silently emitted
        // the 6-byte long form while `.W` emitted the 4-byte short one.
        for src in ["\tLEA\t$400.w,A6\n", "\tLEA\t$400.W,A6\n"] {
            let mut asm = Assembler::new(0);
            asm.assemble(src).unwrap();
            assert_eq!(asm.code[0].words, vec![0x4DF8, 0x0400], "for {:?}", src);
        }
        for src in ["\tLEA\t$400.l,A6\n", "\tLEA\t$400.L,A6\n"] {
            let mut asm = Assembler::new(0);
            asm.assemble(src).unwrap();
            assert_eq!(asm.code[0].words, vec![0x4DF9, 0x0000, 0x0400]);
        }
    }

    #[test]
    fn test_include_guard_survives_both_passes() {
        // `IFND SYM` / `SYM SET 1` is the guard every Amiga system header
        // uses. Pass 1 took the branch correctly, but pass 2 saw the symbol
        // already defined and skipped the body — so the header contributed
        // nothing at all. Reference: 7001 4e75.
        let mut asm = Assembler::new(0);
        asm.assemble("\tIFND SYM\nSYM\tSET 1\n\tMOVEQ #1,D0\n\tENDC\n\tRTS\n")
            .unwrap();
        let words: Vec<u16> = asm.code.iter().flat_map(|i| i.words.clone()).collect();
        assert_eq!(words, vec![0x7001, 0x4E75], "guarded block was skipped");

        // The same holds when the SET comes after the block.
        let mut asm = Assembler::new(0);
        asm.assemble("\tIFND SYM\n\tMOVEQ #1,D0\n\tENDC\nSYM\tSET 1\n\tRTS\n")
            .unwrap();
        let words: Vec<u16> = asm.code.iter().flat_map(|i| i.words.clone()).collect();
        assert_eq!(words, vec![0x7001, 0x4E75]);
    }

    #[test]
    fn test_special_registers_do_not_collide_with_literal_values() {
        // CCR/SR/USP used to be marker immediates (-1, -2, 0x800), so
        // `MOVE.W #-1,D0` matched the CCR arm and assembled to 42C0 —
        // `MOVE CCR,D0`, a different instruction, silently.
        // Reference: move.w #-1,d0 = 303c ffff, move.w ccr,d0 = 42c0.
        let mut asm = Assembler::new(0);
        asm.set_cpu("68020");
        asm.assemble("\tMOVE.W #-1,D0\n\tMOVE.W #-2,D0\n\tMOVE.W CCR,D0\n\tMOVE.W SR,D0\n")
            .unwrap();
        let words: Vec<Vec<u16>> = asm.code.iter().map(|i| i.words.clone()).collect();
        assert_eq!(words[0], vec![0x303C, 0xFFFF], "MOVE.W #-1 became MOVE CCR");
        assert_eq!(words[1], vec![0x303C, 0xFFFE], "MOVE.W #-2 became MOVE SR");
        assert_eq!(words[2], vec![0x42C0]);
        assert_eq!(words[3], vec![0x40C0]);
    }

    #[test]
    fn test_byte_immediate_occupies_only_the_low_byte() {
        // The extension word's high byte is zero for a byte immediate.
        // Reference: move.b #-1,d0 = 103c 00ff, not 103c ffff.
        let mut asm = Assembler::new(0);
        asm.assemble("\tMOVE.B #-1,D0\n").unwrap();
        assert_eq!(asm.code[0].words, vec![0x103C, 0x00FF]);
    }

    #[test]
    fn test_undefined_symbol_is_named_in_the_error() {
        // By pass 2 every symbol is known, so an operand that still fails
        // to parse is a real error. Reporting the instruction's shape
        // ("MOVE requires source and destination") pointed at the wrong
        // thing entirely.
        let mut asm = Assembler::new(0);
        let err = asm.assemble("\tMOVE.L #NOSUCHSYM,D1\n").unwrap_err();
        assert!(
            err.message.contains("NOSUCHSYM"),
            "error should name the missing symbol, got: {}",
            err.message
        );
    }

    #[test]
    fn test_octal_literals() {
        // `@17` is the Motorola octal form the reference assembler accepts.
        // It was not implemented at all, in any of the three number parsers.
        let mut asm = Assembler::new(0);
        asm.assemble("    MOVEQ #@17,D0\n    DC.W @17\n    DC.L @777\n")
            .unwrap();
        assert_eq!(asm.code[0].words, vec![0x700F]); // @17 = 15
        assert_eq!(asm.code[1].words, vec![0x000F]);
        assert_eq!(asm.code[2].words, vec![0x0000, 0x01FF]); // @777 = 511

        // And inside expressions and EQU.
        let mut asm = Assembler::new(0);
        asm.assemble("V   EQU @20\n    DC.W V+@10\n").unwrap();
        assert_eq!(asm.code[0].words, vec![0x0018]); // 16 + 8
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
        // Reference encoding: move.l ([a0]),d2 -> 2430 0151
        let bytes = assemble_source_with_cpu("    MOVE.L ([A0]),D2\n", "68020");
        assert_eq!(bytes, vec![0x24, 0x30, 0x01, 0x51]);
    }

    #[test]
    fn test_source_memory_indirect_with_bd() {
        // Reference encoding: move.l ([$10,a0]),d2 -> 2430 0161 0010
        let bytes = assemble_source_with_cpu("    MOVE.L ([$10,A0]),D2\n", "68020");
        assert_eq!(bytes, vec![0x24, 0x30, 0x01, 0x61, 0x00, 0x10]);
    }

    #[test]
    fn test_source_memory_indirect_long_bd() {
        // Base displacement outside 16-bit signed range forces bd_code=3 (long).
        // Reference encoding: move.l ([$100000,a0]),d2 -> 2430 0171 0010 0000
        let bytes = assemble_source_with_cpu("    MOVE.L ([$100000,A0]),D2\n", "68020");
        assert_eq!(bytes, vec![0x24, 0x30, 0x01, 0x71, 0x00, 0x10, 0x00, 0x00]);
    }

    #[test]
    fn test_source_memory_indirect_preindexed() {
        // Index inside the brackets: ([bd,An,Xn],od)
        // Reference encoding: move.l ([$10,a0,d1.w*2],$20),d2 -> 2430 1322 0010 0020
        let bytes = assemble_source_with_cpu("    MOVE.L ([$10,A0,D1.W*2],$20),D2\n", "68020");
        assert_eq!(bytes, vec![0x24, 0x30, 0x13, 0x22, 0x00, 0x10, 0x00, 0x20]);
    }

    #[test]
    fn test_source_memory_indirect_postindexed() {
        // Index outside the brackets: ([bd,An],Xn,od)
        // Reference encoding: move.l ([$10,a0],d1.w*2,$20),d2 -> 2430 1326 0010 0020
        let bytes = assemble_source_with_cpu("    MOVE.L ([$10,A0],D1.W*2,$20),D2\n", "68020");
        assert_eq!(bytes, vec![0x24, 0x30, 0x13, 0x26, 0x00, 0x10, 0x00, 0x20]);
    }

    #[test]
    fn test_source_memory_indirect_base_suppressed() {
        // No An/PC in the brackets: base register suppressed, index-only
        // (preindexed - the index is inside the brackets, so is_postindexed
        // is false regardless of the trailing ",D2"). bd_code=1 (null
        // displacement, not 0/reserved) since there's no base displacement.
        let bytes = assemble_source_with_cpu("    MOVE.L ([D1.W*2],D2),D3\n", "68020");
        assert_eq!(bytes, vec![0x26, 0x30, 0x21, 0x91]);
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
        //
        // Extension word layout: bit15=D/A, bits14-12=Xn, bit11=W/L,
        // bits10-9=scale, bit8=0, bits7-0=disp. Xn=D1 -> 0x1000, disp=0.
        // (These expected values were previously wrong — 0x0801/0x0001 —
        // because ea_encode.rs destructured AddrRegIndirectIndex's `Xn`
        // and `disp` fields swapped, an all-CPU bug fixed alongside this
        // test; disp=0/Xn=D0-D1 happened to mostly cancel the swap out
        // except for the long bit ending up in the wrong nibble.)
        let bytes_l = assemble_source_with_cpu("    MOVE.L (0,A0,D1.L),D0\n", "68000");
        let bytes_w = assemble_source_with_cpu("    MOVE.L (0,A0,D1.W),D0\n", "68000");
        assert_eq!(bytes_l, vec![0x20, 0x30, 0x18, 0x00]);
        assert_eq!(bytes_w, vec![0x20, 0x30, 0x10, 0x00]);
    }

    #[test]
    fn test_pc_relative_index_l_suffix_sets_long_bit() {
        // Xn=D2 -> bits14-12=0x2000.
        //
        // For `(d,PC,Xn)` the number is a target *address*, encoded
        // relative to the extension word (instruction start + 2), exactly
        // like the non-indexed `(d,PC)` form. The instruction sits at 0
        // here, so target $4 becomes displacement 4 - 2 = 2.
        // Reference encoding: 203B 2002.
        let bytes_l = assemble_source_with_cpu("    MOVE.L ($4,PC,D2.L),D0\n", "68000");
        let bytes_w = assemble_source_with_cpu("    MOVE.L ($4,PC,D2.W),D0\n", "68000");
        assert_eq!(bytes_w, vec![0x20, 0x3B, 0x20, 0x02]);
        // Only the extension word's long bit (0x0800) should differ.
        assert_eq!(bytes_l, vec![0x20, 0x3B, 0x28, 0x02]);
    }

    /// Regression: a *label* in either PC-relative form resolved to 0.
    ///
    /// Both parsers only ran `evaluate_simple_number`, which understands
    /// literals but not symbols, and fell back to 0 — so `(target,PC)`
    /// measured its displacement from address 0 instead of from `target`,
    /// and `(target,PC,Xn)` encoded a displacement byte of 0. Neither was
    /// covered, because every existing case used a numeric displacement.
    ///
    /// Reference encoding for this source: 41FA 0006 43FB 1002 4E71.
    #[test]
    fn test_pc_relative_forms_resolve_label_targets() {
        let bytes = assemble_source_with_cpu(
            "    ORG $1000\n    LEA (target,PC),A0\n    LEA (target,PC,D1.W),A1\ntarget:\n    NOP\n",
            "68020",
        );
        assert_eq!(
            bytes,
            vec![0x41, 0xFA, 0x00, 0x06, 0x43, 0xFB, 0x10, 0x02, 0x4E, 0x71]
        );
    }

    /// Regression: `(d,PC,Xn)` whose target is too far for the brief
    /// format's 8-bit displacement must be a clean error, not a silently
    /// truncated encoding.
    #[test]
    fn test_pc_relative_index_out_of_range_is_error() {
        let mut asm = Assembler::new(0x1000);
        asm.set_cpu("68000");
        let err = asm.assemble_bytes("    ORG $1000\n    MOVE.L ($4,PC,D1.W),D2\n");
        assert!(
            err.is_err(),
            "expected out-of-range error, got {:?}",
            err.map(|b| b.len())
        );
    }

    /// Regression for the ea_encode.rs Xn/disp field-swap bug: the brief
    /// index extension word's Xn nibble and displacement byte were bound
    /// in the wrong order (compiled silently since both are integers), so
    /// any case where the index register number and displacement value
    /// differed encoded garbage. Uses asymmetric operands (index != disp)
    /// so the swap would be caught, unlike the pre-existing tests above
    /// which happened to use disp=0/4 with small Xn numbers.
    #[test]
    fn test_addr_reg_indirect_index_asymmetric_xn_and_disp() {
        // (4,A0,D1.W): Xn=D1 -> bits14-12=0x1000, disp=4.
        let bytes = assemble_source_with_cpu("    MOVE.W (4,A0,D1.W),D0\n", "68020");
        assert_eq!(bytes, vec![0x30, 0x30, 0x10, 0x04]);

        // (0x7F,A0,D2.L): Xn=D2 -> bits14-12=0x2000, long bit 0x0800,
        // disp=0x7F. Previously disp (0x7F) landed in the register-number
        // nibble, overflowing into the A/D-register-select bit (15).
        let bytes2 = assemble_source_with_cpu("    LEA ($7F,A0,D2.L),A2\n", "68020");
        assert_eq!(bytes2, vec![0x45, 0xF0, 0x28, 0x7F]);
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
        // DBNE is condition code 6, so the opword is 0x56C9. This test
        // asserted 0x57C9 — the encoding of DBEQ — because the encoder
        // used 0x51C8 as its base, which ORs DBF's condition bit into
        // every even condition. Reference: `dbne d1,lbl` = `56c9 fffe`.
        let bytes = assemble_source_with_cpu("LOOP:\n    DBNE D1,LOOP\n", "68000");
        assert_eq!(bytes, vec![0x56, 0xC9, 0xFF, 0xFE]);
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
