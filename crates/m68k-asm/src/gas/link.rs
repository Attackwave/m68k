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
//! address moved by [`Linker::probe_step`]: a longword that moves by exactly that much
//! holds an address of the image.

use std::collections::HashMap;

use m68k_core::operands::{BitfieldSpec, MemoryIndirectOperand, Operand};

use super::expr::{Expr, Linear};
use super::parse::{Class, FileModel, Insn, Item, Op, SectionModel};
use super::{GasError, GasImage, GasSection, GasSymbol, MAX_IMAGE};
use crate::encoder::encode_instruction;

/// How far the probe encoding moves the image when no alignment is
/// coarser than 4 KiB: a multiple of it that changes both words of a
/// longword.
const PROBE: u32 = 0x0001_1000;

/// The highest address a program may end at: the last 256 bytes of the
/// address space stay free, since the encoder works with addresses past
/// the instruction it encodes.
const TOP: u64 = 0xFFFF_FF00;

/// The symbols the link defines.
const LINKER_SYMBOLS: [&str; 7] = [
    "__image_end",
    "__bss_start",
    "__bss_end",
    "__init_array_start",
    "__init_array_end",
    "__fini_array_start",
    "__fini_array_end",
];

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
    /// `.set`: the value of an expression, in [`Linker::aliases`].
    Alias(usize),
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
    /// 68020+: a 32-bit displacement (`bra.l`, `bsr.l`, `bcc.l`), behind
    /// a DBcc that skips over it.
    Long,
    /// 68000/68010: `jmp`/`jsr` absolute long, around an inverted branch
    /// for a condition, or behind a DBcc that skips over it.
    Abs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchKind {
    /// `jra`, `jbsr`, `jXX`: relaxed up to a 32-bit branch (68020+) or an
    /// absolute jump.
    J,
    /// `bra`, `bsr`, `bXX`: byte or word, nothing further.
    B,
    /// `dbXX %dn,target`.
    Db(u8),
    /// `fjXX`: an FPU branch, word or 32-bit as needed.
    FJ,
    /// `fbXX`: an FPU branch, word only unless a size is written.
    FB,
}

#[derive(Debug, Clone, Copy)]
enum Form {
    Fixed,
    Align,
    Table,
    /// Long jumps for `.word a-b` that overflow, in [`Linker::stubs`].
    Stub(usize),
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

/// What GNU as does for `.word a-b` that does not fit 16 bits (its
/// "broken words"): at the first label after the words, a `bra.w` over
/// one long jump per overflowing target, and the word holds the distance
/// to that jump instead.
struct Stub {
    /// Where the jumps go: section and item.
    at: (usize, usize),
    /// The words, newest first as GNU as lists them: section and item.
    words: Vec<(usize, usize)>,
    /// Per word, the labels `a` and `b` of `a-b+n` in one section, and `n`.
    parts: Vec<Option<(usize, usize, usize, i64)>>,
    /// Per word: 0 fits, 1 has a jump, 2 uses the jump of word `jump`.
    added: Vec<u8>,
    jump: Vec<usize>,
}

impl Stub {
    fn size(&self) -> u32 {
        match self.added.iter().filter(|&&a| a == 1).count() {
            0 => 0,
            n => 4 + 6 * n as u32,
        }
    }
}

struct Linker {
    files: Vec<FileModel>,
    secs: Vec<Sec>,
    syms: Vec<Sym>,
    globals: HashMap<String, SymId>,
    locals: Vec<HashMap<String, SymId>>,
    /// Global commons: `(symbol, size, align)`.
    commons: Vec<(SymId, u32, u32)>,
    /// `.set` expressions with the file they are read in.
    aliases: Vec<(usize, Expr)>,
    /// Global symbols whose definition is weak: another one replaces it.
    weak_defs: std::collections::HashSet<SymId>,
    stubs: Vec<Stub>,
    /// A word of a stub: the stub and the word's place in it.
    word_stub: HashMap<(usize, usize), (usize, usize)>,
    /// The first section of each file.
    sec_base: Vec<usize>,
    cpu: String,
    errors: Vec<GasError>,
    /// Items already reported as wrong, not encoded again.
    failed: std::collections::HashSet<(usize, usize)>,
}

/// Addresses of one layout. They wrap at 4 GiB, which only the probe
/// layout may reach: `top` says whether the real one fits.
struct Layout {
    base: Vec<u32>,
    common: Vec<u32>,
    linker: [u32; 7],
    /// The end of BSS without wrapping.
    top: u64,
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
        for f in 0..self.files.len() {
            for (name, expr, line) in std::mem::take(&mut self.files[f].sets) {
                pending.push((f, line, name, Def::Alias(self.aliases.len())));
                self.aliases.push((f, expr));
            }
        }
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
            let weak = global && self.files[file].weak.contains(&name);
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
                if self.syms[id].def.is_some() && weak {
                    // A weak definition yields to one there already is.
                    continue;
                }
                if self.syms[id].def.is_some() && self.weak_defs.remove(&id) {
                    self.syms[id].def = Some(def);
                    continue;
                }
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
            if weak {
                self.weak_defs.insert(id);
            }
            if global {
                self.globals.insert(name, id);
            } else {
                self.locals[file].insert(name, id);
            }
        }
        // A weak symbol nothing defines is 0, as ld makes it.
        for f in 0..self.files.len() {
            let mut names: Vec<String> = self.files[f].weak.iter().cloned().collect();
            names.sort();
            for name in names {
                if self.globals.contains_key(&name) {
                    continue;
                }
                let id = self.syms.len();
                self.syms.push(Sym {
                    name: name.clone(),
                    def: Some(Def::Alias(self.aliases.len())),
                });
                self.aliases.push((f, Expr::Num(0)));
                self.globals.insert(name, id);
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
                        Some(Def::Item { sec, .. }) if self.secs[sec].file == f => self.error(
                            f,
                            line,
                            format!("'{name}' is defined in this file and also .comm"),
                        ),
                        // A definition in another file wins, as with ld.
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
        self.item_behind(sec, id, addend, 0)
    }

    /// The label of section `sec` that symbol `id` (plus `addend`) is,
    /// looking through local `.set` aliases as GNU as does: GCC aliases a
    /// function to an identical one (`-fipa-icf`).
    fn item_behind(&self, sec: usize, id: SymId, addend: i64, depth: u32) -> Option<(SymId, i64)> {
        match self.syms[id].def? {
            Def::Item { sec: s, .. } if s == sec => Some((id, addend)),
            Def::Alias(a) if depth < 64 => {
                let (file, expr) = &self.aliases[a];
                let (name, more) = expr.symbol_plus_const()?;
                let inner = self.lookup(*file, name).ok()?;
                self.item_behind(sec, inner, addend.checked_add(more)?, depth + 1)
            }
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
                    Item::WordStub(words) => {
                        let idx = self.new_stub(si, ii, words);
                        (Form::Stub(idx), 0)
                    }
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

    /// How a branch reaches what a word displacement does not.
    fn far_reach(&self) -> Reach {
        if matches!(self.cpu.as_str(), "68000" | "68010") {
            Reach::Abs
        } else {
            Reach::Long
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
            let far = self.far_reach();
            let (reach, fixed) = match (kind, insn.size) {
                (BranchKind::J | BranchKind::FJ | BranchKind::Db(_), Some(_)) => {
                    return Err(format!("{} takes no size suffix", insn.mnemonic));
                }
                (BranchKind::FB, Some('w')) | (BranchKind::FB, None) => (Reach::Word, true),
                (BranchKind::FB, Some('l')) => (Reach::Long, true),
                (BranchKind::FB, Some(_)) => return Err("bad size for an FPU branch".into()),
                (BranchKind::FJ, None) if local.is_none() => (Reach::Long, true),
                (BranchKind::FJ, None) => (Reach::Word, false),
                (BranchKind::B, Some('s' | 'b')) => (Reach::Byte, true),
                (BranchKind::B, Some('w')) => (Reach::Word, true),
                (BranchKind::B, Some('l')) if far == Reach::Long => (Reach::Long, true),
                (BranchKind::B, Some(_)) => return Err("a 32-bit branch needs a 68020".into()),
                (BranchKind::B, None) if local.is_none() => (Reach::Word, true),
                (BranchKind::J, None) if local.is_none() => (far, true),
                (BranchKind::Db(_), None) if local.is_none() => (far, true),
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
        let file = self.secs[sec].file;
        let sizing = |op: Option<(usize, bool)>| -> Result<u32, String> {
            let words =
                self.encode_insn(insn, 0, &mut |e: &Expr| self.sizing_value(file, e, 0), op)?;
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
                        && !matches!(disp, 0 | -1)
                        && !matches!(kind, BranchKind::Db(_) | BranchKind::FJ)
                    {
                        Reach::Byte
                    } else if (-32768..=32767).contains(&disp) {
                        Reach::Word
                    } else if kind == BranchKind::B {
                        // Out of reach for `bra`: the encoder reports it.
                        Reach::Word
                    } else if kind == BranchKind::FJ {
                        Reach::Long
                    } else {
                        self.far_reach()
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
                    let file = self.secs[si].file;
                    let mut at = |e: &Expr| {
                        if std::ptr::eq(e, cand) {
                            Ok(Val {
                                value: target,
                                weight: 1,
                            })
                        } else {
                            self.sizing_value(file, e, 0)
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
                Form::Stub(idx) => {
                    self.relax_stub(idx);
                    let size = self.stubs[idx].size();
                    if size != self.secs[si].sizes[ii] {
                        self.secs[si].sizes[ii] = size;
                        grew = true;
                    }
                }
                _ => {}
            }
        }
        grew
    }

    fn new_stub(&mut self, si: usize, ii: usize, words: &[(usize, usize)]) -> usize {
        let file = self.secs[si].file;
        let base = self.sec_base[file];
        let words: Vec<(usize, usize)> = words.iter().rev().map(|&(s, i)| (base + s, i)).collect();
        let parts = words.iter().map(|&w| self.word_parts(w)).collect();
        let idx = self.stubs.len();
        for (k, &w) in words.iter().enumerate() {
            self.word_stub.insert(w, (idx, k));
        }
        self.stubs.push(Stub {
            at: (si, ii),
            added: vec![0; words.len()],
            jump: vec![0; words.len()],
            words,
            parts,
        });
        idx
    }

    /// `a-b+n` of a word, when `a` and `b` are labels of one section.
    fn word_parts(&self, (si, ii): (usize, usize)) -> Option<(usize, usize, usize, i64)> {
        // A dropped COMDAT copy has no items left.
        let Some((Item::Data { expr, .. }, _)) = self.secs[si].model.items.get(ii) else {
            return None;
        };
        let file = self.secs[si].file;
        let lin: Linear<SymId> = expr.eval(&mut |name| self.lookup(file, name), None).ok()?;
        let item = |c: i64| {
            let id = lin.terms.iter().find(|t| t.1 == c)?.0;
            match self.syms[id].def? {
                Def::Item { sec, item } => Some((sec, item)),
                _ => None,
            }
        };
        let ((sa, a), (sb, b)) = (item(1)?, item(-1)?);
        (lin.terms.len() == 2 && sa == sb).then_some((sa, a, b, lin.k))
    }

    /// One relaxation round of a stub, as GNU as does it: a word that
    /// overflows gets a jump, shared by every word with the same target.
    fn relax_stub(&mut self, idx: usize) {
        let target = |l: &Linker, p: (usize, usize, usize, i64)| (p.0, l.secs[p.0].offs[p.1]);
        for k in 0..self.stubs[idx].words.len() {
            let stub = &self.stubs[idx];
            let Some(p) = stub.parts[k] else { continue };
            if stub.added[k] != 0 {
                continue;
            }
            let offs = &self.secs[p.0].offs;
            let off = i64::from(offs[p.1]) + p.3 - i64::from(offs[p.2]);
            if off > -32768 && off < 32767 {
                continue;
            }
            let t = target(self, p);
            let same: Vec<usize> = (0..stub.words.len())
                .filter(|&j| stub.parts[j].is_some_and(|q| target(self, q) == t))
                .collect();
            let stub = &mut self.stubs[idx];
            for j in same {
                stub.added[j] = 2;
                stub.jump[j] = k;
            }
            stub.added[k] = 1;
        }
    }

    /// How far the probe encoding moves the image: a multiple of every
    /// alignment, so padding stays the same, that changes both 16-bit
    /// halves of an address while the alignments allow it (below 64 KiB).
    fn probe_step(&self) -> u32 {
        let align = self
            .secs
            .iter()
            .map(|s| s.model.align)
            .chain(self.commons.iter().map(|c| c.2))
            .max()
            .unwrap_or(1);
        match align {
            0..=0x1000 => PROBE,
            0x1001..=0xFFFF => 0x1_0000 + align,
            _ => align,
        }
    }

    /// The address of the jump word `k` of stub `idx` uses.
    fn jump_address(&self, layout: &Layout, idx: usize, k: usize) -> u32 {
        let stub = &self.stubs[idx];
        let (si, ii) = stub.at;
        let before = (0..stub.jump[k]).filter(|&j| stub.added[j] == 1).count() as u32;
        layout.base[si]
            .wrapping_add(self.secs[si].offs[ii])
            .wrapping_add(4 + 6 * before)
    }

    /// Item offsets within section `si` from the current sizes.
    fn offsets(&mut self, si: usize) {
        let sec = &mut self.secs[si];
        let mut off = 0u32;
        sec.offs.clear();
        for (ii, form) in sec.forms.iter().enumerate() {
            sec.offs.push(off);
            if let (Form::Align, Item::Align { align, max, .. }) = (form, &sec.model.items[ii].0) {
                let pad = off.checked_next_multiple_of(*align).unwrap_or(u32::MAX) - off;
                sec.sizes[ii] = if max.is_some_and(|m| pad > m) { 0 } else { pad };
            }
            // Saturates beyond 4 GiB; `check_size` refuses such a section.
            off = off.saturating_add(sec.sizes[ii]);
        }
        sec.size = off;
    }

    /// Shares the strings of every mergeable section, as `ld` does:
    /// each distinct string once, a string that ends another one inside
    /// it, the block in the first such section.
    fn merge_strings(&mut self) {
        let mut groups: HashMap<(u8, u32), Vec<usize>> = HashMap::new();
        for &si in &self.link_order() {
            if let Some(size) = self.secs[si].model.merge_strings {
                groups
                    .entry((size, self.secs[si].model.align))
                    .or_default()
                    .push(si);
            }
        }
        for ((size, _), members) in groups {
            let size = usize::from(size);
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
                if bytes.len() % size != 0
                    || bytes
                        .rchunks(size)
                        .next()
                        .is_some_and(|c| c.iter().any(|&b| b != 0))
                {
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
                for (k, c) in bytes.chunks_exact(size).enumerate() {
                    if c.iter().all(|&b| b == 0) {
                        let i = k * size;
                        let s = bytes[start..i].to_vec();
                        let n = unique.len();
                        let e = *index.entry(s.clone()).or_insert_with(|| {
                            unique.push(s);
                            n
                        });
                        strings.push((start as u32, (i + size - start) as u32, e));
                        start = i + size;
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
                    block.resize(block.len() + size, 0);
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
        if sec.model.merge_strings.is_none() {
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
        if sec.model.merge_strings.is_some() {
            sec.merged.as_ref().map_or(0, |b| b.len() as u32)
        } else {
            sec.size
        }
    }

    /// Stored sections in image order: by class, then file, then the order
    /// of the sections in the file. Constructor and destructor arrays go
    /// by priority first, as `ld` sorts them: `.init_array.N` by `N`, the
    /// plain one last.
    fn link_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.secs.len())
            .filter(|&s| self.secs[s].model.class != Class::Bss)
            .collect();
        order.sort_by_key(|&s| {
            let m = &self.secs[s].model;
            let prio = match m.class {
                Class::InitArray | Class::FiniArray => m
                    .name
                    .rsplit_once('.')
                    .and_then(|(_, n)| n.parse::<u32>().ok())
                    .map_or(u64::MAX, u64::from),
                _ => 0,
            };
            (m.class, prio, self.secs[s].file)
        });
        order
    }

    /// Whether the image stays below [`MAX_IMAGE`] before strings are
    /// merged (merging only shrinks it), which bounds what the link holds
    /// in memory.
    fn check_size(&mut self) -> Result<(), ()> {
        let stored: u64 = self
            .link_order()
            .iter()
            .map(|&si| u64::from(self.secs[si].size) + u64::from(self.secs[si].model.align))
            .sum();
        if stored > u64::from(MAX_IMAGE) {
            self.error(0, 0, format!("the image is larger than {MAX_IMAGE} bytes"));
            return Err(());
        }
        Ok(())
    }

    fn layout(&self, origin: u32) -> Layout {
        let mut base = vec![0u32; self.secs.len()];
        let mut dot = u64::from(origin);
        let order = self.link_order();
        // [init start, init end, fini start, fini end]
        let mut arrays = [0u32; 4];
        for class in [
            Class::Text,
            Class::Rodata,
            Class::InitArray,
            Class::FiniArray,
            Class::Data,
        ] {
            let members: Vec<usize> = order
                .iter()
                .copied()
                .filter(|&si| self.secs[si].model.class == class)
                .collect();
            let array = match class {
                Class::InitArray => Some(0),
                Class::FiniArray => Some(2),
                _ => None,
            };
            if let Some(k) = array {
                // As an output section of its own: it starts at the largest
                // alignment of its sections, so the start symbol is never
                // in front of padding.
                let align = members.iter().map(|&si| self.secs[si].model.align).max();
                dot = dot.next_multiple_of(u64::from(align.unwrap_or(1)));
                arrays[k] = dot as u32;
            }
            for si in members {
                dot = dot.next_multiple_of(u64::from(self.secs[si].model.align));
                base[si] = dot as u32;
                dot += u64::from(self.stored_size(si));
            }
            if let Some(k) = array {
                arrays[k + 1] = dot as u32;
            }
        }
        let image_end = dot as u32;
        let bss: Vec<usize> = (0..self.secs.len())
            .filter(|&s| self.secs[s].model.class == Class::Bss)
            .collect();
        let align = bss
            .iter()
            .map(|&s| self.secs[s].model.align)
            .chain(self.commons.iter().map(|c| c.2))
            .max()
            .unwrap_or(1);
        dot = dot.next_multiple_of(u64::from(align));
        for si in bss {
            dot = dot.next_multiple_of(u64::from(self.secs[si].model.align));
            base[si] = dot as u32;
            dot += u64::from(self.secs[si].size);
        }
        let mut common = Vec::new();
        for &(_, size, align) in &self.commons {
            dot = dot.next_multiple_of(u64::from(align));
            common.push(dot as u32);
            dot += u64::from(size);
        }
        let [is, ie, fs, fe] = arrays;
        Layout {
            base,
            common,
            linker: [image_end, image_end, dot as u32, is, ie, fs, fe],
            top: dot,
        }
    }

    /// The program's symbols at their final addresses: no `.L…` (GCC's
    /// own labels, numeric locals) and none that is only referenced.
    fn symbol_list(&self, layout: &Layout) -> Vec<GasSymbol> {
        let mut owner = vec![None; self.syms.len()];
        for (f, locals) in self.locals.iter().enumerate() {
            for &id in locals.values() {
                owner[id] = Some(f);
            }
        }
        // The array bounds only for a program with such an array.
        let arrays = self.secs.iter().any(|s| {
            matches!(s.model.class, Class::InitArray | Class::FiniArray)
                && !s.model.items.is_empty()
        });
        self.syms
            .iter()
            .enumerate()
            .filter(|(_, sym)| !sym.name.starts_with(".L"))
            .filter(|(_, sym)| arrays || !matches!(sym.def, Some(Def::Linker(3..))))
            .filter_map(|(id, sym)| {
                let address = self.address(layout, sym.def?).ok()?;
                Some(GasSymbol {
                    name: sym.name.clone(),
                    file: owner[id].map(|f| self.files[f].name.clone()),
                    address,
                })
            })
            .collect()
    }

    /// The non-empty input sections at their final place, by address.
    fn section_list(&self, layout: &Layout) -> Vec<GasSection> {
        let mut out: Vec<GasSection> = (0..self.secs.len())
            .filter_map(|si| {
                let sec = &self.secs[si];
                let size = if sec.model.class == Class::Bss {
                    sec.size
                } else {
                    self.stored_size(si)
                };
                (size > 0).then(|| GasSection {
                    file: Some(self.files[sec.file].name.clone()),
                    name: sec.model.name.clone(),
                    start: layout.base[si],
                    size,
                })
            })
            .collect();
        if let Some(&start) = layout.common.first() {
            out.push(GasSection {
                file: None,
                name: "COMMON".into(),
                start,
                size: layout.linker[2] - start,
            });
        }
        out.sort_by_key(|s| s.start);
        out
    }

    fn address(&self, layout: &Layout, def: Def) -> Result<u32, String> {
        Ok(match def {
            Def::Item { sec, item } if self.secs[sec].model.merge_strings.is_some() => layout.base
                [self.secs[sec].holder]
                .wrapping_add(self.mapped(sec, self.secs[sec].offs[item])?),
            Def::Item { sec, item } => layout.base[sec].wrapping_add(self.secs[sec].offs[item]),
            Def::Common(c) => layout.common[c],
            Def::Linker(i) => layout.linker[i],
            Def::Alias(a) => {
                let v = self.alias_value(layout, a, 0)?;
                if !matches!(v.weight, 0 | 1) {
                    return Err("expression is neither a number nor an address".into());
                }
                long32(v.value)? as u32
            }
        })
    }

    /// For sizing an instruction: every address 0 with the weight as
    /// written; a `.set` symbol stands for its expression, so a constant
    /// keeps its value.
    fn sizing_value(&self, file: usize, e: &Expr, depth: u32) -> Result<Val, String> {
        if depth > 64 {
            return Err("a .set symbol is defined in terms of itself".into());
        }
        // An undefined name sizes as an address; encoding reports it.
        let lin: Linear<Option<SymId>> =
            e.eval(&mut |name| Ok(self.lookup(file, name).ok()), Some(None))?;
        let overflow = || "overflow in expression".to_string();
        let mut v = Val {
            value: lin.k,
            weight: 0,
        };
        for (id, c) in lin.terms {
            let term = match id.map(|id| self.syms[id].def) {
                Some(Some(Def::Alias(a))) => {
                    let (f, expr) = &self.aliases[a];
                    self.sizing_value(*f, expr, depth + 1)?
                }
                _ => Val {
                    value: 0,
                    weight: 1,
                },
            };
            v.weight = c
                .checked_mul(term.weight)
                .and_then(|w| w.checked_add(v.weight))
                .ok_or_else(overflow)?;
            v.value = c
                .checked_mul(term.value)
                .and_then(|x| x.checked_add(v.value))
                .ok_or_else(overflow)?;
        }
        Ok(v)
    }

    /// The value of the `.set` expression `a`; `depth` counts the aliases
    /// it is reached through.
    fn alias_value(&self, layout: &Layout, a: usize, depth: u32) -> Result<Val, String> {
        if depth > 64 {
            return Err("a .set symbol is defined in terms of itself".into());
        }
        let (file, expr) = &self.aliases[a];
        self.value_at(layout, *file, None, expr, depth + 1)
    }

    /// The value of `expr` in file `file` with `dot` the location of the
    /// item. A difference of two symbols of one section counts their
    /// distance in the section as written, as GNU as resolves it.
    fn value(&self, layout: &Layout, file: usize, dot: Def, expr: &Expr) -> Result<Val, String> {
        self.value_at(layout, file, Some(dot), expr, 0)
    }

    fn value_at(
        &self,
        layout: &Layout,
        file: usize,
        dot: Option<Def>,
        expr: &Expr,
        depth: u32,
    ) -> Result<Val, String> {
        let lin: Linear<SymId> =
            expr.eval(&mut |name| self.lookup(file, name), dot.map(|_| DOT))?;
        let mut value = lin.k;
        let mut weight = 0i64;
        let def_of = |id: SymId| match dot {
            Some(d) if id == DOT => Ok(d),
            _ => self.sym_def(id),
        };
        let overflow = || "overflow in expression".to_string();
        let mut by_sec: HashMap<usize, i64> = HashMap::new();
        for &(id, c) in &lin.terms {
            if let Def::Item { sec, .. } = def_of(id)? {
                let sum = by_sec.entry(sec).or_default();
                *sum = sum.checked_add(c).ok_or_else(overflow)?;
            }
        }
        for &(id, c) in &lin.terms {
            let def = def_of(id)?;
            if let Def::Alias(a) = def {
                let v = self.alias_value(layout, a, depth)?;
                weight = c
                    .checked_mul(v.weight)
                    .and_then(|w| w.checked_add(weight))
                    .ok_or_else(overflow)?;
                value = c
                    .checked_mul(v.value)
                    .and_then(|v| v.checked_add(value))
                    .ok_or_else(overflow)?;
                continue;
            }
            weight = c.checked_add(weight).ok_or_else(overflow)?;
            let addr = match def {
                Def::Item { sec, item } if by_sec[&sec] == 0 => {
                    i64::from(self.secs[sec].offs[item])
                }
                _ => i64::from(self.address(layout, def)?),
            };
            value = c
                .checked_mul(addr)
                .and_then(|v| v.checked_add(value))
                .ok_or_else(overflow)?;
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
        let (words, numeric_pc) = self.encode_insn_at(insn, pc, ev, pcrel, 0)?;
        if !numeric_pc {
            return Ok(words);
        }
        // A numeric `d(%pc)` counts from its extension word, which is not
        // always the word after the opcode: `mulu.l`, FPU and bitfield
        // instructions put one of their own before it. Moving the target
        // by 2 changes only the word that holds the displacement.
        let moved = match self.encode_insn_at(insn, pc, ev, pcrel, 2) {
            Ok((w, _)) => w,
            Err(_) => self.encode_insn_at(insn, pc, ev, pcrel, -2)?.0,
        };
        match words.iter().zip(&moved).position(|(a, b)| a != b) {
            Some(k) if k > 1 => {
                let shift = 2 * (k as i64 - 1);
                Ok(self.encode_insn_at(insn, pc, ev, pcrel, shift)?.0)
            }
            _ => Ok(words),
        }
    }

    /// [`Self::encode_insn`] with a numeric PC displacement counted from
    /// `pc + 2 + shift`; also says whether there was one.
    fn encode_insn_at(
        &self,
        insn: &Insn,
        pc: u32,
        ev: &mut dyn FnMut(&Expr) -> Result<Val, String>,
        pcrel: Option<(usize, bool)>,
        shift: i64,
    ) -> Result<(Vec<u16>, bool), String> {
        let numeric_pc = std::cell::Cell::new(false);
        let (mnemonic, size) = gas_choice(insn)?;
        let fpu = mnemonic.starts_with('F');
        // The size of an FPU operand in memory or an immediate that holds
        // a floating-point bit pattern.
        // (FMOVEM's `.x` names the registers; its `#n` is their mask.)
        let float_size = size.filter(|s| fpu && matches!(s, 's' | 'd' | 'x' | 'p'));
        let float_imm = float_size.filter(|_| mnemonic != "FMOVEM");
        if let Some(s) = size
            && !matches!(s, 'b' | 'w' | 'l')
            && float_size.is_none()
        {
            return Err(format!("bad size .{s} for {}", insn.mnemonic));
        }
        let size_str = size.map(|c| c.to_string());
        let long_field = size == Some('l');
        let movem_predec = mnemonic == "MOVEM" && matches!(insn.ops.get(1), Some(Op::PreDec(_)));
        let mut ops = Vec::with_capacity(2);
        let full_format = !matches!(self.cpu.as_str(), "68000" | "68010");
        for (i, op) in insn.ops.iter().enumerate() {
            // A bitfield wraps its effective address; translated below,
            // then wrapped again.
            let (op, field) = match op {
                Op::Bitfield(ea, offset, width) => (&**ea, Some((offset, width))),
                _ => (op, None),
            };
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
                    0 if i == 0 => {
                        numeric_pc.set(true);
                        Ok((i64::from(pc) + 2 + shift + v.value) as i32)
                    }
                    0 => Err(
                        "a numeric PC displacement is supported in the first operand only".into(),
                    ),
                    _ => Err("expression is neither a number nor an address".into()),
                }
            };
            let operand = match op {
                Op::DReg(n) => Operand::DataReg(*n),
                Op::AReg(n) => Operand::AddrReg(*n),
                Op::Ind(n) => Operand::AddrRegIndirect(*n),
                Op::PostInc(n) => Operand::AddrRegPostInc(*n),
                Op::PreDec(n) => Operand::AddrRegPreDec(*n),
                Op::Special(s) => Operand::Special((*s).to_string()),
                Op::RegList(m) => Operand::Immediate(i64::from(*m)),
                Op::RegPair(h, l) => Operand::RegPair(*h, *l),
                Op::FpReg(n) => Operand::FpReg(*n),
                Op::FpCtrl(m) => Operand::FpCtrlList(*m),
                Op::FpRegList(m) => Operand::Immediate(i64::from(*m)),
                Op::ImmBits(bits) => match float_imm {
                    Some(s) => Operand::ImmediateWords(float_words(*bits, s)?),
                    None => return Err("a constant wider than 63 bits needs .d, .x or .p".into()),
                },
                Op::Imm(e) if float_imm.is_some() => {
                    let bits = e
                        .const_value()
                        .filter(|v| *v >= 0)
                        .ok_or("a floating-point immediate must be a bit pattern (#0x...)")?;
                    Operand::ImmediateWords(float_words(bits as u128, float_imm.unwrap_or('s'))?)
                }
                Op::Bitfield(..) => return Err("a bitfield inside a bitfield".into()),
                Op::Disp(e, n) => {
                    let v = ev(e)?;
                    if full_format && (v.weight != 0 || !(-32768..=32767).contains(&v.value)) {
                        full_ea(Some(*n), None, v)?
                    } else {
                        let d = narrow(v, "16-bit displacement")?;
                        if !(-32768..=32767).contains(&d) {
                            return Err(format!("displacement {d} does not fit 16 bits"));
                        }
                        Operand::AddrRegIndirectDisp(*n, d as i32, false)
                    }
                }
                Op::PcDisp(e) => Operand::PcRelativeDisp(pc_target(ev(e)?)?, false),
                Op::Index {
                    base: Some(n),
                    disp,
                    xn,
                    long,
                    scale,
                } => {
                    let v = ev(disp)?;
                    if full_format && (v.weight != 0 || !(-128..=127).contains(&v.value)) {
                        full_ea(Some(*n), Some((*xn, *long, *scale)), v)?
                    } else {
                        let d = narrow(v, "8-bit displacement")?;
                        let d = i8::try_from(d)
                            .map_err(|_| format!("displacement {d} does not fit 8 bits"))?;
                        Operand::AddrRegIndirectIndex(*n, *xn, d, *scale, *long)
                    }
                }
                Op::NoBaseIndex {
                    disp,
                    xn,
                    long,
                    scale,
                } => {
                    if !full_format {
                        return Err("an index without a base register needs a 68020".into());
                    }
                    full_ea(None, Some((*xn, *long, *scale)), ev(disp)?)?
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
                    if mnemonic == "FMOVEM" {
                        // As for MOVEM, the mask as it is encoded: the
                        // encoder reverses it for every mode but -(An).
                        let raw = u8::try_from(v)
                            .map_err(|_| format!("register mask {v} does not fit 8 bits"))?;
                        let predec = matches!(insn.ops.get(1), Some(Op::PreDec(_)));
                        Operand::Immediate(i64::from(if predec { raw } else { raw.reverse_bits() }))
                    } else if mnemonic == "MOVEM" {
                        // GNU as takes the mask as it is encoded; the
                        // encoder wants bit 0 = %d0 in every mode.
                        let raw = u16::try_from(v)
                            .map_err(|_| format!("register mask {v} does not fit 16 bits"))?;
                        Operand::Immediate(i64::from(if movem_predec {
                            raw.reverse_bits()
                        } else {
                            raw
                        }))
                    } else {
                        Operand::Immediate(v)
                    }
                }
            };
            ops.push(match field {
                Some((offset, width)) => Operand::Bitfield(
                    Box::new(operand),
                    Box::new(offset.clone()),
                    // GNU as reads a width of 0 as 32, which is how the
                    // field encodes it; GCC writes `{#0:#0}`.
                    Box::new(match width {
                        BitfieldSpec::Immediate(0) => BitfieldSpec::Immediate(32),
                        w => w.clone(),
                    }),
                ),
                None => operand,
            });
        }
        let mut words = match (mnemonic.as_str(), ops.as_slice()) {
            // `cas Dc,Du,<ea>`: the <ea> extension follows CAS's own word.
            ("CAS", [dc, du, ea]) => crate::enc_logic::enc_cas(
                dc,
                du,
                ea,
                size_str.as_deref().unwrap_or("w"),
                pc.wrapping_add(6),
                &self.cpu,
            ),
            (_, [_, _, _, ..]) => {
                return Err(format!("{} takes at most two operands", insn.mnemonic));
            }
            _ => encode_instruction(
                &mnemonic,
                size_str.as_deref(),
                ops.first(),
                ops.get(1),
                pc,
                &self.cpu,
            ),
        }
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
        // `muls.l <ea>,%dn` (32-bit product): the CPU ignores the Dh field
        // of the extension word; GNU as leaves it 0 where the Motorola
        // encoding repeats Dl.
        if matches!(mnemonic.as_str(), "MULS" | "MULU")
            && long_field
            && matches!(ops.get(1), Some(Operand::DataReg(_)))
        {
            words[1] &= !7;
        }
        Ok((words, numeric_pc.get()))
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
        let pc = layout.base[si].wrapping_add(sec.offs[ii]);
        let here = Def::Item { sec: si, item: ii };
        let mut ev = |e: &Expr| self.value(layout, file, here, e);
        let item = &sec.model.items[ii].0;
        Ok(match (item, sec.forms[ii]) {
            (Item::Label(_), _) => Vec::new(),
            (Item::Bytes(b), _) => b.clone(),
            (Item::Zero(n), _) => vec![0; *n as usize],
            (Item::RelocTable, _) => table.to_vec(),
            (Item::WordStub(_), Form::Stub(idx)) => {
                let stub = &self.stubs[idx];
                if stub.size() == 0 {
                    return Ok(Vec::new());
                }
                let n = (stub.size() - 4) / 6;
                let mut w = vec![0x6000u16, (2 + 6 * n) as u16];
                let long_branch = !matches!(self.cpu.as_str(), "68000" | "68010");
                let mut at = pc.wrapping_add(4);
                for k in (0..stub.words.len()).filter(|&k| stub.added[k] == 1) {
                    let (s, a, _, n) = stub.parts[k].expect("only words with parts get a jump");
                    let to = i64::from(self.address(layout, Def::Item { sec: s, item: a })?) + n;
                    let (op, v) = if long_branch {
                        (0x60FF, to - i64::from(at) - 2)
                    } else {
                        (0x4EF9, to)
                    };
                    w.extend([op, (v >> 16) as u16, v as u16]);
                    at = at.wrapping_add(6);
                }
                w.iter().flat_map(|x| x.to_be_bytes()).collect()
            }
            (Item::WordStub(_), _) => Vec::new(),
            (Item::Data { width: 2, .. }, _)
                if self
                    .word_stub
                    .get(&(si, ii))
                    .is_some_and(|&(idx, k)| self.stubs[idx].added[k] != 0) =>
            {
                // Redirected: the distance from `b` to the jump.
                let (idx, k) = self.word_stub[&(si, ii)];
                let (s, _, b, _) = self.stubs[idx].parts[k].expect("redirected words have parts");
                let to = i64::from(self.jump_address(layout, idx, k));
                let from = i64::from(self.address(layout, Def::Item { sec: s, item: b })?);
                ((to - from) as u16).to_be_bytes().to_vec()
            }
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
                        // A number is a target too: an undefined weak
                        // symbol is 0, as ld resolves it.
                        if !matches!(target.weight, 0 | 1) {
                            return Err("a branch needs a label as its target".into());
                        }
                        // To a number, a 32-bit displacement would not
                        // survive relocation; `jsr`/`jmp` absolute is as
                        // long and does.
                        let reach = match (reach, kind) {
                            (Reach::Long, BranchKind::J | BranchKind::B)
                                if target.weight == 0 && cc <= 1 =>
                            {
                                Reach::Abs
                            }
                            _ => reach,
                        };
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
        let mut image = vec![0u8; end.wrapping_sub(origin) as usize];
        let mut locs = Vec::new();
        let mut errors = Vec::new();
        for si in self.link_order() {
            let start = layout.base[si].wrapping_sub(origin) as usize;
            if let Some(block) = &self.secs[si].merged {
                image[start..start + block.len()].copy_from_slice(block);
                continue;
            }
            if self.secs[si].model.merge_strings.is_some() {
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

/// The extension words of a floating-point immediate: the bit pattern of
/// a single (32 bits), double (64) or extended/packed (96) value.
fn float_words(bits: u128, size: char) -> Result<Vec<u16>, String> {
    let width = match size {
        's' => 32,
        'd' => 64,
        _ => 96,
    };
    if bits >> width != 0 {
        return Err(format!(
            "constant does not fit the {width}-bit .{size} format"
        ));
    }
    Ok((0..width / 16)
        .rev()
        .map(|i| (bits >> (16 * i)) as u16)
        .collect())
}

/// A 68020 full-format effective address without memory indirection:
/// `(bd,An,Xn)`, `(bd,An)` with the index suppressed, `(bd,Xn)` with the
/// base suppressed. As GNU as does it, an address gets a 32-bit `bd`, a
/// number the smaller size that holds it, and a 0 without a base none.
fn full_ea(base: Option<u8>, index: Option<(u8, bool, u8)>, bd: Val) -> Result<Operand, String> {
    if !matches!(bd.weight, 0 | 1) {
        return Err("expression is neither a number nor an address".into());
    }
    let value = long32(bd.value)?;
    let null = base.is_none() && bd.weight == 0 && bd.value == 0;
    let long = bd.weight != 0 || !(-32768..=32767).contains(&bd.value);
    let (index_reg, index_long, index_scale) = match index {
        Some((xn, long, scale)) => (Some(xn), long, scale),
        None => (None, false, 1),
    };
    Ok(Operand::MemoryIndirect(Box::new(MemoryIndirectOperand {
        base_reg: base,
        base_is_pc: false,
        base_disp: (!null).then_some(value),
        index_reg,
        index_long,
        index_scale,
        outer_disp: None,
        is_postindexed: false,
        is_indirect: false,
        bd_forced: Some(long),
        od_forced: None,
        zpc: false,
    })))
}

fn long32(v: i64) -> Result<i32, String> {
    if (-0x8000_0000..=0xFFFF_FFFF).contains(&v) {
        Ok(v as u32 as i32)
    } else {
        Err(format!("value {v} does not fit 32 bits"))
    }
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
        _ if m.starts_with("fj") || m.starts_with("fb") => {
            let cc = crate::encoder::fpu_cc(&m[2..].to_ascii_uppercase())? as u8;
            let kind = if m.starts_with("fj") {
                BranchKind::FJ
            } else {
                BranchKind::FB
            };
            (kind, cc)
        }
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
        (Reach::Long | Reach::Abs, BranchKind::Db(_)) => 12,
        (Reach::Long, _) => 6,
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
            // 0 and -1 ($00, $FF) mark the word and 32-bit forms.
            if !(-128..=127).contains(&disp) || matches!(disp, 0 | -1) {
                return Err(format!("short branch displacement {disp} out of range"));
            }
            vec![op(cc) | (disp as u8 as u16)]
        }
        (Reach::Word, _) if !(-32768..=32767).contains(&disp) => {
            return Err(format!("branch displacement {disp} does not fit 16 bits"));
        }
        (Reach::Word, BranchKind::FJ | BranchKind::FB) => {
            vec![0xF280 | u16::from(cc), disp as u16]
        }
        (Reach::Long, BranchKind::FJ | BranchKind::FB) => {
            vec![0xF2C0 | u16::from(cc), (disp >> 16) as u16, disp as u16]
        }
        (Reach::Word, BranchKind::Db(r)) => {
            vec![0x50C8 | (u16::from(cc) << 8) | u16::from(r), disp as u16]
        }
        (Reach::Word, _) => vec![op(cc), disp as u16],
        (Reach::Long, BranchKind::Db(r)) => {
            // The `bra.l` behind `dbcc` and `bra.s` is 6 bytes further on.
            let d = (disp - 6) as u32;
            vec![
                0x50C8 | (u16::from(cc) << 8) | u16::from(r),
                4,
                0x6006,
                0x60FF,
                (d >> 16) as u16,
                d as u16,
            ]
        }
        (Reach::Long, _) => vec![op(cc) | 0xFF, (disp >> 16) as u16, disp as u16],
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
    let mut sec_base = Vec::with_capacity(files.len());
    // COMDAT groups already linked: a later file's copy is dropped, as
    // `ld` drops it. The section stays, empty, so indices hold.
    let mut groups = std::collections::HashSet::new();
    for (f, file) in files.iter_mut().enumerate() {
        sec_base.push(secs.len());
        let mut new_groups = Vec::new();
        for mut model in std::mem::take(&mut file.sections) {
            if let Some(g) = &model.group {
                if groups.contains(g) {
                    model.items.clear();
                    model.align = 1;
                } else {
                    new_groups.push(g.clone());
                }
            }
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
        groups.extend(new_groups);
    }
    let mut l = Linker {
        locals: vec![HashMap::new(); files.len()],
        files,
        secs,
        syms: Vec::new(),
        globals: HashMap::new(),
        commons: Vec::new(),
        aliases: Vec::new(),
        weak_defs: Default::default(),
        stubs: Vec::new(),
        word_stub: HashMap::new(),
        sec_base,
        cpu: cpu.to_string(),
        errors: Vec::new(),
        failed: Default::default(),
    };
    l.define_symbols();
    l.classify();
    // Before relaxation as well: it encodes at section offsets, which must
    // stay far from 4 GiB.
    for si in 0..l.secs.len() {
        l.offsets(si);
    }
    if l.check_size().is_err() {
        return Err(l.errors);
    }
    for si in 0..l.secs.len() {
        // Forms only grow, a branch at most twice, so every round but the
        // last grows one at least.
        let relaxable: usize = l.secs[si]
            .forms
            .iter()
            .map(|f| match f {
                Form::Branch { fixed: false, .. } | Form::PcRel { .. } => 1,
                Form::Stub(idx) => l.stubs[*idx].words.len(),
                _ => 0,
            })
            .sum();
        let mut rounds = 0;
        while l.relax_pass(si) {
            rounds += 1;
            if rounds > 2 * relaxable + 1 {
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
    if l.check_size().is_err() {
        return Err(l.errors);
    }
    l.merge_strings();
    let step = l.probe_step();
    let first = l.layout(origin);
    let (top, image_end) = (first.top, u64::from(first.linker[0]));
    if top > TOP {
        l.error(
            0,
            0,
            format!("the program ends at ${top:X}, beyond the 32-bit address space"),
        );
        return Err(l.errors);
    }

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
        // Moved down where moving up would pass `TOP` (only the stored
        // part is encoded).
        let probe = if image_end + u64::from(step) <= TOP {
            step
        } else if origin >= step {
            step.wrapping_neg()
        } else {
            l.error(
                0,
                0,
                "an alignment this coarse leaves no room to find the fixups at this origin",
            );
            return Err(l.errors);
        };
        let (moved, _) = l.encode_image(origin.wrapping_add(probe), &table);
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
                && b.wrapping_sub(a) == probe
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
            symbols: l.symbol_list(&layout),
            sections: l.section_list(&layout),
        });
    }
    l.error(0, 0, "the relocation table does not settle");
    Err(l.errors)
}
