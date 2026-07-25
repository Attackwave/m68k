//! Effective Address category bitmasks.
//!
//! These bitmasks are shared between the assembler and disassembler to
//! classify which addressing modes an instruction accepts.

/// EA category bitmasks.
pub mod ea {
    pub const DREG: u16 = 1 << 0;
    pub const AREG: u16 = 1 << 1;
    pub const AREG_IND: u16 = 1 << 2;
    pub const APOSTINC: u16 = 1 << 3;
    pub const APREDEC: u16 = 1 << 4;
    pub const AINDEXED: u16 = 1 << 5;
    pub const ABSW: u16 = 1 << 6;
    pub const ABSL: u16 = 1 << 7;
    pub const PCDISP: u16 = 1 << 8;
    pub const PCINDEXED: u16 = 1 << 9;
    pub const IMM: u16 = 1 << 10;
    /// Mode 5, `(d16,An)`: address register indirect with displacement.
    /// Previously unmapped as its own bit — mode 5 fell through to
    /// AINDEXED (mode 6's bit) instead, which happened to still work for
    /// every category below since both bits were always granted together,
    /// but conflated two distinct addressing modes under one name.
    pub const AREG_DISP: u16 = 1 << 11;

    pub const DATA: u16 = DREG
        | AREG_IND
        | APOSTINC
        | APREDEC
        | AREG_DISP
        | AINDEXED
        | ABSW
        | ABSL
        | PCDISP
        | PCINDEXED
        | IMM;
    pub const MEMORY: u16 =
        AREG_IND | APOSTINC | APREDEC | AREG_DISP | AINDEXED | ABSW | ABSL | PCDISP | PCINDEXED;
    pub const CONTROL: u16 = AREG_IND | AREG_DISP | AINDEXED | ABSW | ABSL | PCDISP | PCINDEXED;
    pub const ALTERABLE_MEMORY: u16 =
        AREG_IND | APOSTINC | APREDEC | AREG_DISP | AINDEXED | ABSW | ABSL;
    /// Control addressing modes plus Dn (used by BFxxx source operands that allow read-modify-write).
    pub const CONTROL_ALT: u16 = ALTERABLE_MEMORY;
    /// Alterable memory modes plus Dn (used by e.g. CAS).
    pub const MEM_ALT: u16 = ALTERABLE_MEMORY | DREG;
    /// Alterable data addressing modes: Dn plus alterable memory, no PC-relative/immediate
    /// (used as the FPU destination-EA category, e.g. `FMOVE FPn,<ea>`).
    pub const DATA_ALT: u16 = MEM_ALT;
    pub const ALL: u16 = 0xFFFF;
}

/// Check if an EA mode matches a category bitmask.
pub fn ea_matches(mode: u8, reg: u8, category: u16) -> bool {
    let ea_bit = ea_mode_to_bit(mode, reg);
    ea_bit != 0 && (category & ea_bit) != 0
}

// Per the M68000PRM addressing mode table, modes 5 (address register
// indirect with displacement) and 6 (address register indirect with
// index) apply uniformly to every address register An (0-7) — there is
// no reg==7 special case, unlike mode 7 where the reg field selects a
// completely different addressing mode (0=abs.W, 1=abs.L, 2=PC disp,
// 3=PC index, 4=immediate). A prior version conflated "An == A7" with
// "reg field == 7 under mode 7", routing `(d16,A7)`/`(d8,A7,Xn)` to
// ABSW/ABSL and leaving mode 7 reg 0/1 (the real abs.W/abs.L encodings)
// unmapped entirely (falling through to the `_ => 0` "invalid" case).
fn ea_mode_to_bit(mode: u8, reg: u8) -> u16 {
    match mode {
        0b000 => ea::DREG,
        0b001 => ea::AREG,
        0b010 => ea::AREG_IND,
        0b011 => ea::APOSTINC,
        0b100 => ea::APREDEC,
        0b101 => ea::AREG_DISP,
        0b110 => ea::AINDEXED,
        0b111 if reg == 0b000 => ea::ABSW,
        0b111 if reg == 0b001 => ea::ABSL,
        0b111 if reg == 0b010 => ea::PCDISP,
        0b111 if reg == 0b011 => ea::PCINDEXED,
        0b111 if reg == 0b100 => ea::IMM,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: mode 5/6 with reg==7 (i.e. An == A7/SP) previously
    /// mapped to ABSW/ABSL instead of AREG_DISP/AINDEXED, and mode 7
    /// reg 0/1 (the real abs.W/abs.L) mapped to nothing at all.
    #[test]
    fn test_areg_disp_and_indexed_apply_to_a7() {
        assert!(ea_matches(0b101, 7, ea::AREG_DISP));
        assert!(!ea_matches(0b101, 7, ea::ABSW));
        assert!(ea_matches(0b110, 7, ea::AINDEXED));
        assert!(!ea_matches(0b110, 7, ea::ABSL));
    }

    #[test]
    fn test_mode7_reg0_is_absw_reg1_is_absl() {
        assert!(ea_matches(0b111, 0, ea::ABSW));
        assert!(ea_matches(0b111, 1, ea::ABSL));
        assert!(!ea_matches(0b111, 0, ea::ABSL));
        assert!(!ea_matches(0b111, 1, ea::ABSW));
    }

    #[test]
    fn test_areg_disp_included_in_common_categories() {
        // (d16,An) must remain valid wherever it always was, now under
        // its own bit instead of piggybacking on AINDEXED.
        assert!(ea_matches(0b101, 0, ea::DATA));
        assert!(ea_matches(0b101, 0, ea::MEMORY));
        assert!(ea_matches(0b101, 0, ea::CONTROL));
        assert!(ea_matches(0b101, 0, ea::ALTERABLE_MEMORY));
    }
}
