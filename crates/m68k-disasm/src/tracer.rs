//! Abstract execution of a loaded program to discover which bytes are code.
//!
//! [`crate::disassembler::Disassembler`] can follow branches whose target
//! sits in the opword, but not `jsr (a5)`-style computed jumps, whose
//! target is whatever a register happens to hold. This module recovers
//! those by *executing* the program: it tracks register contents well
//! enough to resolve an indirect jump, and reports every address it
//! observes executing as a PC trace.
//!
//! # Why this works on executables and not on ROMs
//!
//! Measured on Kickstart 1.3, 816 of the computed jumps load their target
//! from low (RAM) addresses and exactly one from the ROM itself. Those RAM
//! contents are AmigaOS jump tables built during boot — they do not exist
//! in the image, so no amount of interpretation recovers them without
//! emulating the machine. A linked executable is the opposite case: it is
//! self-contained, its entry point is defined, and its internal calls are
//! PC-relative or absolute. What it does need from the operating system —
//! `jsr -$126(a6)` against a library base — is precisely what this module
//! declines to model, treating the call as an opaque side effect and
//! continuing at the following instruction.
//!
//! # What this deliberately is not
//!
//! Not a CPU emulator. Condition codes, arithmetic results, and memory
//! writes are not modelled; a value that cannot be determined statically
//! becomes [`Value::Unknown`] and propagates as such. Both sides of a
//! conditional branch are explored regardless of any computed flag, so
//! coverage does not depend on guessing which way a comparison went. The
//! result is a set of addresses that are *reachable*, which is what the
//! disassembler needs — not a faithful record of one particular run.

use std::collections::HashSet;

use m68k_core::addressing::{EAOperand, InstructionStream};

use crate::decoder::{DecodeResult, decode_next};

/// Upper bound on instructions executed before tracing gives up.
///
/// Abstract execution explores both sides of every branch, so a program
/// with many conditionals can expand far beyond its own instruction
/// count. This bounds the work; hitting it yields a partial trace, which
/// is still useful, rather than hanging.
const DEFAULT_STEP_LIMIT: usize = 2_000_000;

/// What is known about a register's contents.
///
/// Only what is needed to resolve a jump target: an address the program
/// computed, or nothing. Arithmetic on an unknown operand yields unknown
/// rather than a guess, so a resolved target is always one the program
/// genuinely produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Value {
    /// A known 32-bit value.
    Known(u32),
    /// Not statically determinable — a library return value, a memory
    /// read, or arithmetic involving either.
    Unknown,
}

impl Value {
    fn known(self) -> Option<u32> {
        match self {
            Value::Known(v) => Some(v),
            Value::Unknown => None,
        }
    }
}

/// The subset of machine state this tracer models: the eight data and
/// eight address registers. Memory is not modelled — see [`Value`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct State {
    d: [Value; 8],
    a: [Value; 8],
}

impl State {
    fn unknown() -> Self {
        Self {
            d: [Value::Unknown; 8],
            a: [Value::Unknown; 8],
        }
    }
}

/// One place execution can continue, with the register state it arrives
/// holding.
#[derive(Debug, Clone)]
struct Work {
    pc: u32,
    state: State,
}

/// Result of tracing a program.
#[derive(Debug, Clone)]
pub struct TraceResult {
    /// Every address observed as an instruction start, ascending.
    pub pcs: Vec<u32>,
    /// Computed jumps (`jsr (a5)` and friends) whose target the tracer
    /// resolved, as `(site, target)`. Reported separately because these
    /// are the addresses a purely static walk cannot reach.
    pub resolved_indirect: Vec<(u32, u32)>,
    /// Computed jumps whose target stayed unknown. A non-empty list means
    /// the trace is incomplete in a way the caller may want to know
    /// about, rather than silently partial.
    pub unresolved_indirect: Vec<u32>,
    /// True if [`Tracer`] stopped at its step limit, so `pcs` is partial.
    pub hit_step_limit: bool,
}

/// Abstract executor over a flat image.
pub struct Tracer<'a> {
    data: &'a [u8],
    origin: u32,
    cpu: String,
    step_limit: usize,
}

impl<'a> Tracer<'a> {
    pub fn new(data: &'a [u8], origin: u32) -> Self {
        Self {
            data,
            origin,
            cpu: "68000".to_string(),
            step_limit: DEFAULT_STEP_LIMIT,
        }
    }

    pub fn set_cpu(&mut self, cpu: &str) {
        self.cpu = cpu.to_string();
    }

    pub fn set_step_limit(&mut self, limit: usize) {
        self.step_limit = limit;
    }

    fn offset_of(&self, addr: u32) -> Option<usize> {
        let off = addr.checked_sub(self.origin)? as usize;
        (off < self.data.len()).then_some(off)
    }

    /// Execute from `entries`, returning every address reached.
    pub fn trace<I: IntoIterator<Item = u32>>(&self, entries: I) -> TraceResult {
        let mut pcs: HashSet<u32> = HashSet::new();
        let mut resolved: Vec<(u32, u32)> = Vec::new();
        let mut unresolved: HashSet<u32> = HashSet::new();
        // Visiting the same address twice with the same register state
        // cannot produce anything new, so that pair is the termination
        // condition. Tracking state (not just the address) is what lets a
        // subroutine called from two sites be explored under both, which
        // is often how one call site resolves a jump the other does not.
        let mut seen: HashSet<(u32, u64)> = HashSet::new();
        let mut queue: Vec<Work> = entries
            .into_iter()
            .map(|pc| Work {
                pc,
                state: State::unknown(),
            })
            .collect();

        let mut steps = 0usize;
        let mut hit_limit = false;

        while let Some(work) = queue.pop() {
            let mut pc = work.pc;
            let mut state = work.state;

            loop {
                steps += 1;
                if steps > self.step_limit {
                    hit_limit = true;
                    break;
                }
                let Some(off) = self.offset_of(pc) else { break };
                if !seen.insert((pc, state_key(&state))) {
                    break;
                }

                let mut stream = InstructionStream::new(self.data, self.origin);
                stream.seek(off);
                let Ok((_, DecodeResult::Instruction(inst))) = decode_next(&mut stream, &self.cpu)
                else {
                    break;
                };
                pcs.insert(pc);

                let next_pc = pc.wrapping_add(inst.raw_bytes.len() as u32);
                let stem = inst.mnemonic.split('.').next().unwrap_or("");

                // Control flow first: whether execution continues at
                // `next_pc` is what the rest of the loop depends on.
                match classify(stem) {
                    Flow::Return | Flow::Stop => break,
                    Flow::Jump => {
                        // Unconditional transfer: follow the target and
                        // stop; nothing continues at next_pc.
                        if let Some(t) = inst.target_address {
                            queue.push(Work {
                                pc: t,
                                state: state.clone(),
                            });
                        } else if let Some(t) = self.indirect_target(&inst, &state) {
                            resolved.push((pc, t));
                            queue.push(Work {
                                pc: t,
                                state: state.clone(),
                            });
                        } else if is_indirect(&inst) {
                            unresolved.insert(pc);
                        }
                        break;
                    }
                    Flow::Call => {
                        // A call returns, so execution continues after it.
                        // The callee may clobber anything, so registers
                        // are unknown on return — modelling a calling
                        // convention would be guessing.
                        if let Some(t) = inst.target_address {
                            queue.push(Work {
                                pc: t,
                                state: state.clone(),
                            });
                        } else if let Some(t) = self.indirect_target(&inst, &state) {
                            resolved.push((pc, t));
                            queue.push(Work {
                                pc: t,
                                state: state.clone(),
                            });
                        } else if is_indirect(&inst) {
                            unresolved.insert(pc);
                        }
                        state = State::unknown();
                    }
                    Flow::Branch => {
                        // Both paths are explored: which way a real run
                        // went depends on flags this tracer does not
                        // model, and coverage must not depend on a guess.
                        if let Some(t) = inst.target_address {
                            queue.push(Work {
                                pc: t,
                                state: state.clone(),
                            });
                        }
                        apply_effect(&inst, stem, &mut state);
                    }
                    Flow::Normal => apply_effect(&inst, stem, &mut state),
                }

                pc = next_pc;
            }
        }

        let mut out: Vec<u32> = pcs.into_iter().collect();
        out.sort_unstable();
        resolved.sort_unstable();
        resolved.dedup();
        let mut unres: Vec<u32> = unresolved.into_iter().collect();
        unres.sort_unstable();

        TraceResult {
            pcs: out,
            resolved_indirect: resolved,
            unresolved_indirect: unres,
            hit_step_limit: hit_limit,
        }
    }

    /// Resolve `jmp (an)` / `jsr (an)` from the tracked register.
    fn indirect_target(
        &self,
        inst: &crate::decoder::DecodedInstruction,
        state: &State,
    ) -> Option<u32> {
        let ea = inst.operands.first()?.ea.as_ref()?;
        let reg = match ea {
            EAOperand::AddrIndirect(n) => *n,
            _ => return None,
        };
        let target = state.a.get(reg as usize)?.known()?;
        // A target outside the image is a real value the program computed
        // — a library entry, say — but there is nothing here to trace.
        self.offset_of(target).map(|_| target)
    }
}

/// Whether an instruction takes an address-register-indirect operand,
/// i.e. is a computed jump whose target this tracer tries to resolve.
fn is_indirect(inst: &crate::decoder::DecodedInstruction) -> bool {
    matches!(
        inst.operands.first().and_then(|o| o.ea.as_ref()),
        Some(EAOperand::AddrIndirect(_))
    )
}

/// How an instruction affects what executes next.
enum Flow {
    /// Falls through to the next instruction.
    Normal,
    /// Conditionally transfers: both the target and the next instruction
    /// are reachable.
    Branch,
    /// Transfers and returns (`bsr`/`jsr`).
    Call,
    /// Transfers without returning (`bra`/`jmp`).
    Jump,
    /// Ends this path (`rts`/`rte`/...).
    Return,
    /// Halts (`stop`/`illegal`).
    Stop,
}

fn classify(stem: &str) -> Flow {
    match stem {
        "rts" | "rte" | "rtr" | "rtd" | "rtm" => Flow::Return,
        "stop" | "illegal" => Flow::Stop,
        "bra" | "jmp" => Flow::Jump,
        "bsr" | "jsr" => Flow::Call,
        // Bcc (3 letters, b-prefixed), DBcc and the FPU equivalents all
        // have a fall-through path as well as a target.
        s if s.len() == 3 && s.starts_with('b') => Flow::Branch,
        s if s.starts_with("db") || s.starts_with("fb") || s.starts_with("fdb") => Flow::Branch,
        _ => Flow::Normal,
    }
}

/// Update tracked registers for a non-control-flow instruction.
///
/// Only the few forms that actually produce jump targets are modelled —
/// `lea`, and `move`/`movea` of an immediate or another register. Every
/// other instruction invalidates whatever it writes, so a stale value can
/// never be mistaken for a live one.
fn apply_effect(inst: &crate::decoder::DecodedInstruction, stem: &str, state: &mut State) {
    let src = inst.operands.first().and_then(|o| o.ea.as_ref());
    let dst = inst.operands.get(1).and_then(|o| o.ea.as_ref());

    let value = match (stem, src) {
        // `lea <ea>,An` — the canonical way a program computes an address.
        ("lea", Some(ea)) | ("pea", Some(ea)) => match ea {
            EAOperand::AbsoluteLong(v) => Value::Known(*v),
            EAOperand::AbsoluteShort(v) => Value::Known(*v as u32),
            // PcDisp carries the resolved target in its first field.
            EAOperand::PcDisp(target, _) => Value::Known(*target as u32),
            _ => Value::Unknown,
        },
        ("move" | "movea" | "moveq", Some(ea)) => match ea {
            EAOperand::Immediate(v, _) => Value::Known(*v as u32),
            EAOperand::AbsoluteLong(v) => Value::Known(*v),
            EAOperand::DataReg(n) => state.d[*n as usize],
            EAOperand::AddrReg(n) => state.a[*n as usize],
            // Anything read from memory is unknown: memory is not
            // modelled, and guessing here is how a wrong jump target
            // would be invented.
            _ => Value::Unknown,
        },
        _ => Value::Unknown,
    };

    match dst {
        Some(EAOperand::DataReg(n)) => state.d[*n as usize] = value,
        Some(EAOperand::AddrReg(n)) => state.a[*n as usize] = value,
        // A write through a register does not change the register, but an
        // unmodelled destination might alias anything; only registers are
        // tracked, so nothing to invalidate.
        _ => {}
    }
}

/// Compact key for the `(pc, state)` visited-set.
///
/// Only whether each register is known, plus the known values, matters
/// for whether revisiting can produce something new. Hashing the values
/// directly would make the set enormous on register-heavy code; folding
/// them keeps it bounded while still distinguishing states that resolve
/// different jump targets.
fn state_key(state: &State) -> u64 {
    let mut key = 0u64;
    let mut hash = 1469598103934665603u64; // FNV-1a offset basis
    for (i, v) in state.a.iter().chain(state.d.iter()).enumerate() {
        if let Value::Known(x) = v {
            key |= 1 << i;
            hash ^= *x as u64;
            hash = hash.wrapping_mul(1099511628211);
        }
    }
    key ^ hash
}

/// Render a trace as a PC trace table, one value per line.
///
/// The format [`crate::disassembler::parse_pc_trace`] reads, so a trace
/// produced here can be fed straight back with `-t`.
pub fn format_pc_trace(pcs: &[u32]) -> String {
    let mut out = String::new();
    for pc in pcs {
        out.push_str(&format!("0x{:08x}\n", pc));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_straight_line_and_stops_at_rts() {
        // nop; nop; rts; <unreached>
        let data = vec![0x4E, 0x71, 0x4E, 0x71, 0x4E, 0x75, 0xFF, 0xFF];
        let t = Tracer::new(&data, 0x1000);
        let r = t.trace([0x1000]);

        assert_eq!(r.pcs, vec![0x1000, 0x1002, 0x1004]);
    }

    /// The point of the module: a jump whose target lives in a register,
    /// which the disassembler's static walk cannot follow.
    #[test]
    fn resolves_computed_jump_through_lea() {
        // lea $1010,a5 ; jsr (a5) ; rts ; ... ; $1010: nop; rts
        let data = vec![
            0x4B, 0xF9, 0x00, 0x00, 0x10, 0x10, // lea $00001010,a5
            0x4E, 0x95, // jsr (a5)
            0x4E, 0x75, // rts
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // padding to 0x1010
            0x4E, 0x71, // 0x1010: nop
            0x4E, 0x75, // rts
        ];
        let t = Tracer::new(&data, 0x1000);
        let r = t.trace([0x1000]);

        assert_eq!(r.resolved_indirect, vec![(0x1006, 0x1010)]);
        // And the routine behind the computed jump was actually traced.
        assert!(r.pcs.contains(&0x1010), "target not traced: {:x?}", r.pcs);
        assert!(r.pcs.contains(&0x1012));
        assert!(r.unresolved_indirect.is_empty());
    }

    /// A jump through a register loaded from memory cannot be resolved,
    /// and must be reported rather than guessed at.
    #[test]
    fn reports_unresolvable_computed_jump() {
        // movea.l $4.w,a6 ; jsr (a6) ; rts
        let data = vec![
            0x2C, 0x78, 0x00, 0x04, // movea.l $0004.w,a6
            0x4E, 0x96, // jsr (a6)
            0x4E, 0x75, // rts
        ];
        let t = Tracer::new(&data, 0x1000);
        let r = t.trace([0x1000]);

        assert_eq!(r.unresolved_indirect, vec![0x1004]);
        assert!(r.resolved_indirect.is_empty());
        // Execution still continues after the call.
        assert!(r.pcs.contains(&0x1006));
    }

    /// Both sides of a conditional branch are explored: which way a real
    /// run goes depends on flags this tracer does not model.
    #[test]
    fn explores_both_sides_of_conditional_branch() {
        // beq.s +4 ; nop ; rts ; nop ; rts
        let data = vec![
            0x67, 0x04, // beq.s -> 0x1006
            0x4E, 0x71, // nop (fall-through)
            0x4E, 0x75, // rts
            0x4E, 0x71, // 0x1006: nop (taken)
            0x4E, 0x75, // rts
        ];
        let t = Tracer::new(&data, 0x1000);
        let r = t.trace([0x1000]);

        assert_eq!(r.pcs, vec![0x1000, 0x1002, 0x1004, 0x1006, 0x1008]);
    }

    /// A loop must terminate rather than spin: revisiting an address in a
    /// state already seen cannot produce anything new.
    #[test]
    fn terminates_on_backward_branch() {
        // loop: nop ; bra.s loop
        let data = vec![0x4E, 0x71, 0x60, 0xFC];
        let t = Tracer::new(&data, 0x1000);
        let r = t.trace([0x1000]);

        assert_eq!(r.pcs, vec![0x1000, 0x1002]);
        assert!(!r.hit_step_limit);
    }

    /// A call clobbers registers, so a target resolved before the call
    /// must not be reused after it.
    #[test]
    fn call_invalidates_tracked_registers() {
        // lea $1010,a5 ; jsr (a5) ; jsr (a5) — the second jsr must be
        // unresolved, because the callee may have changed a5.
        let data = vec![
            0x4B, 0xF9, 0x00, 0x00, 0x10, 0x10, // lea $00001010,a5
            0x4E, 0x95, // jsr (a5)
            0x4E, 0x95, // jsr (a5)  <- a5 unknown here
            0x4E, 0x75, // rts
            0x00, 0x00, 0x00, 0x00, // pad
            0x4E, 0x75, // 0x1010: rts
        ];
        let t = Tracer::new(&data, 0x1000);
        let r = t.trace([0x1000]);

        assert_eq!(r.resolved_indirect, vec![(0x1006, 0x1010)]);
        assert_eq!(r.unresolved_indirect, vec![0x1008]);
    }

    /// A resolved target outside the image is a real value, but there is
    /// nothing to trace there — it must not be queued or reported as
    /// resolved-and-followed.
    #[test]
    fn out_of_image_target_is_not_followed() {
        // lea $90000,a5 ; jsr (a5) ; rts
        let data = vec![
            0x4B, 0xF9, 0x00, 0x09, 0x00, 0x00, // lea $00090000,a5
            0x4E, 0x95, // jsr (a5)
            0x4E, 0x75, // rts
        ];
        let t = Tracer::new(&data, 0x1000);
        let r = t.trace([0x1000]);

        assert!(r.resolved_indirect.is_empty());
        assert_eq!(r.unresolved_indirect, vec![0x1006]);
    }

    #[test]
    fn step_limit_yields_partial_trace_rather_than_hanging() {
        let data: Vec<u8> = std::iter::repeat_n([0x4E, 0x71], 32).flatten().collect();
        let mut t = Tracer::new(&data, 0x1000);
        t.set_step_limit(4);
        let r = t.trace([0x1000]);

        assert!(r.hit_step_limit);
        assert!(r.pcs.len() <= 4);
    }

    #[test]
    fn format_pc_trace_roundtrips_through_the_parser() {
        let pcs = vec![0x1000, 0x1002, 0x2000];
        let text = format_pc_trace(&pcs);
        let parsed = crate::disassembler::parse_pc_trace(&text).unwrap();
        assert_eq!(parsed, pcs);
    }
}
