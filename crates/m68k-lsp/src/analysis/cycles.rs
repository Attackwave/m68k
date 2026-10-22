//! MC68000 instruction timing, from the execution time tables of the
//! M68000 user's manual (section 8).
//!
//! Times are clock periods for a zero-wait-state bus: instruction fetch,
//! operand accesses and effective address calculation included. On an
//! Amiga, chip RAM accesses can be delayed by custom chip DMA, so these are
//! lower bounds there.

/// Addressing mode of one operand, as far as timing is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ea {
    Dn,
    An,
    Ind,
    PostInc,
    PreDec,
    Disp,
    Index,
    AbsW,
    AbsL,
    PcDisp,
    PcIndex,
    Imm,
}

impl Ea {
    fn is_reg(self) -> bool {
        matches!(self, Ea::Dn | Ea::An)
    }

    /// Effective address calculation time (table 8-1).
    fn calc(self, long: bool) -> u32 {
        let (bw, l) = match self {
            Ea::Dn | Ea::An => (0, 0),
            Ea::Ind | Ea::PostInc | Ea::Imm => (4, 8),
            Ea::PreDec => (6, 10),
            Ea::Disp | Ea::AbsW | Ea::PcDisp => (8, 12),
            Ea::Index | Ea::PcIndex => (10, 14),
            Ea::AbsL => (12, 16),
        };
        if long { l } else { bw }
    }
}

/// Timing of one instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timing {
    /// Fastest case.
    pub min: u32,
    /// Slowest case; `None` when it depends on a runtime value without a
    /// useful bound (a shift count held in a register).
    pub max: Option<u32>,
    /// Compact form for an inline hint, e.g. `12`, `10/8`, `38-70`.
    pub label: String,
    /// One-line explanation for hovers.
    pub detail: String,
}

impl Timing {
    fn fixed(c: u32) -> Self {
        Timing {
            min: c,
            max: Some(c),
            label: c.to_string(),
            detail: format!("{c} cycles"),
        }
    }

    fn range(min: u32, max: u32, label: String, detail: String) -> Self {
        Timing {
            min,
            max: Some(max),
            label,
            detail,
        }
    }

    /// Timing of a two-way instruction (branch taken or not).
    fn either(a: u32, a_what: &str, b: u32, b_what: &str) -> Self {
        Self::range(
            a.min(b),
            a.max(b),
            format!("{a}/{b}"),
            format!("{a} cycles {a_what}, {b} {b_what}"),
        )
    }
}

/// Classify an operand. `None` for forms without 68000 timing (68020+
/// addressing, register lists, special registers).
pub fn classify(op: &str) -> Option<Ea> {
    let s = op.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    if s.starts_with('#') {
        return Some(Ea::Imm);
    }
    if is_dreg(&s) {
        return Some(Ea::Dn);
    }
    if is_areg(&s) {
        return Some(Ea::An);
    }
    if let Some(inner) = s.strip_prefix("-(").and_then(|r| r.strip_suffix(')')) {
        return is_areg(inner).then_some(Ea::PreDec);
    }
    if let Some(inner) = s.strip_prefix('(').and_then(|r| r.strip_suffix(")+")) {
        return is_areg(inner).then_some(Ea::PostInc);
    }
    if let Some(open) = s.find('(')
        && s.ends_with(')')
    {
        let outer = &s[..open];
        let parts: Vec<&str> = s[open + 1..s.len() - 1].split(',').map(str::trim).collect();
        // `d(An,Xn)` puts the displacement outside, `(d,An,Xn)` inside.
        let (disp_inside, regs) = match parts.first() {
            Some(p) if !is_areg(p) && *p != "pc" => (true, &parts[1..]),
            _ => (false, &parts[..]),
        };
        let has_disp = disp_inside || !outer.is_empty();
        let base = *regs.first()?;
        let index = match regs.len() {
            1 => false,
            2 => is_index_reg(regs[1]),
            _ => return None,
        };
        if regs.len() == 2 && !index {
            return None;
        }
        return match (base, index, has_disp) {
            ("pc", false, _) => Some(Ea::PcDisp),
            ("pc", true, _) => Some(Ea::PcIndex),
            (b, false, false) if is_areg(b) => Some(Ea::Ind),
            (b, false, true) if is_areg(b) => Some(Ea::Disp),
            (b, true, _) if is_areg(b) => Some(Ea::Index),
            // `(label).w`, `($4)` and similar parenthesised absolutes.
            _ if !index => Some(absolute(&s)),
            _ => None,
        };
    }
    if s.contains('/') || s.contains('-') && s.split('-').all(|r| is_dreg(r) || is_areg(r)) {
        return None; // register list
    }
    Some(absolute(&s))
}

/// Absolute addressing: long unless `.w` asks for short, which is also
/// what the assembler encodes.
fn absolute(s: &str) -> Ea {
    if s.ends_with(".w") {
        Ea::AbsW
    } else {
        Ea::AbsL
    }
}

fn is_dreg(s: &str) -> bool {
    matches!(s.as_bytes(), [b'd', b'0'..=b'7'])
}

fn is_areg(s: &str) -> bool {
    matches!(s.as_bytes(), [b'a', b'0'..=b'7']) || s == "sp"
}

fn is_index_reg(s: &str) -> bool {
    let reg = s.split('.').next().unwrap_or(s);
    is_dreg(reg) || is_areg(reg)
}

/// Number of registers in a MOVEM register list such as `d0-d7/a0-a6`.
pub fn register_count(list: &str) -> Option<u32> {
    let mut n = 0;
    for part in list.trim().to_ascii_lowercase().split('/') {
        let part = part.trim();
        let index = |r: &str| -> Option<u32> {
            match r.as_bytes() {
                [b'd', d @ b'0'..=b'7'] => Some((d - b'0') as u32),
                [b'a', d @ b'0'..=b'7'] => Some(8 + (d - b'0') as u32),
                _ if r == "sp" => Some(15),
                _ => None,
            }
        };
        n += match part.split_once('-') {
            Some((a, b)) => index(b)?.checked_sub(index(a)?)? + 1,
            None => {
                index(part)?;
                1
            }
        };
    }
    Some(n)
}

/// Timing for one instruction. `size` is the suffix without the dot
/// (empty when omitted). `None` when the instruction or an operand form has
/// no 68000 timing to report.
pub fn instruction_timing(mnemonic: &str, size: &str, operands: &[String]) -> Option<Timing> {
    let m = mnemonic.to_ascii_lowercase();
    let size = size.to_ascii_lowercase();
    let long = size == "l";
    let ea = |i: usize| operands.get(i).and_then(|o| classify(o));
    let op_text = |i: usize| {
        operands
            .get(i)
            .map(|o| o.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    let fixed = |c| Some(Timing::fixed(c));

    match m.as_str() {
        "move" | "movea" => {
            let src = op_text(0);
            let dst = op_text(1);
            // MOVE to/from SR, CCR, USP
            if dst == "sr" || dst == "ccr" {
                return fixed(12 + ea(0)?.calc(false));
            }
            if src == "sr" {
                return fixed(match ea(1)? {
                    Ea::Dn => 6,
                    e => 8 + e.calc(false),
                });
            }
            if src == "usp" || dst == "usp" {
                return fixed(4);
            }
            let (s, d) = (ea(0)?, ea(1)?);
            // Destination -(An) costs the same as (An) for MOVE.
            let d = if d == Ea::PreDec { Ea::Ind } else { d };
            fixed(4 + s.calc(long) + d.calc(long))
        }
        "moveq" => fixed(4),

        "add" | "sub" | "and" | "or" | "eor" | "cmp" | "adda" | "suba" | "cmpa" => {
            let (s, d) = (ea(0)?, ea(1)?);
            let base = m.trim_end_matches('a');
            if s == Ea::Imm && !d.is_reg() {
                // Assembled as the immediate form.
                return immediate(&format!("{base}i"), long, d);
            }
            match d {
                Ea::An => {
                    let t = match (base, long) {
                        ("cmp", _) => 6,
                        (_, false) => 8,
                        (_, true) if s.is_reg() || s == Ea::Imm => 8,
                        (_, true) => 6,
                    };
                    fixed(t + s.calc(long))
                }
                Ea::Dn => {
                    let t = match (base, long) {
                        ("eor", false) => 4,
                        ("eor", true) => 8,
                        (_, false) => 4,
                        ("cmp", true) => 6,
                        (_, true) if s.is_reg() || s == Ea::Imm => 8,
                        (_, true) => 6,
                    };
                    fixed(t + s.calc(long))
                }
                mem => {
                    if base == "cmp" {
                        return None; // CMP has no Dn,<M> form
                    }
                    fixed(if long { 12 } else { 8 } + mem.calc(long))
                }
            }
        }
        "addi" | "subi" | "andi" | "ori" | "eori" | "cmpi" => {
            let dst = op_text(1);
            if dst == "sr" || dst == "ccr" {
                return fixed(20);
            }
            immediate(&m, long, ea(1)?)
        }
        "addq" | "subq" => match ea(1)? {
            Ea::Dn => fixed(if long { 8 } else { 4 }),
            Ea::An => fixed(8),
            mem => fixed(if long { 12 } else { 8 } + mem.calc(long)),
        },

        "mulu" | "muls" => {
            let e = ea(0)?.calc(false);
            Some(Timing::range(
                38 + e,
                70 + e,
                format!("{}-{}", 38 + e, 70 + e),
                format!(
                    "38+2n{} cycles: n = {} of the source, at most 70",
                    plus(e),
                    if m == "mulu" {
                        "number of ones"
                    } else {
                        "number of 01/10 bit pairs"
                    }
                ),
            ))
        }
        "divu" | "divs" => {
            let worst = if m == "divu" { 140 } else { 158 } + ea(0)?.calc(false);
            Some(Timing::range(
                worst,
                worst,
                format!("≤{worst}"),
                format!("at most {worst} cycles; the best case is less than 10% faster"),
            ))
        }

        "clr" | "neg" | "negx" | "not" => match ea(0)? {
            e if e.is_reg() => fixed(if long { 6 } else { 4 }),
            mem => fixed(if long { 12 } else { 8 } + mem.calc(long)),
        },
        "tst" => fixed(4 + ea(0)?.calc(long)),
        "nbcd" => match ea(0)? {
            Ea::Dn => fixed(6),
            mem => fixed(8 + mem.calc(false)),
        },
        "tas" => match ea(0)? {
            Ea::Dn => fixed(4),
            mem => fixed(10 + mem.calc(false)),
        },
        _ if is_scc(&m) => match ea(0)? {
            Ea::Dn => Some(Timing::either(6, "if true", 4, "if false")),
            mem => fixed(8 + mem.calc(false)),
        },

        "asl" | "asr" | "lsl" | "lsr" | "rol" | "ror" | "roxl" | "roxr" => {
            if operands.len() == 1 {
                return fixed(8 + ea(0)?.calc(false));
            }
            let base = if long { 8 } else { 6 };
            let count = op_text(0);
            match count.strip_prefix('#').and_then(parse_number) {
                Some(n) => fixed(base + 2 * n),
                None if is_dreg(&count) => Some(Timing {
                    min: base,
                    max: None,
                    label: format!("{base}+2n"),
                    detail: format!("{base}+2n cycles, n = shift count in {count} (mod 64)"),
                }),
                None => None,
            }
        }

        "btst" | "bchg" | "bclr" | "bset" => {
            let dynamic = ea(0)? == Ea::Dn;
            match ea(1)? {
                Ea::Dn => {
                    let t = match (m.as_str(), dynamic) {
                        ("btst", true) => 6,
                        ("btst", false) => 10,
                        ("bclr", true) => 10,
                        ("bclr", false) => 14,
                        (_, true) => 8,
                        (_, false) => 12,
                    };
                    if m == "btst" {
                        fixed(t)
                    } else {
                        // Maximum; bit numbers below 16 take 2 fewer.
                        Some(Timing::range(
                            t - 2,
                            t,
                            t.to_string(),
                            format!("{t} cycles, {} for bit numbers 0-15", t - 2),
                        ))
                    }
                }
                mem => {
                    let t = match (m.as_str(), dynamic) {
                        ("btst", true) => 4,
                        ("btst", false) => 8,
                        (_, true) => 8,
                        (_, false) => 12,
                    };
                    fixed(t + mem.calc(false))
                }
            }
        }

        "bra" => fixed(10),
        "bsr" => fixed(18),
        _ if is_bcc(&m) => {
            if size == "w" {
                Some(Timing::either(10, "taken", 12, "not taken"))
            } else {
                Some(Timing::either(10, "taken", 8, "not taken"))
            }
        }
        _ if is_dbcc(&m) => Some(Timing::range(
            10,
            14,
            "10/14".into(),
            "10 cycles looping, 14 when the count expires, 12 when the condition is true".into(),
        )),

        "jmp" | "jsr" | "lea" | "pea" => {
            let e = ea(0)?;
            let t = match (m.as_str(), e) {
                (_, Ea::Dn | Ea::An | Ea::PostInc | Ea::PreDec | Ea::Imm) => return None,
                ("jmp", Ea::Ind) => 8,
                ("jmp", Ea::Disp | Ea::AbsW | Ea::PcDisp) => 10,
                ("jmp", Ea::AbsL) => 12,
                ("jmp", _) => 14,
                ("jsr", Ea::Ind) => 16,
                ("jsr", Ea::Disp | Ea::AbsW | Ea::PcDisp) => 18,
                ("jsr", Ea::AbsL) => 20,
                ("jsr", _) => 22,
                ("lea", Ea::Ind) => 4,
                ("lea", Ea::Disp | Ea::AbsW | Ea::PcDisp) => 8,
                ("lea", _) => 12,
                ("pea", Ea::Ind) => 12,
                ("pea", Ea::Disp | Ea::AbsW | Ea::PcDisp) => 16,
                (_, _) => 20,
            };
            fixed(t)
        }

        "movem" => {
            let per = if long { 8 } else { 4 };
            let (list, mem, to_regs) =
                match (register_count(&op_text(0)), register_count(&op_text(1))) {
                    (Some(n), None) => (n, ea(1)?, false),
                    (None, Some(n)) => (n, ea(0)?, true),
                    _ => return None,
                };
            let base = match (to_regs, mem) {
                (true, Ea::Ind | Ea::PostInc) => 12,
                (true, Ea::Disp | Ea::AbsW | Ea::PcDisp) => 16,
                (true, Ea::Index | Ea::PcIndex) => 18,
                (true, Ea::AbsL) => 20,
                (false, Ea::Ind | Ea::PreDec) => 8,
                (false, Ea::Disp | Ea::AbsW) => 12,
                (false, Ea::Index) => 14,
                (false, Ea::AbsL) => 16,
                _ => return None,
            };
            let t = base + per * list;
            Some(Timing::range(
                t,
                t,
                t.to_string(),
                format!("{base}+{per}n cycles, n = {list} registers"),
            ))
        }

        "addx" | "subx" => match ea(0)? {
            Ea::Dn => fixed(if long { 8 } else { 4 }),
            _ => fixed(if long { 30 } else { 18 }),
        },
        "abcd" | "sbcd" => match ea(0)? {
            Ea::Dn => fixed(6),
            _ => fixed(18),
        },
        "cmpm" => fixed(if long { 20 } else { 12 }),
        "movep" => fixed(if long { 24 } else { 16 }),

        "chk" => Some(Timing::range(
            10 + ea(0)?.calc(false),
            40 + ea(0)?.calc(false),
            (10 + ea(0)?.calc(false)).to_string(),
            "without trap; about 40 more when it traps".into(),
        )),
        "trap" => fixed(34),
        "trapv" => fixed(4),
        "illegal" => fixed(34),
        "exg" => fixed(6),
        "ext" | "swap" | "nop" | "stop" => fixed(4),
        "link" => fixed(16),
        "unlk" => fixed(12),
        "rts" => fixed(16),
        "rte" | "rtr" => fixed(20),
        "reset" => fixed(132),
        _ => None,
    }
}

/// Immediate forms (table 8-5); `dst` is the destination operand.
fn immediate(m: &str, long: bool, dst: Ea) -> Option<Timing> {
    let t = match (m, dst, long) {
        ("cmpi", Ea::Dn, false) => 8,
        ("cmpi", Ea::Dn, true) => 14,
        ("cmpi", mem, false) => 8 + mem.calc(false),
        ("cmpi", mem, true) => 12 + mem.calc(true),
        (_, Ea::Dn, false) => 8,
        (_, Ea::Dn, true) => 16,
        (_, Ea::An, _) => return None,
        (_, mem, false) => 12 + mem.calc(false),
        (_, mem, true) => 20 + mem.calc(true),
    };
    Some(Timing::fixed(t))
}

fn plus(e: u32) -> String {
    if e == 0 {
        String::new()
    } else {
        format!("+{e}")
    }
}

fn parse_number(s: &str) -> Option<u32> {
    let s = s.trim();
    match s.strip_prefix('$') {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

const CONDITIONS: [&str; 16] = [
    "t", "f", "hi", "ls", "cc", "cs", "ne", "eq", "vc", "vs", "pl", "mi", "ge", "lt", "gt", "le",
];

fn is_bcc(m: &str) -> bool {
    m.strip_prefix('b').is_some_and(|c| {
        c != "t" && c != "f" && (CONDITIONS.contains(&c) || c == "hs" || c == "lo")
    })
}

fn is_dbcc(m: &str) -> bool {
    m == "dbra"
        || m.strip_prefix("db")
            .is_some_and(|c| CONDITIONS.contains(&c) || c == "hs" || c == "lo")
}

fn is_scc(m: &str) -> bool {
    m.strip_prefix('s')
        .is_some_and(|c| CONDITIONS.contains(&c) || c == "hs" || c == "lo")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(line: &str) -> Timing {
        let (_, mnemonic, size, operands) = m68k_core::tokens::split_line(line);
        instruction_timing(&mnemonic, &size, &operands)
            .unwrap_or_else(|| panic!("no timing: {line}"))
    }

    fn c(line: &str) -> u32 {
        let timing = t(line);
        assert_eq!(
            Some(timing.min),
            timing.max,
            "{line} is not fixed: {timing:?}"
        );
        timing.min
    }

    /// Every cell of the MOVE tables (8-2 and 8-3).
    #[test]
    fn move_matches_the_tables() {
        let src = [
            "d1", "a1", "(a1)", "(a1)+", "-(a1)", "4(a1)", "4(a1,d2)", "$10.w", "$10000", "4(pc)",
            "4(pc,d2)", "#1",
        ];
        let dst = [
            "d0", "a0", "(a0)", "(a0)+", "-(a0)", "4(a0)", "4(a0,d3)", "$20.w", "$20000",
        ];
        #[rustfmt::skip]
        let word: [[u32; 9]; 12] = [
            [4, 4, 8, 8, 8, 12, 14, 12, 16],
            [4, 4, 8, 8, 8, 12, 14, 12, 16],
            [8, 8, 12, 12, 12, 16, 18, 16, 20],
            [8, 8, 12, 12, 12, 16, 18, 16, 20],
            [10, 10, 14, 14, 14, 18, 20, 18, 22],
            [12, 12, 16, 16, 16, 20, 22, 20, 24],
            [14, 14, 18, 18, 18, 22, 24, 22, 26],
            [12, 12, 16, 16, 16, 20, 22, 20, 24],
            [16, 16, 20, 20, 20, 24, 26, 24, 28],
            [12, 12, 16, 16, 16, 20, 22, 20, 24],
            [14, 14, 18, 18, 18, 22, 24, 22, 26],
            [8, 8, 12, 12, 12, 16, 18, 16, 20],
        ];
        #[rustfmt::skip]
        let long: [[u32; 9]; 12] = [
            [4, 4, 12, 12, 12, 16, 18, 16, 20],
            [4, 4, 12, 12, 12, 16, 18, 16, 20],
            [12, 12, 20, 20, 20, 24, 26, 24, 28],
            [12, 12, 20, 20, 20, 24, 26, 24, 28],
            [14, 14, 22, 22, 22, 26, 28, 26, 30],
            [16, 16, 24, 24, 24, 28, 30, 28, 32],
            [18, 18, 26, 26, 26, 30, 32, 30, 34],
            [16, 16, 24, 24, 24, 28, 30, 28, 32],
            [20, 20, 28, 28, 28, 32, 34, 32, 36],
            [16, 16, 24, 24, 24, 28, 30, 28, 32],
            [18, 18, 26, 26, 26, 30, 32, 30, 34],
            [12, 12, 20, 20, 20, 24, 26, 24, 28],
        ];
        for (r, s) in src.iter().enumerate() {
            for (col, d) in dst.iter().enumerate() {
                assert_eq!(
                    c(&format!(" move.w {s},{d}")),
                    word[r][col],
                    "move.w {s},{d}"
                );
                assert_eq!(
                    c(&format!(" move.l {s},{d}")),
                    long[r][col],
                    "move.l {s},{d}"
                );
            }
        }
    }

    #[test]
    fn standard_and_immediate() {
        assert_eq!(c(" add.w d1,d0"), 4);
        assert_eq!(c(" add.l d1,d0"), 8); // register source: 6 becomes 8
        assert_eq!(c(" add.l (a0),d0"), 14);
        assert_eq!(c(" adda.l d0,a0"), 8);
        assert_eq!(c(" adda.w (a0),a1"), 12);
        assert_eq!(c(" cmpa.l (a0),a1"), 14);
        assert_eq!(c(" cmp.l (a0),d0"), 14);
        assert_eq!(c(" add.w d0,4(a0)"), 16);
        assert_eq!(c(" eor.l d0,d1"), 8);
        assert_eq!(c(" addi.l #1,d0"), 16);
        assert_eq!(c(" add.w #1,(a0)"), 16); // ADDI to memory
        assert_eq!(c(" cmpi.l #1,d0"), 14);
        assert_eq!(c(" cmpi.w #1,(a0)"), 12);
        assert_eq!(c(" addq.w #1,a0"), 8);
        assert_eq!(c(" subq.l #1,(a0)+"), 20);
        assert_eq!(c(" andi.w #$ff,sr"), 20);
    }

    #[test]
    fn single_operand_shift_and_bits() {
        assert_eq!(c(" clr.l d0"), 6);
        assert_eq!(c(" clr.w (a0)"), 12);
        assert_eq!(c(" tst.l (a0)"), 12);
        assert_eq!(c(" asl.l #2,d0"), 12);
        assert_eq!(c(" lsr.w #8,d0"), 22);
        assert_eq!(c(" lsl.w (a0)"), 12);
        let by_reg = t(" lsl.l d1,d0");
        assert_eq!(
            (by_reg.min, by_reg.max, by_reg.label.as_str()),
            (8, None, "8+2n")
        );
        assert_eq!(c(" btst #3,d0"), 10);
        assert_eq!(c(" btst d1,(a0)"), 8);
        assert_eq!(t(" bclr d1,d0").label, "10");
        assert_eq!(c(" bset #7,$bfe001"), 24);
    }

    #[test]
    fn flow_and_misc() {
        assert_eq!(t(" bne.s loop").label, "10/8");
        assert_eq!(t(" beq.w far").label, "10/12");
        assert_eq!(t(" dbf d7,loop").label, "10/14");
        assert_eq!(c(" jsr -198(a6)"), 18);
        assert_eq!(c(" jsr (a0)"), 16);
        assert_eq!(c(" lea label(pc),a0"), 8);
        assert_eq!(c(" lea $dff000,a6"), 12);
        assert_eq!(c(" movem.l d0-d7/a0-a6,-(sp)"), 8 + 8 * 15);
        assert_eq!(c(" movem.w (sp)+,d0/d2"), 12 + 4 * 2);
        assert_eq!(c(" link a5,#-8"), 16);
        assert_eq!(c(" move.w #$8380,DMACON(a6)"), 16);
        assert_eq!(c(" move.l #copper,$dff080"), 28);
        assert_eq!(c(" move.w sr,-(sp)"), 14);
        assert_eq!(t(" mulu.w d1,d0").label, "38-70");
        assert_eq!(t(" divs.w #3,d0").label, "≤162");
    }

    #[test]
    fn no_timing_for_unknown_forms() {
        let (_, m, s, ops) = m68k_core::tokens::split_line(" move.l ([4,a0],d0*4),d1");
        assert_eq!(instruction_timing(&m, &s, &ops), None);
        assert_eq!(instruction_timing("dc", "w", &["1".into()]), None);
    }
}
