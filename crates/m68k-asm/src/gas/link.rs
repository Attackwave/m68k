//! Linking the parsed files into one flat image.
//!
//! Each input section is relaxed on its own, as GNU as does it: a branch
//! or an absolute reference can only shrink to a PC-relative form when
//! its target is a non-global symbol of the same section. Then the
//! sections are laid out the way `ld` lays them out with a script
//! `*(.text .text.*) *(.rodata .rodata.*) *(.data .data.*)` followed by
//! `.bss` and `COMMON`, mergeable strings shared between files, and every
//! item is encoded with the final addresses.
//!
//! Relocations are found by encoding the image a second time at an
//! address moved by [`PROBE`]: a longword that moves by exactly that much
//! holds an address of the image.

use std::collections::HashMap;

use m68k_core::operands::Operand;

use super::expr::{Expr, Linear};
use super::parse::{Class, FileModel, Insn, Item, Op, SectionModel};
use super::{GasError, GasImage};
use crate::encoder::encode_instruction;

/// How far the probe encoding moves the image: a multiple of 4 KiB, so
/// alignment padding stays the same, that changes both words of a
/// longword.
const PROBE: u32 = 0x0001_1000;

/// The symbols the link defines.
const LINKER_SYMBOLS: [&str; 3] = ["__image_end", "__bss_start", "__bss_end"];

/// `.` in an expression: the location of the item being encoded.
const DOT: usize = usize::MAX;

type SymId = usize;

/// Strings of a mergeable section: input offset, length with the NUL,
/// index of the distinct string.
type Strings = Vec<(u32, u32, usize)>;

#[derive(Debug, Clone, Copy)]
enum Def {
    /// An item (a label) of a section.
    Item {
        sec: usize,
        item: usize,
    },
    Common(usize),
    Linker(usize),
}

struct Sym {
    name: String,
    def: Option<Def>,
}

/// The relaxation states of a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Reach {
    Byte,
    Word,
    /// `jmp`/`jsr` absolute long, around an inverted branch for a
    /// condition, or behind a DBcc that skips over it.
    Abs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchKind {
    /// `jra`, `jbsr`, `jXX`: relaxed up to an absolute jump.
    J,
    /// `bra`, `bsr`, `bXX`: byte or word, nothing further.
    B,
    /// `dbXX %dn,target`.
    Db(u8),
}

#[derive(Debug, Clone, Copy)]
enum Form {
    Fixed,
    Align,
    Table,
    Branch {
        kind: BranchKind,
        cc: u8,
        /// Same-section target, which makes the branch relaxable.
        local: Option<(SymId, i64)>,
        reach: Reach,
        /// The size was written (`.s`/`.w`) or the target is out of the
        /// section: no relaxation.
        fixed: bool,
    },
    /// An absolute operand that becomes `(d16,%pc)` while in reach.
    PcRel {
        op: usize,
        target: (SymId, i64),
        long: bool,
        short_size: u32,
    },
}

struct Sec {
    file: usize,
    model: SectionModel,
    forms: Vec<Form>,
    sizes: Vec<u32>,
    offs: Vec<u32>,
    size: u32,
    /// Merged strings: the input offset and length (with the NUL) of
    /// each string, and where it went in the merged block.
    strings: Vec<(u32, u32, u32)>,
    /// The merged block, for the section that holds it.
    merged: Option<Vec<u8>>,
    /// The section holding the merged block this one's strings went to.
    holder: usize,
}

/// A value as an expression produced it: `weight` 1 is an address of
/// the image, 0 a plain number.
#[derive(Debug, Clone, Copy)]
struct Val {
    value: i64,
    weight: i64,
}

struct Linker {
    files: Vec<FileModel>,
    secs: Vec<Sec>,
    syms: Vec<Sym>,
    globals: HashMap<String, SymId>,
    locals: Vec<HashMap<String, SymId>>,
    /// Global commons: `(symbol, size, align)`.
    commons: Vec<(SymId, u32, u32)>,
    cpu: String,
    errors: Vec<GasError>,
    /// Items already reported as wrong, not encoded again.
    failed: std::collections::HashSet<(usize, usize)>,
}

/// Addresses of one layout.
struct Layout {
    base: Vec<u32>,
    common: Vec<u32>,
    linker: [u32; 3],
}

impl Linker {
    fn error(&mut self, file: usize, line: usize, message: impl Into<String>) {
        self.errors.push(GasError {
            file: self.files[file].name.clone(),
            line,
            message: message.into(),
        });
    }

    fn lookup(&self, file: usize, name: &str) -> Result<SymId, String> {
        self.locals[file]
            .get(name)
            .or_else(|| self.globals.get(name))
            .copied()
            .ok_or_else(|| format!("undefined symbol '{}'", display_name(name)))
    }

    fn sym_def(&self, id: SymId) -> Result<Def, String> {
        self.syms[id]
            .def
            .ok_or_else(|| format!("undefined symbol '{}'", display_name(&self.syms[id].name)))
    }

    /// Enters every definition: labels, commons and the link's own.
    fn define_symbols(&mut self) {
        for name in LINKER_SYMBOLS {
            let id = self.syms.len();
            self.syms.push(Sym {
                name: name.into(),
                def: Some(Def::Linker(id)),
            });
            self.globals.insert(name.into(), id);
        }
        let mut pending = Vec::new();
        for (si, sec) in self.secs.iter().enumerate() {
            for (ii, (item, line)) in sec.model.items.iter().enumerate() {
                if let Item::Label(name) = item {
                    pending.push((
                        sec.file,
                        *line,
                        name.clone(),
                        Def::Item { sec: si, item: ii },
                    ));
                }
            }
        }
        for (file, line, name, def) in pending {
            let global = self.files[file].globals.contains(&name);
            let existing = if global {
                self.globals.get(&name)
            } else {
                self.locals[file].get(&name)
            };
            if let Some(&id) = existing {
                let what = if LINKER_SYMBOLS.contains(&name.as_str()) {
                    "is defined by the link"
                } else {
                    "is already defined"
                };
                if self.syms[id].def.is_some() {
                    self.error(
                        file,
                        line,
                        format!("symbol '{}' {what}", display_name(&name)),
                    );
                    continue;
                }
            }
            let id = self.syms.len();
            self.syms.push(Sym {
                name: name.clone(),
                def: Some(def),
            });
            if global {
                self.globals.insert(name, id);
            } else {
                self.locals[file].insert(name, id);
            }
        }
        // A common is a definition only where nothing else defines the
        // name; the same common in several files is one block, as large
        // and as aligned as the largest.
        for f in 0..self.files.len() {
            for (name, size, align, line) in self.files[f].commons.clone() {
                if self.locals[f].contains_key(&name) {
                    self.error(
                        f,
                        line,
                        format!("'{name}' is defined in this file and also .comm"),
                    );
                    continue;
                }
                match self.globals.get(&name).copied() {
                    Some(id) => match self.syms[id].def {
                        Some(Def::Common(c)) => {
                            let entry = &mut self.commons[c];
                            entry.1 = entry.1.max(size);
                            entry.2 = entry.2.max(align);
                        }
                        Some(Def::Linker(_)) => {
                            self.error(f, line, format!("symbol '{name}' is defined by the link"))
                        }
                        _ => {}
                    },
                    None => {
                        let id = self.syms.len();
                        self.syms.push(Sym {
                            name: name.clone(),
                            def: Some(Def::Common(self.commons.len())),
                        });
                        self.commons.push((id, size, align));
                        self.globals.insert(name, id);
                    }
                }
            }
        }
    }

    /// The target of a branch or an absolute reference when GNU as would
    /// resolve it within the section: a symbol (plus a constant) defined
    /// in `sec` and not global.
    fn local_target(&self, sec: usize, expr: &Expr) -> Option<(SymId, i64)> {
        let (name, addend) = expr.symbol_plus_const()?;
        let file = self.secs[sec].file;
        let id = *self.locals[file].get(name)?;
        match self.syms[id].def? {
            Def::Item { sec: s, .. } if s == sec => Some((id, addend)),
            _ => None,
        }
    }

    /// Chooses each item's form and its first size.
    fn classify(&mut self) {
        for si in 0..self.secs.len() {
            let n = self.secs[si].model.items.len();
            let mut forms = Vec::with_capacity(n);
            let mut sizes = Vec::with_capacity(n);
            for ii in 0..n {
                let (item, line) = self.secs[si].model.items[ii].clone();
                let file = self.secs[si].file;
                let (form, size) = match &item {
                    Item::Label(_) => (Form::Fixed, 0),
                    Item::Bytes(b) => (Form::Fixed, b.len() as u32),
                    Item::Zero(n) => (Form::Fixed, *n),
                    Item::Data { width, .. } => (Form::Fixed, u32::from(*width)),
                    Item::Align { .. } => (Form::Align, 0),
                    Item::RelocTable => (Form::Table, 0),
                    Item::Insn(insn) => match self.classify_insn(si, insn) {
                        Ok(fs) => fs,
                        Err(e) => {
                            self.error(file, line, e);
                            self.failed.insert((si, ii));
                            (Form::Fixed, 0)
                        }
                    },
                };
                forms.push(form);
                sizes.push(size);
            }
            self.secs[si].forms = forms;
            self.secs[si].sizes = sizes;
        }
    }

    fn classify_insn(&self, sec: usize, insn: &Insn) -> Result<(Form, u32), String> {
        if let Some((kind, cc)) = branch_kind(&insn.mnemonic) {
            let target = match (kind, insn.ops.as_slice()) {
                (BranchKind::Db(_), [Op::DReg(_), Op::Abs(t, None)]) => t,
                (BranchKind::Db(_), _) => return Err(format!("{} needs %dn,label", insn.mnemonic)),
                (_, [Op::Abs(t, None)]) => t,
                _ => return Err(format!("{} needs a label", insn.mnemonic)),
            };
            let kind = match (kind, insn.ops.first()) {
                (BranchKind::Db(_), Some(Op::DReg(r))) => BranchKind::Db(*r),
                (k, _) => k,
            };
            let local = self.local_target(sec, target);
            let (reach, fixed) = match (kind, insn.size) {
                (BranchKind::J, Some(_)) | (BranchKind::Db(_), Some(_)) => {
                    return Err(format!("{} takes no size suffix", insn.mnemonic));
                }
                (BranchKind::B, Some('s' | 'b')) => (Reach::Byte, true),
                (BranchKind::B, Some('w')) => (Reach::Word, true),
                (BranchKind::B, Some(_)) => return Err("a 32-bit branch needs a 68020".into()),
                (BranchKind::B, None) if local.is_none() => (Reach::Word, true),
                (BranchKind::J, None) if local.is_none() => (Reach::Abs, true),
                (BranchKind::Db(_), None) if local.is_none() => (Reach::Abs, true),
                (BranchKind::Db(_), None) => (Reach::Word, false),
                (_, None) => (Reach::Byte, false),
            };
            let form = Form::Branch {
                kind,
                cc,
                local,
                reach,
                fixed,
            };
            return Ok((form, branch_size(kind, cc, reach)));
        }
        // An absolute operand that may become PC-relative: the one the
        // encoder accepts in its (d16,%pc) form.
        let mut candidates = insn.ops.iter().enumerate().filter_map(|(i, op)| match op {
            Op::Abs(e, None) => self.local_target(sec, e).map(|t| (i, t)),
            _ => None,
        });
        let candidate = candidates.next();
        let sizing = |op: Option<(usize, bool)>| -> Result<u32, String> {
            let words = self.encode_insn(insn, 0, &mut |e: &Expr| sizing_value(e), op)?;
            Ok(words.len() as u32 * 2)
        };
        if let Some((op, target)) = candidate
            && sizing(Some((op, false))).is_ok()
        {
            if candidates.next().is_some() {
                return Err("two operands that could be PC-relative are not supported".into());
            }
            let short_size = sizing(Some((op, false)))?;
            return Ok((
                Form::PcRel {
                    op,
                    target,
                    long: false,
                    short_size,
                },
                short_size,
            ));
        }
        Ok((Form::Fixed, sizing(None)?))
    }

    /// Lays out one section with the current forms; true when a form had
    /// to grow.
    fn relax_pass(&mut self, si: usize) -> bool {
        self.offsets(si);
        let mut grew = false;
        for ii in 0..self.secs[si].forms.len() {
            let off = self.secs[si].offs[ii];
            match self.secs[si].forms[ii] {
                Form::Branch {
                    kind,
                    cc,
                    local: Some((t, addend)),
                    reach,
                    fixed: false,
                } => {
                    let Ok(Def::Item { item, .. }) = self.sym_def(t) else {
                        continue;
                    };
                    let target = i64::from(self.secs[si].offs[item]) + addend;
                    let disp = target - i64::from(off) - 2;
                    let needed = if (-128..=127).contains(&disp)
                        && disp != 0
                        && !matches!(kind, BranchKind::Db(_))
                    {
                        Reach::Byte
                    } else if (-32768..=32767).contains(&disp) {
                        Reach::Word
                    } else if kind == BranchKind::B {
                        // Out of reach for `bra`: the encoder reports it.
                        Reach::Word
                    } else {
                        Reach::Abs
                    };
                    if needed > reach {
                        self.secs[si].forms[ii] = Form::Branch {
                            kind,
                            cc,
                            local: Some((t, addend)),
                            reach: needed,
                            fixed: false,
                        };
                        self.secs[si].sizes[ii] = branch_size(kind, cc, needed);
                        grew = true;
                    }
                }
                Form::PcRel {
                    op,
                    target: (t, addend),
                    long: false,
                    short_size,
                } => {
                    let Ok(Def::Item { item, .. }) = self.sym_def(t) else {
                        continue;
                    };
                    let target = i64::from(self.secs[si].offs[item]) + addend;
                    let Item::Insn(insn) = &self.secs[si].model.items[ii].0 else {
                        continue;
                    };
                    let Some(Op::Abs(cand, _)) = insn.ops.get(op) else {
                        continue;
                    };
                    // Only the relaxed operand is placed; the rest only
                    // has to have its size.
                    let mut at = |e: &Expr| {
                        if std::ptr::eq(e, cand) {
                            Ok(Val {
                                value: target,
                                weight: 1,
                            })
                        } else {
                            sizing_value(e)
                        }
                    };
                    if self
                        .encode_insn(insn, off, &mut at, Some((op, false)))
                        .is_err()
                    {
                        self.secs[si].forms[ii] = Form::PcRel {
                            op,
                            target: (t, addend),
                            long: true,
                            short_size,
                        };
                        self.secs[si].sizes[ii] = short_size + 2;
                        grew = true;
                    }
                }
                _ => {}
            }
        }
        grew
    }

    /// Item offsets within section `si` from the current sizes.
    fn offsets(&mut self, si: usize) {
        let sec = &mut self.secs[si];
        let mut off = 0u32;
        sec.offs.clear();
        for (ii, form) in sec.forms.iter().enumerate() {
            sec.offs.push(off);
            if let (Form::Align, Item::Align { align, max, .. }) = (form, &sec.model.items[ii].0) {
                let pad = off.next_multiple_of(*align) - off;
                sec.sizes[ii] = if max.is_some_and(|m| pad > m) { 0 } else { pad };
            }
            off += sec.sizes[ii];
        }
        sec.size = off;
    }

    /// Shares the strings of every mergeable section, as `ld` does:
    /// each distinct string once, a string that ends another one inside
    /// it, the block in the first such section.
    fn merge_strings(&mut self) {
        let mut groups: HashMap<u32, Vec<usize>> = HashMap::new();
        for &si in &self.link_order() {
            if self.secs[si].model.merge_strings {
                groups
                    .entry(self.secs[si].model.align)
                    .or_default()
                    .push(si);
            }
        }
        for (_, members) in groups {
            let mut unique: Vec<Vec<u8>> = Vec::new();
            let mut index: HashMap<Vec<u8>, usize> = HashMap::new();
            let mut per_sec: Vec<(usize, Strings)> = Vec::new();
            for &si in &members {
                let mut bytes = Vec::new();
                for (ii, (item, line)) in self.secs[si].model.items.iter().enumerate() {
                    match item {
                        Item::Label(_) => {}
                        Item::Bytes(b) => bytes.extend_from_slice(b),
                        Item::Zero(n) => bytes.resize(bytes.len() + *n as usize, 0),
                        Item::Align { .. } if self.secs[si].sizes[ii] == 0 => {}
                        _ => {
                            let (file, line) = (self.secs[si].file, *line);
                            self.error(
                                file,
                                line,
                                "a mergeable string section may hold only strings",
                            );
                            return;
                        }
                    }
                }
                if bytes.last().is_some_and(|&b| b != 0) {
                    let file = self.secs[si].file;
                    self.error(
                        file,
                        0,
                        format!(
                            "section {} does not end with a NUL",
                            self.secs[si].model.name
                        ),
                    );
                    return;
                }
                let mut strings = Vec::new();
                let mut start = 0usize;
                for (i, &b) in bytes.iter().enumerate() {
                    if b == 0 {
                        let s = bytes[start..i].to_vec();
                        let n = unique.len();
                        let e = *index.entry(s.clone()).or_insert_with(|| {
                            unique.push(s);
                            n
                        });
                        strings.push((start as u32, (i + 1 - start) as u32, e));
                        start = i + 1;
                    }
                }
                per_sec.push((si, strings));
            }
            // Tail merging over the strings sorted by their reversed
            // bytes: a string that the next larger one ends with lives
            // inside it.
            let mut order: Vec<usize> = (0..unique.len()).collect();
            order.sort_by(|&a, &b| unique[a].iter().rev().cmp(unique[b].iter().rev()));
            let mut inside: Vec<Option<usize>> = vec![None; unique.len()];
            if let Some((&last, rest)) = order.split_last() {
                let mut e = last;
                for &c in rest.iter().rev() {
                    if unique[c].len() < unique[e].len() && unique[e].ends_with(&unique[c]) {
                        inside[c] = Some(e);
                    } else {
                        e = c;
                    }
                }
            }
            let mut pos = vec![0u32; unique.len()];
            let mut block = Vec::new();
            for (i, s) in unique.iter().enumerate() {
                if inside[i].is_none() {
                    pos[i] = block.len() as u32;
                    block.extend_from_slice(s);
                    block.push(0);
                }
            }
            for i in 0..unique.len() {
                if let Some(c) = inside[i] {
                    pos[i] = pos[c] + (unique[c].len() - unique[i].len()) as u32;
                }
            }
            let holder = members[0];
            for (k, (si, strings)) in per_sec.into_iter().enumerate() {
                let sec = &mut self.secs[si];
                sec.holder = holder;
                sec.strings = strings
                    .into_iter()
                    .map(|(o, l, e)| (o, l, pos[e]))
                    .collect();
                sec.merged = (k == 0).then(|| block.clone());
            }
        }
    }

    /// The section's offset of input offset `off`, after merging.
    fn mapped(&self, si: usize, off: u32) -> Result<u32, String> {
        let sec = &self.secs[si];
        if !sec.model.merge_strings {
            return Ok(off);
        }
        sec.strings
            .iter()
            .find(|(o, l, _)| (*o..o + l).contains(&off))
            .map(|(o, _, p)| p + (off - o))
            .ok_or_else(|| "a label at the end of a mergeable string section".into())
    }

    fn stored_size(&self, si: usize) -> u32 {
        let sec = &self.secs[si];
        if sec.model.merge_strings {
            sec.merged.as_ref().map_or(0, |b| b.len() as u32)
        } else {
            sec.size
        }
    }

    /// Text, read-only data and data sections in image order: by class,
    /// then file, then the order of the sections in the file.
    fn link_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.secs.len())
            .filter(|&s| self.secs[s].model.class != Class::Bss)
            .collect();
        order.sort_by_key(|&s| (self.secs[s].model.class, self.secs[s].file));
        order
    }

    fn layout(&self, origin: u32) -> Layout {
        let mut base = vec![0u32; self.secs.len()];
        let mut dot = origin;
        for si in self.link_order() {
            dot = dot.next_multiple_of(self.secs[si].model.align);
            base[si] = dot;
            dot += self.stored_size(si);
        }
        let image_end = dot;
        let bss: Vec<usize> = (0..self.secs.len())
            .filter(|&s| self.secs[s].model.class == Class::Bss)
            .collect();
        let align = bss
            .iter()
            .map(|&s| self.secs[s].model.align)
            .chain(self.commons.iter().map(|c| c.2))
            .max()
            .unwrap_or(1);
        dot = dot.next_multiple_of(align);
        for si in bss {
            dot = dot.next_multiple_of(self.secs[si].model.align);
            base[si] = dot;
            dot += self.secs[si].size;
        }
        let mut common = Vec::new();
        for &(_, size, align) in &self.commons {
            dot = dot.next_multiple_of(align);
            common.push(dot);
            dot += size;
        }
        Layout {
            base,
            common,
            linker: [image_end, image_end, dot],
        }
    }

    fn address(&self, layout: &Layout, def: Def) -> Result<u32, String> {
        Ok(match def {
            Def::Item { sec, item } if self.secs[sec].model.merge_strings => {
                layout.base[self.secs[sec].holder] + self.mapped(sec, self.secs[sec].offs[item])?
            }
            Def::Item { sec, item } => layout.base[sec] + self.secs[sec].offs[item],
            Def::Common(c) => layout.common[c],
            Def::Linker(i) => layout.linker[i],
        })
    }

    /// The value of `expr` in file `file` with `dot` the location of the
    /// item. A difference of two symbols of one section counts their
    /// distance in the section as written, as GNU as resolves it.
    fn value(&self, layout: &Layout, file: usize, dot: Def, expr: &Expr) -> Result<Val, String> {
        let lin: Linear<SymId> = expr.eval(&mut |name| self.lookup(file, name), Some(DOT))?;
        let mut value = lin.k;
        let mut weight = 0;
        let def_of = |id: SymId| if id == DOT { Ok(dot) } else { self.sym_def(id) };
        let mut by_sec: HashMap<usize, i64> = HashMap::new();
        for &(id, c) in &lin.terms {
            if let Def::Item { sec, .. } = def_of(id)? {
                *by_sec.entry(sec).or_default() += c;
            }
        }
        for &(id, c) in &lin.terms {
            let def = def_of(id)?;
            weight += c;
            let addr = match def {
                Def::Item { sec, item } if by_sec[&sec] == 0 => {
                    i64::from(self.secs[sec].offs[item])
                }
                _ => i64::from(self.address(layout, def)?),
            };
            value += c * addr;
        }
        Ok(Val { value, weight })
    }

    /// Encodes an instruction at `pc`. `pcrel` is the operand that was
    /// relaxed and whether it stayed absolute long.
    fn encode_insn(
        &self,
        insn: &Insn,
        pc: u32,
        ev: &mut dyn FnMut(&Expr) -> Result<Val, String>,
        pcrel: Option<(usize, bool)>,
    ) -> Result<Vec<u16>, String> {
        let (mnemonic, size) = gas_choice(insn)?;
        if let Some(s) = size
            && !matches!(s, 'b' | 'w' | 'l')
        {
            return Err(format!("bad size .{s} for {}", insn.mnemonic));
        }
        let size_str = size.map(|c| c.to_string());
        let long_field = size == Some('l');
        let movem_predec = mnemonic == "MOVEM" && matches!(insn.ops.get(1), Some(Op::PreDec(_)));
        let mut ops = Vec::with_capacity(2);
        for (i, op) in insn.ops.iter().enumerate() {
            let narrow = |v: Val, what: &str| -> Result<i64, String> {
                match v.weight {
                    0 => Ok(v.value),
                    1 => Err(format!(
                        "an address in a {what}: only 32-bit fields can hold one"
                    )),
                    _ => Err("expression is neither a number nor an address".into()),
                }
            };
            let addr = |v: Val| -> Result<i64, String> {
                match v.weight {
                    0 | 1 => Ok(v.value),
                    _ => Err("expression is neither a number nor an address".into()),
                }
            };
            let pc_target = |v: Val| -> Result<i32, String> {
                match v.weight {
                    1 => Ok(v.value as i32),
                    0 if i == 0 => Ok((i64::from(pc) + 2 + v.value) as i32),
                    0 => Err(
                        "a numeric PC displacement is supported in the first operand only".into(),
                    ),
                    _ => Err("expression is neither a number nor an address".into()),
                }
            };
            ops.push(match op {
                Op::DReg(n) => Operand::DataReg(*n),
                Op::AReg(n) => Operand::AddrReg(*n),
                Op::Ind(n) => Operand::AddrRegIndirect(*n),
                Op::PostInc(n) => Operand::AddrRegPostInc(*n),
                Op::PreDec(n) => Operand::AddrRegPreDec(*n),
                Op::Special(s) => Operand::Special((*s).to_string()),
                Op::RegList(m) => Operand::Immediate(i64::from(*m)),
                Op::Disp(e, n) => {
                    let d = narrow(ev(e)?, "16-bit displacement")?;
                    if !(-32768..=32767).contains(&d) {
                        return Err(format!("displacement {d} does not fit 16 bits"));
                    }
                    Operand::AddrRegIndirectDisp(*n, d as i32, false)
                }
                Op::PcDisp(e) => Operand::PcRelativeDisp(pc_target(ev(e)?)?, false),
                Op::Index {
                    base: Some(n),
                    disp,
                    xn,
                    long,
                    scale,
                } => {
                    let d = narrow(ev(disp)?, "8-bit displacement")?;
                    let d = i8::try_from(d)
                        .map_err(|_| format!("displacement {d} does not fit 8 bits"))?;
                    Operand::AddrRegIndirectIndex(*n, *xn, d, *scale, *long)
                }
                Op::Index {
                    base: None,
                    disp,
                    xn,
                    long,
                    scale,
                } => Operand::PcRelativeIndex(*xn, Some(pc_target(ev(disp)?)?), *scale, *long),
                Op::Abs(e, Some('w')) => {
                    let v = narrow(ev(e)?, "16-bit absolute address")?;
                    Operand::AbsoluteShort(v as i32)
                }
                Op::Abs(e, _) => {
                    let v = ev(e)?;
                    match pcrel {
                        Some((op, false)) if op == i => {
                            Operand::PcRelativeDisp(pc_target(v)?, false)
                        }
                        _ if e.is_const() && (-32768..=32767).contains(&v.value) => {
                            Operand::AbsoluteShort(v.value as i32)
                        }
                        _ => Operand::AbsoluteLong(long32(addr(v)?)?),
                    }
                }
                Op::Imm(e) => {
                    let v = ev(e)?;
                    let v = if long_field {
                        addr(v)?
                    } else {
                        narrow(v, "field narrower than 32 bits")?
                    };
                    if mnemonic == "MOVEM" {
                        // GNU as takes the mask as it is encoded; the
                        // encoder wants bit 0 = %d0 in every mode.
                        let raw = v as u16;
                        Operand::Immediate(i64::from(if movem_predec {
                            raw.reverse_bits()
                        } else {
                            raw
                        }))
                    } else {
                        Operand::Immediate(v)
                    }
                }
            });
        }
        if ops.len() > 2 {
            return Err(format!("{} takes at most two operands", insn.mnemonic));
        }
        let mut words = encode_instruction(
            &mnemonic,
            size_str.as_deref(),
            ops.first(),
            ops.get(1),
            pc,
            &self.cpu,
        )
        .map_err(|e| e.message)?;
        m68k_core::cpu_gate::check_mnemonic_cpu(&mnemonic, &self.cpu)?;
        // GNU as stores a negative byte immediate sign-extended over its
        // whole extension word (`#-1` is $FFFF); the CPU reads the low
        // byte only.
        if size == Some('b')
            && let Some(Operand::Immediate(v)) = ops.first()
            && (-128..0).contains(v)
            && words.len() > 1
        {
            words[1] = *v as u16;
        }
        Ok(words)
    }

    /// The bytes of item `ii` of section `si`.
    fn encode_item(
        &self,
        layout: &Layout,
        si: usize,
        ii: usize,
        table: &[u8],
    ) -> Result<Vec<u8>, String> {
        let sec = &self.secs[si];
        let file = sec.file;
        let pc = layout.base[si] + sec.offs[ii];
        let here = Def::Item { sec: si, item: ii };
        let mut ev = |e: &Expr| self.value(layout, file, here, e);
        let item = &sec.model.items[ii].0;
        Ok(match (item, sec.forms[ii]) {
            (Item::Label(_), _) => Vec::new(),
            (Item::Bytes(b), _) => b.clone(),
            (Item::Zero(n), _) => vec![0; *n as usize],
            (Item::RelocTable, _) => table.to_vec(),
            (Item::Data { width, expr }, _) => {
                let v = ev(expr)?;
                let (lo, hi) = match width {
                    1 => (-0x80, 0xFF),
                    2 => (-0x8000, 0xFFFF),
                    _ => (-0x8000_0000, 0xFFFF_FFFF),
                };
                match v.weight {
                    0 => {}
                    1 if *width == 4 => {}
                    1 => {
                        return Err(format!(
                            "an address in a {}-bit field: only 32-bit fields can hold one",
                            width * 8
                        ));
                    }
                    _ => return Err("expression is neither a number nor an address".into()),
                }
                if !(lo..=hi).contains(&v.value) {
                    return Err(format!("value {} does not fit {} bits", v.value, width * 8));
                }
                v.value.to_be_bytes()[8 - *width as usize..].to_vec()
            }
            (Item::Align { fill, word, .. }, _) => {
                let pad = sec.sizes[ii] as usize;
                if *word {
                    if !pad.is_multiple_of(2) {
                        return Err(format!(
                            "alignment padding ({pad} bytes) not a multiple of 2"
                        ));
                    }
                    (*fill as u16).to_be_bytes().repeat(pad / 2)
                } else {
                    vec![*fill as u8; pad]
                }
            }
            (Item::Insn(insn), form) => {
                if !pc.is_multiple_of(2) {
                    return Err("instruction at an odd address".into());
                }
                let words = match form {
                    Form::Branch {
                        kind, cc, reach, ..
                    } => {
                        let target = match insn.ops.last() {
                            Some(Op::Abs(t, None)) => ev(t)?,
                            _ => unreachable!("classified as a branch"),
                        };
                        if target.weight != 1 {
                            return Err("a branch needs a label as its target".into());
                        }
                        branch_words(kind, cc, reach, pc, target.value)?
                    }
                    Form::PcRel { op, long, .. } => {
                        self.encode_insn(insn, pc, &mut ev, Some((op, long)))?
                    }
                    _ => self.encode_insn(insn, pc, &mut ev, None)?,
                };
                if words.len() as u32 * 2 != sec.sizes[ii] {
                    return Err("instruction size changed after layout".into());
                }
                words.iter().flat_map(|w| w.to_be_bytes()).collect()
            }
        })
    }

    /// Every stored byte of the image laid out at `origin`.
    fn encode_image(&mut self, origin: u32, table: &[u8]) -> (Vec<u8>, Vec<(u32, usize, usize)>) {
        let layout = self.layout(origin);
        let end = layout.linker[0];
        let mut image = vec![0u8; (end - origin) as usize];
        let mut locs = Vec::new();
        let mut errors = Vec::new();
        for si in self.link_order() {
            let start = (layout.base[si] - origin) as usize;
            if let Some(block) = &self.secs[si].merged {
                image[start..start + block.len()].copy_from_slice(block);
                continue;
            }
            if self.secs[si].model.merge_strings {
                continue;
            }
            for ii in 0..self.secs[si].model.items.len() {
                if self.failed.contains(&(si, ii)) {
                    continue;
                }
                let line = self.secs[si].model.items[ii].1;
                let at = start + self.secs[si].offs[ii] as usize;
                locs.push((at as u32, self.secs[si].file, line));
                match self.encode_item(&layout, si, ii, table) {
                    Ok(bytes) => image[at..at + bytes.len()].copy_from_slice(&bytes),
                    Err(e) => errors.push((self.secs[si].file, line, e)),
                }
            }
        }
        // BSS is not stored: the parser allowed nothing but zeros there.
        for (file, line, e) in errors {
            self.error(file, line, e);
        }
        (image, locs)
    }
}

fn long32(v: i64) -> Result<i32, String> {
    if (-0x8000_0000..=0xFFFF_FFFF).contains(&v) {
        Ok(v as u32 as i32)
    } else {
        Err(format!("value {v} does not fit 32 bits"))
    }
}

/// For sizing an instruction: every symbol 0, the weight as written.
fn sizing_value(e: &Expr) -> Result<Val, String> {
    let lin: Linear<u8> = e.eval(&mut |_| Ok(0), Some(0))?;
    Ok(Val {
        value: lin.k,
        weight: lin.terms.iter().map(|t| t.1).sum(),
    })
}

/// A numeric label `.L1\u{1}2` as the source wrote it.
fn display_name(name: &str) -> String {
    match name.strip_prefix(".L").and_then(|r| r.split_once('\u{1}')) {
        Some((n, _)) => format!("{n}b/{n}f"),
        None => name.to_string(),
    }
}

/// Condition codes by name, as in the Bcc/DBcc opcode.
fn condition(name: &str) -> Option<u8> {
    Some(match name {
        "hi" => 2,
        "ls" => 3,
        "cc" | "hs" => 4,
        "cs" | "lo" => 5,
        "ne" => 6,
        "eq" => 7,
        "vc" => 8,
        "vs" => 9,
        "pl" => 10,
        "mi" => 11,
        "ge" => 12,
        "lt" => 13,
        "gt" => 14,
        "le" => 15,
        _ => return None,
    })
}

/// The branch a mnemonic names: its kind and condition (0 = always,
/// 1 = subroutine for `bra`/`bsr`; for DBcc 0 = true, 1 = false).
fn branch_kind(m: &str) -> Option<(BranchKind, u8)> {
    Some(match m {
        "jra" | "jbra" => (BranchKind::J, 0),
        "jbsr" => (BranchKind::J, 1),
        "bra" => (BranchKind::B, 0),
        "bsr" => (BranchKind::B, 1),
        "dbra" | "dbf" => (BranchKind::Db(0), 1),
        "dbt" => (BranchKind::Db(0), 0),
        _ => {
            if let Some(c) = m.strip_prefix("db") {
                (BranchKind::Db(0), condition(c)?)
            } else if let Some(c) = m.strip_prefix('j') {
                (BranchKind::J, condition(c)?)
            } else {
                (BranchKind::B, condition(m.strip_prefix('b')?)?)
            }
        }
    })
}

fn branch_size(kind: BranchKind, cc: u8, reach: Reach) -> u32 {
    match (reach, kind) {
        (Reach::Byte, _) => 2,
        (Reach::Word, _) => 4,
        (Reach::Abs, BranchKind::Db(_)) => 12,
        (Reach::Abs, _) if cc <= 1 => 6,
        (Reach::Abs, _) => 8,
    }
}

fn branch_words(
    kind: BranchKind,
    cc: u8,
    reach: Reach,
    pc: u32,
    target: i64,
) -> Result<Vec<u16>, String> {
    let disp = target - i64::from(pc) - 2;
    let op = |cc: u8| 0x6000 | (u16::from(cc) << 8);
    let abs = |op: u16| vec![op, (target >> 16) as u16, target as u16];
    Ok(match (reach, kind) {
        (Reach::Byte, _) => {
            if !(-128..=127).contains(&disp) || disp == 0 {
                return Err(format!("short branch displacement {disp} out of range"));
            }
            vec![op(cc) | (disp as u8 as u16)]
        }
        (Reach::Word, _) if !(-32768..=32767).contains(&disp) => {
            return Err(format!("branch displacement {disp} does not fit 16 bits"));
        }
        (Reach::Word, BranchKind::Db(r)) => {
            vec![0x50C8 | (u16::from(cc) << 8) | u16::from(r), disp as u16]
        }
        (Reach::Word, _) => vec![op(cc), disp as u16],
        (Reach::Abs, BranchKind::Db(r)) => {
            let mut w = vec![0x50C8 | (u16::from(cc) << 8) | u16::from(r), 4, 0x6006];
            w.extend(abs(0x4EF9));
            w
        }
        (Reach::Abs, _) if cc == 0 => abs(0x4EF9),
        (Reach::Abs, _) if cc == 1 => abs(0x4EB9),
        (Reach::Abs, _) => {
            let mut w = vec![op(cc ^ 1) | 6];
            w.extend(abs(0x4EF9));
            w
        }
    })
}

/// The encoder mnemonic GNU as picks for an instruction, which for an
/// immediate source is the first matching entry of its opcode table:
/// `addq`/`subq` for 1..8, `moveq` for a long in -128..127, the `I` and
/// `A` forms otherwise.
fn gas_choice(insn: &Insn) -> Result<(String, Option<char>), String> {
    let quick = |lo: i64, hi: i64| match insn.ops.first() {
        Some(Op::Imm(e)) => e.const_value().is_some_and(|v| (lo..=hi).contains(&v)),
        _ => false,
    };
    let imm = matches!(insn.ops.first(), Some(Op::Imm(_)));
    let dst = insn.ops.get(1);
    let to_areg = matches!(dst, Some(Op::AReg(_)));
    let m = insn.mnemonic.as_str();
    let chosen = match m {
        "add" | "sub" if imm && quick(1, 8) => format!("{m}q"),
        "add" | "sub" | "cmp" if to_areg => format!("{m}a"),
        "add" | "sub" | "and" | "or" | "eor" | "cmp" if imm => format!("{m}i"),
        "cmp" if matches!(insn.ops.as_slice(), [Op::PostInc(_), Op::PostInc(_)]) => "cmpm".into(),
        "move" if to_areg && !matches!(insn.ops.first(), Some(Op::Special(_))) => "movea".into(),
        "move"
            if insn.size == Some('l') && quick(-128, 127) && matches!(dst, Some(Op::DReg(_))) =>
        {
            return Ok(("MOVEQ".into(), None));
        }
        _ => m.to_string(),
    };
    Ok((chosen.to_ascii_uppercase(), insn.size))
}

pub fn link(files: Vec<FileModel>, cpu: &str, origin: u32) -> Result<GasImage, Vec<GasError>> {
    let mut secs = Vec::new();
    let mut files = files;
    for (f, file) in files.iter_mut().enumerate() {
        for model in std::mem::take(&mut file.sections) {
            secs.push(Sec {
                file: f,
                model,
                forms: Vec::new(),
                sizes: Vec::new(),
                offs: Vec::new(),
                size: 0,
                strings: Vec::new(),
                merged: None,
                holder: 0,
            });
        }
    }
    let mut l = Linker {
        locals: vec![HashMap::new(); files.len()],
        files,
        secs,
        syms: Vec::new(),
        globals: HashMap::new(),
        commons: Vec::new(),
        cpu: cpu.to_string(),
        errors: Vec::new(),
        failed: Default::default(),
    };
    l.define_symbols();
    l.classify();
    for si in 0..l.secs.len() {
        let mut rounds = 0;
        while l.relax_pass(si) {
            rounds += 1;
            if rounds > 1000 {
                let file = l.secs[si].file;
                l.error(
                    file,
                    0,
                    format!("relaxation of {} does not settle", l.secs[si].model.name),
                );
                return Err(l.errors);
            }
        }
    }
    l.merge_strings();

    let table_item = l
        .secs
        .iter()
        .enumerate()
        .flat_map(|(si, s)| {
            s.model
                .items
                .iter()
                .enumerate()
                .filter(|(_, i)| matches!(i.0, Item::RelocTable))
                .map(move |(ii, i)| (si, ii, i.1))
        })
        .collect::<Vec<_>>();
    if table_item.len() > 1 {
        let (si, _, line) = table_item[1];
        let file = l.secs[si].file;
        l.error(file, line, "only one .reloc_table");
        return Err(l.errors);
    }
    let table_at = table_item.first().map(|&(si, ii, _)| (si, ii));

    let mut table: Vec<u8> = Vec::new();
    for _ in 0..4 {
        if let Some((si, ii)) = table_at {
            l.secs[si].sizes[ii] = table.len() as u32;
            l.offsets(si);
        }
        let (image, locs) = l.encode_image(origin, &table);
        if !l.errors.is_empty() {
            l.errors
                .sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
            return Err(l.errors);
        }
        let (moved, _) = l.encode_image(origin.wrapping_add(PROBE), &table);
        if !l.errors.is_empty() {
            return Err(l.errors);
        }
        let loc_of = |at: usize| {
            let i = locs
                .partition_point(|l| l.0 as usize <= at)
                .saturating_sub(1);
            locs.get(i).map_or((0, 0), |l| (l.1, l.2))
        };
        let mut fixups = Vec::new();
        let mut k = 0;
        let mut reported = std::collections::HashSet::new();
        while k < image.len() {
            let word = |b: &[u8]| {
                b.get(k..k + 4)
                    .map(|s| u32::from_be_bytes(s.try_into().unwrap()))
            };
            if let (Some(a), Some(b)) = (word(&image), word(&moved))
                && b.wrapping_sub(a) == PROBE
            {
                if !k.is_multiple_of(2) {
                    let (f, line) = loc_of(k);
                    l.error(
                        f,
                        line,
                        format!(
                            "an address at the odd offset ${k:X}: a fixup must be at an even offset"
                        ),
                    );
                }
                fixups.push(k as u32);
                k += 4;
                continue;
            }
            if image[k] != moved[k] && reported.insert(loc_of(k)) {
                let (f, line) = loc_of(k);
                l.error(
                    f,
                    line,
                    "a value depends on the image's address in a way a 32-bit fixup cannot express",
                );
            }
            k += 1;
        }
        if table_at.is_some() && fixups.first() == Some(&0) {
            l.error(
                loc_of(0).0,
                loc_of(0).1,
                "an address at offset 0 cannot be in the fixup table",
            );
        }
        if !l.errors.is_empty() {
            return Err(l.errors);
        }
        let layout = l.layout(origin);
        if let Some((si, ii)) = table_at {
            let new = crate::atari_prg_writer::fixup_table(&fixups);
            let end = layout.base[si] + l.secs[si].offs[ii] + table.len() as u32;
            if end != layout.linker[0] {
                let file = l.secs[si].file;
                let line = l.secs[si].model.items[ii].1;
                l.error(file, line, ".reloc_table must be the last bytes of the image (the data section of the file given last)");
                return Err(l.errors);
            }
            if new != table {
                table = new;
                continue;
            }
        }
        return Ok(GasImage {
            image,
            fixups,
            image_end: layout.linker[0],
            bss_start: layout.linker[1],
            bss_end: layout.linker[2],
        });
    }
    l.error(0, 0, "the relocation table does not settle");
    Err(l.errors)
}
