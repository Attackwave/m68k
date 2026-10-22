//! Copper instruction decoding.
//!
//! Every copper instruction is two words. Bit 0 of the first selects MOVE
//! (0) or WAIT/SKIP (1); for the latter, bit 0 of the second selects WAIT
//! (0) or SKIP (1). `$FFFF,$FFFE` waits for a beam position that never
//! comes and is the conventional end of a list.

use crate::amiga_custom::{self, Copper};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopperInsn {
    /// Write `value` to the register at `offset` from $DFF000.
    Move {
        offset: u16,
        value: u16,
    },
    Wait(Position),
    Skip(Position),
    /// `$FFFF,$FFFE`.
    End,
}

/// Beam position and compare masks of a WAIT or SKIP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Vertical position, bits 7-0 of the line.
    pub v: u8,
    /// Horizontal position in colour clocks, bits 8-1 (always even).
    pub h: u8,
    /// Vertical compare enable mask (bit 7 is always compared).
    pub v_mask: u8,
    /// Horizontal compare enable mask.
    pub h_mask: u8,
    /// Also wait for the blitter to finish: the blitter-finish-disable bit
    /// (bit 15 of the second word) is clear. The usual `$FFFE` sets it.
    pub waits_for_blitter: bool,
}

pub fn decode(ir1: u16, ir2: u16) -> CopperInsn {
    if ir1 & 1 == 0 {
        return CopperInsn::Move {
            offset: ir1 & 0x1FE,
            value: ir2,
        };
    }
    if ir1 == 0xFFFF && ir2 == 0xFFFE {
        return CopperInsn::End;
    }
    let pos = Position {
        v: (ir1 >> 8) as u8,
        h: (ir1 & 0xFE) as u8,
        v_mask: ((ir2 >> 8) & 0x7F) as u8 | 0x80,
        h_mask: (ir2 & 0xFE) as u8,
        waits_for_blitter: ir2 & 0x8000 == 0,
    };
    if ir2 & 1 == 0 {
        CopperInsn::Wait(pos)
    } else {
        CopperInsn::Skip(pos)
    }
}

/// What the copper does with a MOVE to `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveEffect {
    Writes,
    /// Only with the COPCON danger bit set.
    NeedsDanger,
    /// The copper stops.
    Stops,
}

pub fn move_effect(offset: u16) -> MoveEffect {
    match amiga_custom::by_offset(offset).map(|r| r.copper) {
        Some(Copper::No) => MoveEffect::Stops,
        Some(Copper::Danger) => MoveEffect::NeedsDanger,
        Some(Copper::Yes) => MoveEffect::Writes,
        // Unassigned offsets: the same address ranges as the table.
        None if offset < 0x40 => MoveEffect::Stops,
        None if offset < 0x80 => MoveEffect::NeedsDanger,
        None => MoveEffect::Writes,
    }
}

impl Position {
    fn format(&self) -> String {
        let mut s = format!("v=${:02X} h=${:02X}", self.v, self.h);
        if self.v_mask != 0xFF || self.h_mask != 0xFE {
            s.push_str(&format!(
                " mask v=${:02X} h=${:02X}",
                self.v_mask, self.h_mask
            ));
        }
        if self.waits_for_blitter {
            s.push_str(" and blitter finished");
        }
        s
    }
}

/// One line of text for an instruction, with register names and decoded
/// values: `MOVE COLOR00 = $0F00  R=15 G=0 B=0`.
pub fn format(insn: &CopperInsn) -> String {
    match insn {
        CopperInsn::Move { offset, value } => {
            let (name, summary) = match amiga_custom::by_offset(*offset) {
                Some(reg) => {
                    let summary = amiga_custom::summarize(reg, *value);
                    let summary = if amiga_custom::fields(reg).is_empty() {
                        String::new()
                    } else {
                        summary
                    };
                    (reg.name.to_string(), summary)
                }
                None => (format!("${:03X}", offset), String::new()),
            };
            let mut s = format!("MOVE {name} = ${value:04X}");
            if !summary.is_empty() {
                s.push_str("  ");
                s.push_str(&summary);
            }
            match move_effect(*offset) {
                MoveEffect::Writes => {}
                MoveEffect::NeedsDanger => s.push_str("  [needs COPCON danger bit]"),
                MoveEffect::Stops => s.push_str("  [copper stops here]"),
            }
            s
        }
        CopperInsn::Wait(p) => format!("WAIT {}", p.format()),
        CopperInsn::Skip(p) => format!("SKIP {}", p.format()),
        CopperInsn::End => "END".to_string(),
    }
}

/// Decode a list from `words` until END or the end of the slice, returning
/// each instruction with its index in the slice.
pub fn decode_list(words: &[u16]) -> Vec<CopperInsn> {
    let mut out = Vec::new();
    for [ir1, ir2] in words.as_chunks::<2>().0 {
        let insn = decode(*ir1, *ir2);
        out.push(insn);
        let stops = matches!(insn, CopperInsn::Move { offset, .. } if move_effect(offset) == MoveEffect::Stops);
        if insn == CopperInsn::End || stops {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_three_instructions_and_the_end_marker() {
        assert_eq!(
            format(&decode(0x0180, 0x0F80)),
            "MOVE COLOR00 = $0F80  R=15 G=8 B=0"
        );
        assert_eq!(format(&decode(0x2C07, 0xFFFE)), "WAIT v=$2C h=$06");
        assert_eq!(format(&decode(0x2C07, 0xFFFF)), "SKIP v=$2C h=$06");
        assert_eq!(decode(0xFFFF, 0xFFFE), CopperInsn::End);
        // Bit 15 clear: the copper also waits for the blitter.
        assert_eq!(
            format(&decode(0x0001, 0x7F00)),
            "WAIT v=$00 h=$00 mask v=$FF h=$00 and blitter finished"
        );
        assert_eq!(
            format(&decode(0x0100, 0x4200)),
            "MOVE BPLCON0 = $4200  BPU=4|COLOR"
        );
        assert_eq!(
            format(&decode(0x0096, 0x8380)),
            "MOVE DMACON = $8380  SET DMAEN|BPLEN|COPEN"
        );
    }

    #[test]
    fn flags_moves_the_copper_cannot_make() {
        assert!(format(&decode(0x0040, 0x09F0)).ends_with("[needs COPCON danger bit]"));
        assert!(format(&decode(0x0002, 0)).ends_with("[copper stops here]"));
        assert!(!format(&decode(0x0180, 0)).contains('['));
    }

    #[test]
    fn list_stops_at_end_or_illegal_move() {
        let words = [
            0x0180, 0x0F00, 0x2C07, 0xFFFE, 0xFFFF, 0xFFFE, 0x0180, 0x00F0,
        ];
        assert_eq!(decode_list(&words).len(), 3);
        assert_eq!(decode_list(&[0x0010, 0, 0x0180, 0]).len(), 1);
    }
}
