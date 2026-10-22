//! Static checks over the assembled program: stack balance and depth per
//! routine, and condition-code results no instruction reads.
//!
//! The input is what pass 2 encoded, one [`InstrInfo`] per instruction
//! with its parsed operands, so sizes, addresses and branch targets are
//! exactly those of the output. Data never enters the analysis.
//!
//! "Unknown" is a result, not a failure: an indirect jump, a stack
//! pointer loaded from memory or an unknown callee stops the arithmetic
//! and is reported with its reason and location rather than guessed at.

use std::collections::{BTreeMap, HashMap, HashSet};

use m68k_core::operands::Operand;

/// One encoded instruction as the analysis sees it.
#[derive(Debug, Clone)]
pub struct InstrInfo {
    pub pc: u32,
    pub size: u32,
    pub line_no: usize,
    /// `file:line` as written, for messages that point at another line.
    pub location: String,
    /// Lower-case mnemonic without size suffix.
    pub mnemonic: String,
    /// Operation size in bytes (1, 2, 4), 0 when the instruction has none.
    pub op_size: u8,
    pub operands: Vec<Operand>,
    /// Resolved branch or jump target, when the operand names one.
    pub target: Option<u32>,
}

const SP: u8 = 7;

/// Stack pointer offset relative to the routine entry, in bytes
/// (negative = deeper).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Sp {
    Known(i32),
    Unknown(String),
}

/// Maximum stack use of a routine including everything it calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Depth {
    Exact(u32),
    /// A lower bound: some callee or path could not be followed.
    AtLeast(u32),
    /// The routine is on a cycle of the call graph.
    Unbounded(String),
}

impl std::fmt::Display for Depth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Depth::Exact(n) => write!(f, "{n}"),
            Depth::AtLeast(n) => write!(f, ">= {n}"),
            Depth::Unbounded(cycle) => write!(f, "unbounded ({cycle})"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoutineReport {
    pub name: String,
    pub pc: u32,
    pub depth: Depth,
    /// `None` when no return is reachable (an endless loop, an exception
    /// handler ending in RTE), so there is nothing to balance against.
    pub balanced: Option<bool>,
    /// Where the arithmetic stopped or disagreed: (line, message). These
    /// are the findings `--stack-check` reports.
    pub problems: Vec<(usize, String)>,
    /// Facts worth knowing that are not errors (an unknown callee, a
    /// tail call).
    pub notes: Vec<String>,
}

/// A flag-setting instruction whose result is overwritten before any
/// instruction reads it: (line, message).
pub type DeadFlagSetter = (usize, String);

struct Program<'a> {
    instrs: &'a [InstrInfo],
    by_pc: HashMap<u32, usize>,
    /// Routine entries: address and name, ascending.
    ///
    /// A global label is a routine entry when something calls it, or
    /// when no instruction refers to it at all (a reset vector, an
    /// exception handler, a table entry). A label only ever branched to
    /// is a continuation of whoever branches there — a shared epilogue,
    /// an old-style loop head — and is walked as part of that routine
    /// with the stack height carried along.
    routines: Vec<(u32, String)>,
    entries: HashSet<u32>,
    /// Frame size for every UNLK, from the nearest earlier LINK on the
    /// same register.
    unlk: HashMap<u32, i32>,
    /// Call targets declared `NORETURN`.
    noreturn: &'a HashSet<u32>,
}

impl<'a> Program<'a> {
    fn new(instrs: &'a [InstrInfo], labels: &[(String, u32)], noreturn: &'a HashSet<u32>) -> Self {
        let by_pc: HashMap<u32, usize> =
            instrs.iter().enumerate().map(|(i, x)| (x.pc, i)).collect();
        let mut called: HashSet<u32> = HashSet::new();
        let mut jumped: HashSet<u32> = HashSet::new();
        for i in instrs {
            match flow(i) {
                Flow::Call(Some(t)) => {
                    called.insert(t);
                }
                Flow::Next(next) => jumped.extend(next.into_iter().filter(|t| *t != i.pc + i.size)),
                _ => {}
            }
        }
        let mut routines: Vec<(u32, String)> = labels
            .iter()
            .filter(|(_, pc)| by_pc.contains_key(pc))
            .filter(|(_, pc)| called.contains(pc) || !jumped.contains(pc))
            .map(|(n, pc)| (*pc, n.clone()))
            .collect();
        routines.sort();
        routines.dedup_by_key(|(pc, _)| *pc);
        let entries = routines.iter().map(|(pc, _)| *pc).collect();

        let mut frames: HashMap<u8, i32> = HashMap::new();
        let mut unlk = HashMap::new();
        for i in instrs {
            match (i.mnemonic.as_str(), i.operands.first(), i.operands.get(1)) {
                ("link", Some(Operand::AddrReg(r)), Some(Operand::Immediate(d))) => {
                    frames.insert(*r, *d as i32);
                }
                ("unlk", Some(Operand::AddrReg(r)), _) => {
                    if let Some(d) = frames.get(r) {
                        unlk.insert(i.pc, 4 - *d);
                    }
                }
                _ => {}
            }
        }
        Self {
            instrs,
            by_pc,
            routines,
            entries,
            unlk,
            noreturn,
        }
    }

    /// Where control goes after `i`, with `NORETURN` applied: a call to
    /// a declared routine ends the path, and inside such a routine a
    /// return is not a return to the caller (it unwinds to somewhere
    /// else), so it is not balance-checked.
    fn flow(&self, i: &InstrInfo, entry: u32) -> Flow {
        match flow(i) {
            Flow::Call(Some(t)) if self.noreturn.contains(&t) => Flow::NoReturn(t),
            Flow::Return if self.noreturn.contains(&entry) => Flow::ReturnUnchecked,
            f => f,
        }
    }

    fn name_of(&self, entry: u32) -> &str {
        self.routines
            .iter()
            .find(|(pc, _)| *pc == entry)
            .map_or("?", |(_, n)| n.as_str())
    }

    fn at(&self, pc: u32) -> Option<&'a InstrInfo> {
        self.by_pc.get(&pc).map(|&i| &self.instrs[i])
    }
}

enum Flow {
    /// Continues at these addresses (fall-through and/or branch target).
    Next(Vec<u32>),
    /// A call that returns to the following instruction; `None` = callee
    /// unknown.
    Call(Option<u32>),
    /// RTS/RTD: leaves the routine; the stack must be back at the entry
    /// level.
    Return,
    /// RTE/RTR: leaves through an exception or CCR frame, which this
    /// analysis does not model, so the stack is not checked.
    ReturnUnchecked,
    /// An indirect jump: nothing can be said about what follows.
    UnknownJump,
    /// A call declared `NORETURN`: the path ends here.
    NoReturn(u32),
}

fn is_conditional_branch(m: &str) -> bool {
    matches!(
        m,
        "bhi"
            | "bls"
            | "bcc"
            | "bcs"
            | "bhs"
            | "blo"
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
    )
}

fn jump_target(i: &InstrInfo) -> Option<u32> {
    i.target.or_else(|| match i.operands.first()? {
        Operand::Address(a) | Operand::Memory(a) | Operand::Immediate(a) => Some(*a as u32),
        Operand::AbsoluteShort(a) | Operand::AbsoluteLong(a) => Some(*a as u32),
        Operand::PcRelativeDisp(target, _) => Some(*target as u32),
        _ => None,
    })
}

fn flow(i: &InstrInfo) -> Flow {
    let fall = i.pc + i.size;
    match i.mnemonic.as_str() {
        "bra" => Flow::Next(i.target.into_iter().collect()),
        "jmp" => match jump_target(i) {
            Some(t) => Flow::Next(vec![t]),
            None => Flow::UnknownJump,
        },
        "bsr" => Flow::Call(i.target),
        "jsr" => Flow::Call(jump_target(i)),
        "rts" | "rtd" => Flow::Return,
        "rte" | "rtr" => Flow::ReturnUnchecked,
        m if is_conditional_branch(m) || m.starts_with("db") => {
            let mut next = vec![fall];
            next.extend(i.target);
            Flow::Next(next)
        }
        _ => Flow::Next(vec![fall]),
    }
}

fn is_sp(op: &Operand) -> bool {
    matches!(op, Operand::AddrReg(SP))
}

/// Bytes a push or pop of `op_size` moves SP by: the 68000 keeps SP even,
/// so a byte push still takes a word.
fn push_size(op_size: u8) -> i32 {
    match op_size {
        0..=2 => 2,
        n => n as i32,
    }
}

/// How the instruction moves SP, or why that cannot be told.
/// `unlk_delta` is the frame size the matching LINK allocated.
fn sp_delta(i: &InstrInfo, unlk_delta: Option<i32>) -> Result<i32, String> {
    let m = i.mnemonic.as_str();
    let ops = &i.operands;
    let dst = ops.last();

    match m {
        "pea" => return Ok(-4),
        "link" => {
            return match ops.get(1) {
                Some(Operand::Immediate(d)) => Ok(-4 + *d as i32),
                _ => Err("LINK with an unreadable displacement".into()),
            };
        }
        "unlk" => {
            return unlk_delta
                .ok_or_else(|| "UNLK without a preceding LINK on this register".into());
        }
        "lea" if dst.is_some_and(is_sp) => {
            return match ops.first() {
                Some(Operand::AddrRegIndirectDisp(SP, d, _)) => Ok(*d),
                Some(Operand::AddrRegIndirect(SP)) => Ok(0),
                _ => Err("SP loaded by LEA from something other than SP".into()),
            };
        }
        "adda" | "addq" | "add" | "suba" | "subq" | "sub" if dst.is_some_and(is_sp) => {
            return match ops.first() {
                Some(Operand::Immediate(n)) => {
                    let n = *n as i32;
                    Ok(if m.starts_with("add") { n } else { -n })
                }
                _ => Err(format!(
                    "SP changed by {} with a non-constant amount",
                    m.to_uppercase()
                )),
            };
        }
        _ => {}
    }

    // Anything else that writes SP; compares and tests only read it.
    let reads_only = matches!(m, "cmp" | "cmpa" | "cmpi" | "cmpm" | "tst" | "btst" | "chk");
    if (dst.is_some_and(is_sp) && !reads_only) || (m == "exg" && ops.iter().any(is_sp)) {
        return Err(format!("SP set by {}", m.to_uppercase()));
    }

    // Pushes and pops through -(SP) / (SP)+ in any operand position.
    let unit = if m == "movem" || m == "fmovem" {
        let count = match ops
            .iter()
            .find(|o| !matches!(o, Operand::AddrRegPreDec(_) | Operand::AddrRegPostInc(_)))
        {
            Some(Operand::Immediate(mask)) => mask.count_ones() as i32,
            Some(_) => 1,
            None => 0,
        };
        let per = if m == "fmovem" {
            12
        } else {
            push_size(i.op_size)
        };
        count * per
    } else {
        push_size(i.op_size)
    };
    let mut delta = 0;
    for op in ops {
        match op {
            Operand::AddrRegPreDec(SP) => delta -= unit,
            Operand::AddrRegPostInc(SP) => delta += unit,
            _ => {}
        }
    }
    Ok(delta)
}

struct Local {
    /// Deepest known offset reached (<= 0).
    min_offset: i32,
    /// Some state was unknown, so `min_offset` is only a bound.
    partial: bool,
    returns: bool,
    balanced: bool,
    /// (callee entry or None, SP offset at the call, is tail call)
    calls: Vec<(Option<u32>, i32, bool)>,
    problems: Vec<(usize, String)>,
    notes: Vec<String>,
}

fn analyze_routine(prog: &Program, entry: u32) -> Local {
    let mut state: BTreeMap<u32, Sp> = BTreeMap::new();
    let mut work = vec![entry];
    state.insert(entry, Sp::Known(0));
    let mut local = Local {
        min_offset: 0,
        partial: false,
        returns: false,
        balanced: true,
        calls: Vec::new(),
        problems: Vec::new(),
        notes: Vec::new(),
    };
    let mut reported_unknown = false;

    while let Some(pc) = work.pop() {
        let Some(i) = prog.at(pc) else { continue };
        let here = state[&pc].clone();
        let out = match (&here, sp_delta(i, prog.unlk.get(&pc).copied())) {
            (Sp::Known(o), Ok(d)) => Sp::Known(o + d),
            (Sp::Known(_), Err(why)) => Sp::Unknown(format!("{why} at {}", i.location)),
            (Sp::Unknown(why), _) => Sp::Unknown(why.clone()),
        };
        match &out {
            Sp::Known(o) => local.min_offset = local.min_offset.min(*o),
            Sp::Unknown(why) => {
                local.partial = true;
                if !reported_unknown {
                    local
                        .notes
                        .push(format!("stack height unknown from {}: {why}", i.location));
                    reported_unknown = true;
                }
            }
        }

        let mut successors: Vec<u32> = Vec::new();
        match prog.flow(i, entry) {
            Flow::Next(next) => successors = next,
            Flow::Call(callee) => {
                if let Sp::Known(o) = out {
                    local.calls.push((callee, o, false));
                    local.min_offset = local.min_offset.min(o - 4);
                }
                if callee.is_none() {
                    local
                        .notes
                        .push(format!("indirect call at {}: callee unknown", i.location));
                }
                let after = i.pc + i.size;
                // The code after the call belongs to another routine: the
                // callee is taken not to return (an error exit), since
                // continuing would be a fall-through into that routine.
                if after != entry && prog.entries.contains(&after) {
                    local.notes.push(format!(
                        "call at {} is the last instruction before {}: assumed not to return",
                        i.location,
                        prog.name_of(after)
                    ));
                } else {
                    successors.push(after);
                }
            }
            Flow::NoReturn(callee) => {
                if let Sp::Known(o) = out {
                    local.calls.push((Some(callee), o, false));
                    local.min_offset = local.min_offset.min(o - 4);
                }
            }
            Flow::ReturnUnchecked => local.returns = true,
            Flow::Return => {
                local.returns = true;
                match &out {
                    Sp::Known(0) => {}
                    Sp::Known(o) => {
                        local.balanced = false;
                        local.problems.push((
                            i.line_no,
                            format!(
                                "{} with SP {} bytes {} the entry level",
                                i.mnemonic.to_uppercase(),
                                o.abs(),
                                if *o < 0 { "below" } else { "above" }
                            ),
                        ));
                    }
                    Sp::Unknown(why) => {
                        local.balanced = false;
                        local.problems.push((
                            i.line_no,
                            format!(
                                "{} with the stack height unknown: {why}",
                                i.mnemonic.to_uppercase()
                            ),
                        ));
                    }
                }
            }
            Flow::UnknownJump => {
                local.partial = true;
                local.notes.push(format!(
                    "indirect jump at {}: paths beyond it not followed",
                    i.location
                ));
            }
        }

        for next in successors {
            if prog.at(next).is_none() {
                local.notes.push(format!(
                    "{}: continues outside the code (data or a gap)",
                    i.location
                ));
                continue;
            }
            // Leaving the routine other than by a return is a tail call:
            // the callee's RTS returns to our caller, so the stack must
            // be at the entry level here.
            if next != entry && prog.entries.contains(&next) {
                match &out {
                    Sp::Known(0) => {}
                    Sp::Known(o) => {
                        local.balanced = false;
                        local.problems.push((
                            i.line_no,
                            format!(
                                "jump to another routine with SP {} bytes off the entry level",
                                o.abs()
                            ),
                        ));
                    }
                    Sp::Unknown(why) => {
                        local.balanced = false;
                        local.problems.push((
                            i.line_no,
                            format!("jump to another routine with the stack height unknown: {why}"),
                        ));
                    }
                }
                local.returns = true;
                local.calls.push((Some(next), 0, true));
                continue;
            }
            let merged = match (state.get(&next), &out) {
                (None, s) => s.clone(),
                (Some(Sp::Known(a)), Sp::Known(b)) if a == b => continue,
                // Two heights at one point is only a problem if a return
                // can still be reached from it; that return reports it.
                (Some(Sp::Known(a)), Sp::Known(b)) => {
                    let where_ = prog.at(next).map_or("?", |x| x.location.as_str());
                    Sp::Unknown(format!(
                        "paths reach {where_} with different stack heights ({a} and {b} bytes)"
                    ))
                }
                (Some(Sp::Unknown(_)), _) => continue,
                (Some(Sp::Known(_)), Sp::Unknown(why)) => Sp::Unknown(why.clone()),
            };
            state.insert(next, merged);
            work.push(next);
        }
    }
    local
}

/// Stack balance and depth for every routine. `routines` are the global
/// labels at code addresses; which of them are routine entries is decided
/// by how they are reached (see [`Program`]).
pub fn stack_analysis(
    instrs: &[InstrInfo],
    routines: &[(String, u32)],
    noreturn: &HashSet<u32>,
) -> Vec<RoutineReport> {
    let prog = Program::new(instrs, routines, noreturn);
    let locals: HashMap<u32, Local> = prog
        .routines
        .iter()
        .map(|(entry, _)| (*entry, analyze_routine(&prog, *entry)))
        .collect();

    let mut memo: HashMap<u32, Depth> = HashMap::new();
    let mut reports = Vec::new();
    for (entry, name) in &prog.routines {
        let mut stack = Vec::new();
        let depth = total_depth(&prog, &locals, *entry, &mut stack, &mut memo);
        let l = &locals[entry];
        reports.push(RoutineReport {
            name: name.clone(),
            pc: *entry,
            depth,
            balanced: l.returns.then_some(l.balanced),
            problems: l.problems.clone(),
            notes: l.notes.clone(),
        });
    }
    reports
}

fn total_depth(
    prog: &Program,
    locals: &HashMap<u32, Local>,
    entry: u32,
    stack: &mut Vec<u32>,
    memo: &mut HashMap<u32, Depth>,
) -> Depth {
    if let Some(d) = memo.get(&entry) {
        return d.clone();
    }
    if let Some(pos) = stack.iter().position(|e| *e == entry) {
        let names: Vec<&str> = stack[pos..]
            .iter()
            .chain(std::iter::once(&entry))
            .map(|e| prog.name_of(*e))
            .collect();
        return Depth::Unbounded(names.join(" -> "));
    }
    let l = &locals[&entry];
    stack.push(entry);
    let mut max = l.min_offset.unsigned_abs();
    let mut exact = !l.partial;
    let mut unbounded = None;
    for (callee, offset, tail) in &l.calls {
        let own = offset.unsigned_abs() + if *tail { 0 } else { 4 };
        match callee.filter(|c| prog.entries.contains(c)) {
            Some(c) => match total_depth(prog, locals, c, stack, memo) {
                Depth::Exact(d) => max = max.max(own + d),
                Depth::AtLeast(d) => {
                    max = max.max(own + d);
                    exact = false;
                }
                Depth::Unbounded(cycle) => {
                    unbounded.get_or_insert(cycle);
                }
            },
            // A call into the middle of a routine or to an unknown
            // address: the callee's own use cannot be added.
            None => {
                max = max.max(own);
                exact = false;
            }
        }
    }
    stack.pop();
    let depth = match unbounded {
        Some(cycle) => Depth::Unbounded(cycle),
        None if exact => Depth::Exact(max),
        None => Depth::AtLeast(max),
    };
    // A routine on a cycle gets a different answer depending on where
    // the walk entered the cycle, so only acyclic results are kept.
    if !matches!(depth, Depth::Unbounded(_)) {
        memo.insert(entry, depth.clone());
    }
    depth
}

// ---------------------------------------------------------------------------
// Condition codes

#[derive(PartialEq, Eq)]
enum Ccr {
    /// Reads the condition codes (or might: calls, indirect jumps, FPU).
    Reads,
    /// Overwrites N, Z, V and C.
    Kills,
    /// Leaves them alone.
    Neutral,
}

fn is_special(op: Option<&Operand>, name: &str) -> bool {
    matches!(op, Some(Operand::Special(s)) if s.eq_ignore_ascii_case(name))
}

fn ccr_effect(i: &InstrInfo) -> Ccr {
    let m = i.mnemonic.as_str();
    let dst = i.operands.last();
    let dst_is_areg = matches!(dst, Some(Operand::AddrReg(_)));
    if is_special(dst, "ccr") || is_special(dst, "sr") {
        return match m {
            "move" => Ccr::Kills,
            _ => Ccr::Reads, // ANDI/ORI/EORI to CCR/SR read before writing
        };
    }
    if is_special(i.operands.first(), "ccr") || is_special(i.operands.first(), "sr") {
        return Ccr::Reads;
    }
    match m {
        "bra" | "bsr" | "jmp" | "jsr" | "nop" | "lea" | "pea" | "link" | "unlk" | "exg"
        | "movea" | "movem" | "dbra" | "dbf" | "rts" | "rtd" | "trap" | "illegal" | "reset"
        | "btst" | "bset" | "bclr" | "bchg" | "movec" | "moves" => Ccr::Neutral,
        "add" | "sub" | "addq" | "subq" | "adda" | "suba" | "move" | "cmp" | "cmpa"
            if dst_is_areg =>
        {
            if m == "cmp" || m == "cmpa" {
                Ccr::Kills
            } else {
                Ccr::Neutral
            }
        }
        "add" | "sub" | "addi" | "subi" | "addq" | "subq" | "and" | "andi" | "or" | "ori"
        | "eor" | "eori" | "not" | "neg" | "clr" | "tst" | "cmp" | "cmpi" | "cmpm" | "cmpa"
        | "ext" | "extb" | "moveq" | "move" | "mulu" | "muls" | "divu" | "divs" | "tas"
        | "swap" | "asl" | "asr" | "lsl" | "lsr" | "rol" | "ror" | "stop" => Ccr::Kills,
        m if is_conditional_branch(m)
            || m.starts_with("db")
            || m.starts_with('s') && m.len() <= 3
            || m.starts_with("trap") =>
        {
            Ccr::Reads
        }
        "addx" | "subx" | "negx" | "abcd" | "sbcd" | "nbcd" | "roxl" | "roxr" | "rtr" | "rte"
        | "chk" => Ccr::Reads,
        _ => Ccr::Reads,
    }
}

/// Pure flag-setters whose result every path overwrites before reading.
pub fn dead_flag_setters(
    instrs: &[InstrInfo],
    routines: &[(String, u32)],
    noreturn: &HashSet<u32>,
) -> Vec<DeadFlagSetter> {
    let prog = Program::new(instrs, routines, noreturn);
    let mut out = Vec::new();
    for i in instrs {
        if !matches!(
            i.mnemonic.as_str(),
            "tst" | "cmp" | "cmpi" | "cmpa" | "cmpm" | "btst"
        ) {
            continue;
        }
        let mut killer: Option<&InstrInfo> = None;
        let mut visited: HashSet<u32> = HashSet::new();
        let mut work: Vec<u32> = match prog.flow(i, i.pc) {
            Flow::Next(n) => n,
            _ => continue,
        };
        let mut live = false;
        while let Some(pc) = work.pop() {
            if !visited.insert(pc) {
                continue;
            }
            let Some(j) = prog.at(pc) else {
                live = true;
                break;
            };
            match ccr_effect(j) {
                Ccr::Reads => {
                    live = true;
                    break;
                }
                Ccr::Kills => {
                    killer.get_or_insert(j);
                }
                Ccr::Neutral => match prog.flow(j, i.pc) {
                    Flow::Next(n) => work.extend(n),
                    // A path that ends without reading is dead; a path
                    // that leaves (call, return, unknown jump) may read.
                    Flow::NoReturn(_) => {}
                    Flow::Call(_) | Flow::Return | Flow::ReturnUnchecked | Flow::UnknownJump => {
                        live = true;
                        break;
                    }
                },
            }
        }
        if !live && let Some(k) = killer {
            out.push((
                i.line_no,
                format!(
                    "{} sets condition codes nothing reads: {} at {} overwrites them first",
                    i.mnemonic.to_uppercase(),
                    k.mnemonic.to_uppercase(),
                    k.location
                ),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::Assembler;

    fn analyze(src: &str) -> (Vec<RoutineReport>, Vec<DeadFlagSetter>) {
        let mut asm = Assembler::new(0x1000);
        asm.assemble(src).unwrap_or_else(|e| panic!("{e}"));
        (asm.stack_analysis(), asm.dead_flag_setters())
    }

    fn report<'a>(r: &'a [RoutineReport], name: &str) -> &'a RoutineReport {
        r.iter()
            .find(|x| x.name == name)
            .unwrap_or_else(|| panic!("no routine {name}"))
    }

    #[test]
    fn balanced_routine_with_movem_and_link() {
        let (r, _) = analyze(
            "f:\tlink a6,#-8\n\tmovem.l d2-d4/a2,-(sp)\n\tmove.l d0,-(sp)\n\taddq.l #4,sp\n\tmovem.l (sp)+,d2-d4/a2\n\tunlk a6\n\trts\n",
        );
        let f = report(&r, "f");
        assert_eq!(f.balanced, Some(true), "{f:?}");
        // link: 4 + 8, movem 16, push 4
        assert_eq!(f.depth, Depth::Exact(32));
    }

    #[test]
    fn unbalanced_error_path_is_reported() {
        let (r, _) = analyze(
            "f:\tmove.l d0,-(sp)\n\ttst.l d1\n\tbeq.s .fail\n\taddq.l #4,sp\n\trts\n.fail:\tmoveq #-1,d0\n\trts\n",
        );
        let f = report(&r, "f");
        assert_eq!(f.balanced, Some(false));
        assert!(
            f.problems
                .iter()
                .any(|(line, m)| *line == 7 && m.contains("4 bytes below")),
            "{f:?}"
        );
    }

    #[test]
    fn disagreeing_paths_are_a_problem_only_at_a_reachable_return() {
        // The merge before a NORETURN call is harmless; the same merge
        // before an RTS is not.
        let src = "\tnoreturn die\nf:\ttst.l d0\n\tbeq .m\n\tclr.w -(sp)\n.m:\tbsr die\ng:\ttst.l d0\n\tbeq .n\n\tclr.w -(sp)\n.n:\trts\ndie:\trts\n";
        let (r, _) = analyze(src);
        let f = report(&r, "f");
        assert!(f.problems.is_empty(), "{f:?}");
        assert!(
            f.notes
                .iter()
                .any(|n| n.contains("different stack heights")),
            "{f:?}"
        );
        let g = report(&r, "g");
        assert_eq!(g.balanced, Some(false));
        assert!(
            g.problems
                .iter()
                .any(|(line, m)| *line == 9 && m.contains("stack height unknown")),
            "{g:?}"
        );
    }

    #[test]
    fn depth_sums_over_calls_and_marks_recursion() {
        let (r, _) = analyze(
            "main:\tbsr leaf\n\tbsr rec\n\trts\nleaf:\tmovem.l d0-d7,-(sp)\n\tmovem.l (sp)+,d0-d7\n\trts\nrec:\tsubq.l #1,d0\n\tbeq.s .done\n\tbsr rec\n.done:\trts\n",
        );
        assert_eq!(report(&r, "leaf").depth, Depth::Exact(32));
        assert!(matches!(report(&r, "rec").depth, Depth::Unbounded(ref c) if c == "rec -> rec"));
        assert!(matches!(report(&r, "main").depth, Depth::Unbounded(ref c) if c == "rec -> rec"));
    }

    #[test]
    fn indirect_call_and_stack_switch_are_unknown_not_guessed() {
        let (r, _) = analyze("disp:\tjsr (a2)\n\trts\nswitch:\tmove.l d0,sp\n\trts\n");
        let d = report(&r, "disp");
        assert_eq!(d.depth, Depth::AtLeast(4));
        assert!(d.notes.iter().any(|n| n.contains("callee unknown")));
        let s = report(&r, "switch");
        assert!(
            s.notes.iter().any(|m| m.contains("SP set by MOVE")),
            "{s:?}"
        );
        assert_eq!(s.balanced, Some(false));
        // Unknown is a note; returning with it unknown is the problem.
        assert!(
            s.problems
                .iter()
                .any(|(line, m)| *line == 4 && m.contains("unknown")),
            "{s:?}"
        );
    }

    #[test]
    fn noreturn_call_ends_the_path_and_last_call_before_a_routine_is_assumed_so() {
        let (r, _) = analyze(
            "	noreturn die
f:	pea 0
	tst.l d0
	beq .ok
	bsr die
.ok:	addq.l #4,sp
	rts
g:	move.l d0,-(sp)
	bsr other
h:	rts
die:	rts
other:	rts
",
        );
        let f = report(&r, "f");
        assert_eq!(f.balanced, Some(true), "{f:?}");
        assert_eq!(f.depth, Depth::Exact(8));
        let g = report(&r, "g");
        assert!(g.problems.is_empty(), "{g:?}");
        assert!(
            g.notes.iter().any(|n| n.contains("assumed not to return")),
            "{g:?}"
        );
    }

    #[test]
    fn rte_is_not_balance_checked() {
        let (r, _) = analyze(
            "handler:	movem.l d0-d1,-(sp)
	move.l x,sp
	rte
x:	dc.l 0
",
        );
        let h = report(&r, "handler");
        assert!(h.problems.is_empty(), "{h:?}");
        assert!(h.notes.iter().any(|n| n.contains("SP set by MOVE")));
    }

    #[test]
    fn dead_flag_message_names_the_file_and_line() {
        let (_, dead) = analyze(
            "f:	btst #0,d0
	moveq #1,d0
	bne f
	rts
",
        );
        assert_eq!(dead.len(), 1, "{dead:?}");
        assert!(dead[0].1.contains("MOVEQ at line 2"), "{}", dead[0].1);
    }

    #[test]
    fn tail_call_counts_without_return_address() {
        let (r, _) = analyze("a:\tbra b\nb:\tpea 0\n\taddq.l #4,sp\n\trts\n");
        assert_eq!(report(&r, "a").depth, Depth::Exact(4));
        assert_eq!(report(&r, "a").balanced, Some(true));
    }

    #[test]
    fn dead_flag_setter_is_found_and_live_one_is_not() {
        let (_, dead) = analyze(
            "f:\ttst.l d0\n\tmove.l d1,d2\n\tbeq .x\n.x:\tcmp.l d0,d1\n\tbne .y\n.y:\trts\n",
        );
        assert_eq!(dead.len(), 1, "{dead:?}");
        assert_eq!(dead[0].0, 1);
        assert!(dead[0].1.contains("MOVE at line 2"), "{}", dead[0].1);
    }

    #[test]
    fn flags_surviving_to_the_caller_are_live() {
        let (_, dead) = analyze("f:\ttst.l d0\n\tmovea.l a0,a1\n\trts\n");
        assert!(dead.is_empty(), "{dead:?}");
    }
}
