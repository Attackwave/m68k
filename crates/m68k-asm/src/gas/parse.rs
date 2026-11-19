//! Reading one GAS source file into sections of items.
//!
//! The model keeps what GNU as would put into an object file: per section
//! a list of labels, data, alignments and instructions, plus the file's
//! `.globl`/`.comm` declarations. Nothing here knows addresses; symbols
//! stay names until the files are linked.

use std::collections::{HashMap, HashSet};

use super::expr::{Expr, is_sym_char, is_sym_start, parse_escape, parse_expr};

/// What kind of output a section contributes to, in image order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Text,
    Rodata,
    Data,
    Bss,
}

#[derive(Debug)]
pub struct SectionModel {
    pub name: String,
    pub class: Class,
    /// `"aMS"` with entity size 1: NUL-terminated strings the link may
    /// share between files.
    pub merge_strings: bool,
    pub align: u32,
    pub items: Vec<(Item, usize)>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Label(String),
    Bytes(Vec<u8>),
    /// Zero bytes; the only content a BSS section may have.
    Zero(u32),
    Data {
        width: u8,
        expr: Expr,
    },
    Align {
        align: u32,
        fill: i64,
        word: bool,
        max: Option<u32>,
    },
    Insn(Insn),
    RelocTable,
}

#[derive(Debug, Clone)]
pub struct Insn {
    pub mnemonic: String,
    pub size: Option<char>,
    pub ops: Vec<Op>,
}

/// An operand as written; `None` as the base of an index is `%pc`.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    DReg(u8),
    AReg(u8),
    Ind(u8),
    PostInc(u8),
    PreDec(u8),
    Disp(Expr, u8),
    PcDisp(Expr),
    Index {
        base: Option<u8>,
        disp: Expr,
        xn: u8,
        long: bool,
        scale: u8,
    },
    Abs(Expr, Option<char>),
    Imm(Expr),
    /// A register list, bit 0 = %d0 .. bit 15 = %a7.
    RegList(u16),
    Special(&'static str),
}

#[derive(Debug, Default)]
pub struct FileModel {
    pub name: String,
    pub sections: Vec<SectionModel>,
    pub globals: HashSet<String>,
    /// `.comm` symbols that are not `.local`: `(name, size, align, line)`.
    pub commons: Vec<(String, u32, u32, usize)>,
}

/// Errors as `(line, message)`.
pub type LineErrors = Vec<(usize, String)>;

/// Section names whose content is dropped: debug info and notes, which
/// a flat image has no place for.
fn is_discarded(name: &str) -> bool {
    name.starts_with(".debug") || name.starts_with(".note") || name == ".comment"
}

fn class_of(name: &str) -> Option<Class> {
    let is =
        |base: &str| name == base || name.strip_prefix(base).is_some_and(|r| r.starts_with('.'));
    if is(".text") {
        Some(Class::Text)
    } else if is(".rodata") {
        Some(Class::Rodata)
    } else if is(".data") {
        Some(Class::Data)
    } else if is(".bss") {
        Some(Class::Bss)
    } else {
        None
    }
}

/// The location after the source's comments: `|` anywhere and `#` at the
/// start of a line begin one. Strings and character constants are kept.
fn strip_comment(line: &str) -> &str {
    if line.trim_start().starts_with('#') {
        return "";
    }
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'\'' => {
                i += if b.get(i + 1) == Some(&b'\\') { 2 } else { 1 };
            }
            b'|' => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

/// Splits on `sep` outside parentheses, strings and character constants.
fn split_top(text: &str, sep: u8) -> Vec<&str> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let (mut depth, mut start, mut i) = (0i32, 0, 0);
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'\'' => i += if b.get(i + 1) == Some(&b'\\') { 2 } else { 1 },
            b'(' => depth += 1,
            b')' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&text[start.min(text.len())..]);
    out
}

/// A register `%name`: data and address registers as 0..15, or one of
/// the special registers.
fn register(text: &str) -> Option<Op> {
    let name = text.trim().strip_prefix('%')?.to_ascii_lowercase();
    let n = |s: &str| s.parse::<u8>().ok().filter(|&n| n < 8);
    Some(match name.as_str() {
        "sp" => Op::AReg(7),
        "fp" => Op::AReg(6),
        "sr" => Op::Special("SR"),
        "ccr" => Op::Special("CCR"),
        "usp" => Op::Special("USP"),
        _ => {
            if let Some(d) = name.strip_prefix('d') {
                Op::DReg(n(d)?)
            } else {
                Op::AReg(n(name.strip_prefix('a')?)?)
            }
        }
    })
}

fn reg_number(op: &Op) -> Option<u8> {
    match op {
        Op::DReg(n) => Some(*n),
        Op::AReg(n) => Some(n + 8),
        _ => None,
    }
}

/// An index register with its size and scale: `%d0`, `%d0.l`, `%a1.w*2`,
/// MIT `%d0:l` or `%d0:w:2`.
fn index_register(text: &str) -> Option<(u8, bool, u8)> {
    let t = text.trim();
    let (t, scale) = match t.split_once('*') {
        Some((r, s)) => (r, s.trim().parse::<u8>().ok()?),
        None => (t, 1),
    };
    let mut parts = t.split([':', '.']);
    let reg = reg_number(&register(parts.next()?)?)?;
    let long = match parts.next().map(|s| s.trim().to_ascii_lowercase()) {
        None => false,
        Some(s) if s == "w" => false,
        Some(s) if s == "l" => true,
        Some(_) => return None,
    };
    let scale = match parts.next() {
        Some(s) if scale == 1 => s.trim().parse::<u8>().ok()?,
        Some(_) => return None,
        None => scale,
    };
    if parts.next().is_some() || ![1, 2, 4, 8].contains(&scale) {
        return None;
    }
    Some((reg, long, scale))
}

/// A register list `%d0-%d2/%a0`.
fn register_list(text: &str) -> Option<u16> {
    let mut mask = 0u16;
    for part in text.split('/') {
        let (lo, hi) = match part.split_once('-') {
            Some((a, b)) => (reg_number(&register(a)?)?, reg_number(&register(b)?)?),
            None => {
                let r = reg_number(&register(part)?)?;
                (r, r)
            }
        };
        if lo > hi {
            return None;
        }
        for r in lo..=hi {
            mask |= 1 << r;
        }
    }
    Some(mask)
}

/// Per-file parsing state.
struct FileParser {
    model: FileModel,
    current: Option<usize>,
    /// Inside a discarded section: everything but a section switch is
    /// skipped.
    discarding: bool,
    locals: HashSet<String>,
    /// Definitions seen so far of each numeric local label.
    numeric: HashMap<u32, u32>,
    /// `.local` + `.comm`: `(name, size, align, line)`.
    local_commons: Vec<(String, u32, u32, usize)>,
    errors: LineErrors,
}

impl FileParser {
    fn local_name(&self, n: u32, forward: bool) -> String {
        let count = self.numeric.get(&n).copied().unwrap_or(0);
        let k = if forward { count + 1 } else { count };
        // `\u{1}` cannot appear in a source symbol, so these never clash.
        format!(".L{n}\u{1}{k}")
    }

    fn expr(&self, text: &str) -> Result<Expr, String> {
        parse_expr(text, &|n, f| self.local_name(n, f))
    }

    fn switch_section(
        &mut self,
        name: &str,
        flags: Option<&str>,
        entsize: Option<&str>,
    ) -> Result<(), String> {
        if is_discarded(name) {
            self.discarding = true;
            return Ok(());
        }
        self.discarding = false;
        let class = class_of(name).ok_or_else(|| {
            format!("section '{name}' is not supported (only .text*, .rodata*, .data*, .bss*)")
        })?;
        if let Some(i) = self.model.sections.iter().position(|s| s.name == name) {
            self.current = Some(i);
            return Ok(());
        }
        let flags = flags.unwrap_or("");
        let merge = flags.contains('M');
        if merge && !(flags.contains('S') && entsize.map(str::trim) == Some("1")) {
            return Err(format!(
                "section '{name}': only string merging with entity size 1 (\"aMS\",@progbits,1) \
                 is supported"
            ));
        }
        if merge && class == Class::Bss {
            return Err(format!("section '{name}': a BSS section cannot be merged"));
        }
        // GNU as gives the three standard sections a 4-byte alignment;
        // every other one starts at 1 and grows with its `.align`s.
        let align = if matches!(name, ".text" | ".data" | ".bss") {
            4
        } else {
            1
        };
        self.model.sections.push(SectionModel {
            name: name.to_string(),
            class,
            merge_strings: merge,
            align,
            items: Vec::new(),
        });
        self.current = Some(self.model.sections.len() - 1);
        Ok(())
    }

    fn section(&mut self) -> &mut SectionModel {
        let i = self.current.unwrap_or(0);
        &mut self.model.sections[i]
    }

    fn push(&mut self, item: Item, line: usize) -> Result<(), String> {
        let sec = self.section();
        if sec.class == Class::Bss {
            let ok = match &item {
                Item::Label(_) | Item::Zero(_) => true,
                Item::Align { fill, .. } => *fill == 0,
                Item::Bytes(b) => b.iter().all(|&b| b == 0),
                _ => false,
            };
            if !ok {
                return Err(format!(
                    "section '{}' holds no data: only zeros may go there",
                    sec.name
                ));
            }
            if let Item::Bytes(b) = item {
                sec.items.push((Item::Zero(b.len() as u32), line));
                return Ok(());
            }
        }
        if let Item::Align { align, .. } = item {
            sec.align = sec.align.max(align);
        }
        sec.items.push((item, line));
        Ok(())
    }

    fn statement(&mut self, stmt: &str, line: usize) -> Result<(), String> {
        let mut rest = stmt.trim();
        // Labels: `name:` (or a numeric `1:`), any number of them.
        loop {
            let b = rest.as_bytes();
            let mut i = 0;
            let numeric = b.first().is_some_and(u8::is_ascii_digit);
            if numeric {
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            } else if b.first().is_some_and(|&c| is_sym_start(c)) {
                while i < b.len() && is_sym_char(b[i]) {
                    i += 1;
                }
            }
            if i == 0 || b.get(i) != Some(&b':') {
                break;
            }
            let name = &rest[..i];
            rest = rest[i + 1..].trim_start();
            if self.discarding {
                continue;
            }
            let name = if numeric {
                let n: u32 = name.parse().map_err(|_| format!("bad label '{name}'"))?;
                *self.numeric.entry(n).or_insert(0) += 1;
                self.local_name(n, false)
            } else {
                name.to_string()
            };
            self.push(Item::Label(name), line)?;
        }
        if rest.is_empty() {
            return Ok(());
        }
        let (word, args) = match rest.find(|c: char| c.is_ascii_whitespace()) {
            Some(i) => (&rest[..i], rest[i..].trim()),
            None => (rest, ""),
        };
        let lower = word.to_ascii_lowercase();
        if lower.starts_with('.') {
            self.directive(&lower, args, line)
        } else if self.discarding {
            Ok(())
        } else {
            let insn = self.instruction(&lower, args)?;
            self.push(Item::Insn(insn), line)
        }
    }

    fn args<'a>(&self, args: &'a str) -> Vec<&'a str> {
        if args.trim().is_empty() {
            Vec::new()
        } else {
            split_top(args, b',').into_iter().map(str::trim).collect()
        }
    }

    fn const_arg(&self, text: &str) -> Result<i64, String> {
        self.expr(text)?
            .const_value()
            .ok_or_else(|| format!("'{text}' must be a constant"))
    }

    fn directive(&mut self, name: &str, args: &str, line: usize) -> Result<(), String> {
        match name {
            ".text" | ".data" | ".bss" if args.is_empty() => {
                return self.switch_section(name, None, None);
            }
            ".text" | ".data" | ".bss" => {
                return Err(format!("subsections ({name} {args}) are not supported"));
            }
            ".section" => {
                let a = self.args(args);
                let name = a.first().map(|s| s.trim_matches('"')).unwrap_or("");
                return self.switch_section(
                    name,
                    a.get(1).map(|f| f.trim_matches('"')),
                    a.get(3).copied(),
                );
            }
            // Information for tools that read object files: no bytes.
            ".type" | ".size" | ".ident" | ".file" | ".loc" | ".hidden" | ".internal"
            | ".protected" => return Ok(()),
            n if n.starts_with(".cfi_") => return Ok(()),
            _ => {}
        }
        if self.discarding {
            return Ok(());
        }
        let a = self.args(args);
        match name {
            ".globl" | ".global" => {
                for s in a {
                    self.model.globals.insert(s.to_string());
                }
            }
            ".local" => {
                for s in a {
                    self.locals.insert(s.to_string());
                }
            }
            ".comm" => {
                let [sym, size, rest @ ..] = a.as_slice() else {
                    return Err(".comm needs a name and a size".into());
                };
                let size = u32::try_from(self.const_arg(size)?).map_err(|_| "bad .comm size")?;
                let align = match rest {
                    [align] => {
                        u32::try_from(self.const_arg(align)?).map_err(|_| "bad .comm alignment")?
                    }
                    _ => return Err(".comm needs an alignment (.comm name,size,align)".into()),
                };
                if !align.is_power_of_two() {
                    return Err(format!(".comm alignment {align} is not a power of 2"));
                }
                if self.locals.contains(*sym) {
                    // GNU as allocates a local common in a subsection of
                    // .bss that follows everything else there.
                    self.local_commons
                        .push((sym.to_string(), size, align, line));
                } else {
                    self.model
                        .commons
                        .push((sym.to_string(), size, align, line));
                }
            }
            ".align" | ".balign" | ".balignw" | ".p2align" => {
                let n = self.const_arg(a.first().ok_or("alignment missing")?)?;
                let align = if name == ".p2align" {
                    1u32.checked_shl(n as u32).filter(|_| (0..32).contains(&n))
                } else {
                    u32::try_from(n).ok().filter(|n| n.is_power_of_two())
                }
                .ok_or_else(|| format!("alignment {n} is not a power of 2"))?;
                let fill = match a.get(1) {
                    Some(f) if !f.is_empty() => self.const_arg(f)?,
                    _ => 0,
                };
                let max = match a.get(2) {
                    Some(m) => Some(u32::try_from(self.const_arg(m)?).map_err(|_| "bad maximum")?),
                    None => None,
                };
                let word = name == ".balignw";
                self.push(
                    Item::Align {
                        align,
                        fill,
                        word,
                        max,
                    },
                    line,
                )?;
            }
            ".byte" | ".word" | ".short" | ".long" | ".int" => {
                let width = match name {
                    ".byte" => 1,
                    ".word" | ".short" => 2,
                    _ => 4,
                };
                if a.is_empty() {
                    return Err(format!("{name} needs a value"));
                }
                for v in a {
                    let expr = self.expr(v)?;
                    self.push(Item::Data { width, expr }, line)?;
                }
            }
            ".string" | ".asciz" | ".ascii" => {
                for s in a {
                    let mut bytes = string_literal(s)?;
                    if name != ".ascii" {
                        bytes.push(0);
                    }
                    self.push(Item::Bytes(bytes), line)?;
                }
            }
            ".base64" => {
                for s in a {
                    let text = s
                        .strip_prefix('"')
                        .and_then(|t| t.strip_suffix('"'))
                        .ok_or(".base64 needs a quoted string")?;
                    self.push(Item::Bytes(base64(text)?), line)?;
                }
            }
            ".zero" | ".skip" | ".space" => {
                let n = u32::try_from(self.const_arg(a.first().ok_or("size missing")?)?)
                    .map_err(|_| format!("bad {name} size"))?;
                let fill = match a.get(1) {
                    Some(f) => self.const_arg(f)?,
                    None => 0,
                };
                if fill == 0 {
                    self.push(Item::Zero(n), line)?;
                } else {
                    self.push(Item::Bytes(vec![fill as u8; n as usize]), line)?;
                }
            }
            ".reloc_table" => {
                if !a.is_empty() {
                    return Err(".reloc_table takes no arguments".into());
                }
                if self.section().class == Class::Bss {
                    return Err(
                        ".reloc_table must be in a section that is stored in the image".into(),
                    );
                }
                self.push(Item::RelocTable, line)?;
            }
            _ => return Err(format!("unsupported directive '{name}'")),
        }
        Ok(())
    }

    fn instruction(&self, mnemonic: &str, args: &str) -> Result<Insn, String> {
        let (base, size) = match mnemonic.rsplit_once('.') {
            Some((b, s)) if matches!(s, "b" | "w" | "l" | "s") => (b, s.chars().next()),
            Some(_) => return Err(format!("bad size suffix in '{mnemonic}'")),
            None => (mnemonic, None),
        };
        let ops = self
            .args(args)
            .into_iter()
            .map(|t| self.operand(t))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Insn {
            mnemonic: base.to_string(),
            size,
            ops,
        })
    }

    fn operand(&self, text: &str) -> Result<Op, String> {
        let t = text.trim();
        if let Some(imm) = t.strip_prefix('#') {
            return Ok(Op::Imm(self.expr(imm)?));
        }
        if let Some(r) = register(t) {
            return Ok(r);
        }
        if t.starts_with('%') && (t.contains('/') || t.contains('-')) && !t.contains('@') {
            return register_list(t)
                .map(Op::RegList)
                .ok_or_else(|| format!("bad register list '{t}'"));
        }
        if let Some((reg, mit)) = t.split_once('@') {
            return self.mit_operand(reg, mit, t);
        }
        if let Some(inner) = t.strip_prefix("-(").and_then(|r| r.strip_suffix(')'))
            && let Some(Op::AReg(n)) = register(inner)
        {
            return Ok(Op::PreDec(n));
        }
        if let Some(inner) = t.strip_prefix('(').and_then(|r| r.strip_suffix(")+"))
            && let Some(Op::AReg(n)) = register(inner)
        {
            return Ok(Op::PostInc(n));
        }
        // An explicit absolute size: `4.w`, `sym+12.l`, `(x).w`.
        let lower = t.to_ascii_lowercase();
        if let Some(size) = [".w", ".l"].iter().find(|s| lower.ends_with(*s))
            && !t.contains('%')
        {
            let body = &t[..t.len() - 2];
            return Ok(Op::Abs(self.expr(body)?, size.chars().nth(1)));
        }
        if t.ends_with(')')
            && let Some(open) = last_group(t)
        {
            let inner = &t[open + 1..t.len() - 1];
            if inner.contains('%') {
                return self.paren_operand(&t[..open], inner, t);
            }
        }
        if t.contains('%') {
            return Err(format!("cannot parse operand '{t}'"));
        }
        Ok(Op::Abs(self.expr(t)?, None))
    }

    /// `disp(...)` and `(disp,...)` forms; `prefix` is the displacement
    /// written before the parentheses.
    fn paren_operand(&self, prefix: &str, inner: &str, whole: &str) -> Result<Op, String> {
        let parts: Vec<&str> = split_top(inner, b',').into_iter().map(str::trim).collect();
        let bad = || format!("cannot parse operand '{whole}'");
        let mut disp: Option<&str> = (!prefix.trim().is_empty()).then_some(prefix.trim());
        let mut regs = parts.as_slice();
        if let Some((first, rest)) = regs.split_first()
            && !first.starts_with('%')
        {
            if disp.is_some() {
                return Err(bad());
            }
            disp = Some(first);
            regs = rest;
        }
        let disp_expr = match disp {
            Some(d) => self.expr(d)?,
            None => Expr::Num(0),
        };
        let base = match regs.first() {
            Some(r) if r.eq_ignore_ascii_case("%pc") => None,
            Some(r) => match register(r) {
                Some(Op::AReg(n)) => Some(n),
                _ => return Err(bad()),
            },
            None => return Err(bad()),
        };
        match regs {
            [_] => Ok(match (base, disp) {
                (None, _) => Op::PcDisp(disp_expr),
                (Some(n), None) => Op::Ind(n),
                (Some(n), Some(_)) => Op::Disp(disp_expr, n),
            }),
            [_, x] => {
                let (xn, long, scale) = index_register(x).ok_or_else(bad)?;
                Ok(Op::Index {
                    base,
                    disp: disp_expr,
                    xn,
                    long,
                    scale,
                })
            }
            _ => Err(bad()),
        }
    }

    /// MIT syntax: `%a0@`, `%a0@+`, `%a0@-`, `%a0@(d)`, `%pc@(d,%d0:w)`.
    fn mit_operand(&self, reg: &str, rest: &str, whole: &str) -> Result<Op, String> {
        let bad = || format!("cannot parse operand '{whole}'");
        let base = if reg.trim().eq_ignore_ascii_case("%pc") {
            None
        } else {
            match register(reg) {
                Some(Op::AReg(n)) => Some(n),
                _ => return Err(bad()),
            }
        };
        let rest = rest.trim();
        match (base, rest) {
            (Some(n), "") => return Ok(Op::Ind(n)),
            (Some(n), "+") => return Ok(Op::PostInc(n)),
            (Some(n), "-") => return Ok(Op::PreDec(n)),
            _ => {}
        }
        let inner = rest
            .strip_prefix('(')
            .and_then(|r| r.strip_suffix(')'))
            .ok_or_else(bad)?;
        let parts: Vec<&str> = split_top(inner, b',').into_iter().map(str::trim).collect();
        let disp = self.expr(parts.first().ok_or_else(bad)?)?;
        match (parts.as_slice(), base) {
            ([_], None) => Ok(Op::PcDisp(disp)),
            ([_], Some(n)) => Ok(Op::Disp(disp, n)),
            ([_, x], base) => {
                let (xn, long, scale) = index_register(x).ok_or_else(bad)?;
                Ok(Op::Index {
                    base,
                    disp,
                    xn,
                    long,
                    scale,
                })
            }
            _ => Err(bad()),
        }
    }
}

/// The start of the last top-level parenthesised group of `t`, which
/// ends `t`.
fn last_group(t: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in t.bytes().enumerate().rev() {
        match c {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The bytes of a `"..."` literal.
fn string_literal(s: &str) -> Result<Vec<u8>, String> {
    let body = s
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .ok_or_else(|| format!("expected a quoted string, got {s}"))?;
    let b = body.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let (c, used) = parse_escape(&b[i..]).ok_or("bad escape in string")?;
        out.push(c);
        i += used;
    }
    Ok(out)
}

/// Standard base64 with `=` padding.
fn base64(text: &str) -> Result<Vec<u8>, String> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let data = text.trim_end_matches('=').as_bytes();
    if !text.len().is_multiple_of(4) || text.len() - data.len() > 2 {
        return Err("malformed .base64 string".into());
    }
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    for chunk in data.chunks(4) {
        let mut acc = 0u32;
        for &c in chunk {
            acc = (acc << 6) | value(c).ok_or("bad character in .base64 string")?;
        }
        acc <<= 6 * (4 - chunk.len());
        out.extend_from_slice(&acc.to_be_bytes()[1..chunk.len()]);
    }
    Ok(out)
}

/// Reads one source file.
pub fn parse_file(name: &str, text: &str) -> (FileModel, LineErrors) {
    let mut p = FileParser {
        model: FileModel {
            name: name.to_string(),
            ..Default::default()
        },
        current: None,
        discarding: false,
        locals: HashSet::new(),
        numeric: HashMap::new(),
        local_commons: Vec::new(),
        errors: Vec::new(),
    };
    // GNU as creates these three in every object, in this order.
    for s in [".text", ".data", ".bss"] {
        let _ = p.switch_section(s, None, None);
    }
    p.current = Some(0);
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        for stmt in split_top(strip_comment(raw), b';') {
            if let Err(e) = p.statement(stmt, line) {
                p.errors.push((line, e));
            }
        }
    }
    let _ = p.switch_section(".bss", None, None);
    for (name, size, align, line) in std::mem::take(&mut p.local_commons) {
        let items = [
            Item::Align {
                align,
                fill: 0,
                word: false,
                max: None,
            },
            Item::Label(name),
            Item::Zero(size),
        ];
        for item in items {
            if let Err(e) = p.push(item, line) {
                p.errors.push((line, e));
            }
        }
    }
    (p.model, p.errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(text: &str) -> Op {
        let (m, e) = parse_file("t.s", &format!(" tst.l {text}\n"));
        assert!(e.is_empty(), "{e:?}");
        match &m.sections[0].items[0].0 {
            Item::Insn(i) => i.ops[0].clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn operand_forms() {
        let sym = |s: &str| Expr::Sym(s.into());
        assert_eq!(op("%d3"), Op::DReg(3));
        assert_eq!(op("%sp"), Op::AReg(7));
        assert_eq!(op("%fp"), Op::AReg(6));
        assert_eq!(op("(%a0)"), Op::Ind(0));
        assert_eq!(op("(%sp)+"), Op::PostInc(7));
        assert_eq!(op("-(%sp)"), Op::PreDec(7));
        assert_eq!(op("%a0@-"), Op::PreDec(0));
        assert_eq!(op("8(%sp)"), Op::Disp(Expr::Num(8), 7));
        assert_eq!(
            op("(-8,%fp)"),
            Op::Disp(Expr::Neg(Box::new(Expr::Num(8))), 6)
        );
        assert_eq!(
            op("(%a0,%d0.l)"),
            Op::Index {
                base: Some(0),
                disp: Expr::Num(0),
                xn: 0,
                long: true,
                scale: 1
            }
        );
        assert_eq!(
            op(".L4(%pc,%d0.l)"),
            Op::Index {
                base: None,
                disp: sym(".L4"),
                xn: 0,
                long: true,
                scale: 1
            }
        );
        assert_eq!(
            op("%pc@(2,%d0:w)"),
            Op::Index {
                base: None,
                disp: Expr::Num(2),
                xn: 0,
                long: false,
                scale: 1
            }
        );
        assert_eq!(op("(reverse.isra.0)"), Op::Abs(sym("reverse.isra.0"), None));
        assert_eq!(op("4.w"), Op::Abs(Expr::Num(4), Some('w')));
        assert_eq!(op("#0x10"), Op::Imm(Expr::Num(16)));
        assert_eq!(op("%d2-%d3/%a2"), Op::RegList(0x040C));
        assert_eq!(op("lf(%pc)"), Op::PcDisp(sym("lf")));
    }

    #[test]
    fn comments_strings_and_statements() {
        let src = "#APP\n| 1311 \"x.h\" 1\n\t.string \"a|b\\n\" | c\n1: .byte 1 ; .byte 2b\n";
        let (m, e) = parse_file("t.s", src);
        assert!(e.is_empty(), "{e:?}");
        let items: Vec<_> = m.sections[0].items.iter().map(|i| &i.0).collect();
        assert!(matches!(items[0], Item::Bytes(b) if b == b"a|b\n\0"));
        assert!(matches!(items[1], Item::Label(l) if l == ".L1\u{1}1"));
        assert!(matches!(items[3], Item::Data { expr: Expr::Sym(s), .. } if s == ".L2\u{1}0"));
    }

    #[test]
    fn base64_decodes() {
        assert_eq!(base64("AQMDBAQFBg==").unwrap(), [1, 3, 3, 4, 4, 5, 6]);
        assert_eq!(
            base64("GZMNChoKAA==").unwrap(),
            [0x19, 0x93, 0x0d, 0x0a, 0x1a, 0x0a, 0]
        );
        assert!(base64("AQ=").is_err());
    }

    #[test]
    fn unknown_sections_and_directives_are_errors() {
        let (_, e) = parse_file(
            "t.s",
            "\t.section .ctors\n\t.weak x\n\t.section .debug_info\n\t.uleb128 4\n",
        );
        assert_eq!(e.len(), 2, "{e:?}");
    }
}
