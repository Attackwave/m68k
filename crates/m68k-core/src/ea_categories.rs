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

    pub const DATA: u16 =
        DREG | AREG_IND | APOSTINC | APREDEC | AINDEXED | ABSW | ABSL | PCDISP | PCINDEXED | IMM;
    pub const MEMORY: u16 =
        AREG_IND | APOSTINC | APREDEC | AINDEXED | ABSW | ABSL | PCDISP | PCINDEXED;
    pub const CONTROL: u16 = AREG_IND | AINDEXED | ABSW | ABSL | PCDISP | PCINDEXED;
    pub const ALTERABLE_MEMORY: u16 = AREG_IND | APOSTINC | APREDEC | AINDEXED | ABSW | ABSL;
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

fn ea_mode_to_bit(mode: u8, reg: u8) -> u16 {
    match mode {
        0b000 => ea::DREG,
        0b001 => ea::AREG,
        0b010 => ea::AREG_IND,
        0b011 => ea::APOSTINC,
        0b100 => ea::APREDEC,
        0b101 if reg != 7 => ea::AINDEXED,
        0b101 if reg == 7 => ea::ABSW,
        0b110 if reg != 7 => ea::AINDEXED,
        0b110 if reg == 7 => ea::ABSL,
        0b111 if reg == 0b001 => ea::PCDISP,
        0b111 if reg == 0b011 => ea::PCINDEXED,
        0b111 if reg == 0b100 => ea::IMM,
        _ => 0,
    }
}
