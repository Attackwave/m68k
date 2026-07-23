//! Operand AST types for the m68k assembler.

/// Operand types for m68k assembly instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    /// Data register: D0-D7
    DataReg(u8),
    /// Address register: A0-A7
    AddrReg(u8),
    /// Address register indirect: (An)
    AddrRegIndirect(u8),
    /// Address register indirect with post-increment: (An)+
    AddrRegPostInc(u8),
    /// Address register indirect with pre-decrement: -(An)
    AddrRegPreDec(u8),
    /// Address register indirect with displacement: (d16,An) or (d32,An)
    AddrRegIndirectDisp(u8, i32, bool),
    /// Address register indirect with index: (d8,An,Xn*scale). Fields:
    /// An, Xn, disp, scale, index size (`true` = `.L`, `false` = `.W`).
    AddrRegIndirectIndex(u8, u8, i8, u8, bool),
    /// Absolute short: (xxx).W
    AbsoluteShort(i32),
    /// Absolute long: (xxx).L
    AbsoluteLong(i32),
    /// Program counter with displacement: (d16,PC) or (d32,PC)
    PcRelativeDisp(i32, bool),
    /// Program counter with index: (d8,PC,Xn*scale). Fields: Xn, disp,
    /// scale, index size (`true` = `.L`, `false` = `.W`).
    PcRelativeIndex(u8, i8, u8, bool),
    /// Immediate: #expr
    Immediate(i64),
    /// Memory reference (general expression)
    Memory(i64),
    /// Address only (for branch targets, etc.)
    Address(i64),
    /// Bitfield operand: ea{offset:width} (68020+ BFxxx instructions)
    Bitfield(Box<Operand>, Box<BitfieldSpec>, Box<BitfieldSpec>),
    /// FPU data register: FP0-FP7 (68881/68882+)
    FpReg(u8),
    /// FPU control register list bitmask: FPIAR=1, FPSR=2, FPCR=4 (e.g. `fpcr/fpsr`)
    FpCtrlList(u8),
    /// A named special-purpose register not covered by a dedicated variant
    /// (e.g. MMU control registers TC/TT0/TT1/SRP/CRP/MMUSR for PMOVE).
    Special(String),
    /// A `Dh:Dl` or `Dr:Dq` register pair destination for the 64-bit forms of
    /// `MULS.L`/`MULU.L` (`Dh:Dl`) and `DIVS.L`/`DIVU.L`/`DIVSL`/`DIVUL`
    /// (`Dr:Dq`) — first field is the register written first in the syntax
    /// (`Dh`/`Dr`), second is the one written second (`Dl`/`Dq`).
    RegPair(u8, u8),
    /// 68020+ memory indirect / full-format indexing:
    /// `([bd,An],Xn,od)` (pre-indexed) or `([bd,An,Xn],od)` (post-indexed),
    /// with `An` optionally suppressed (base register absent) or replaced by
    /// `PC` — mirrors the decoder's `MemoryIndirectOperand` but without the
    /// PC-relative-target bookkeeping, which the assembler doesn't support.
    MemoryIndirect(Box<MemoryIndirectOperand>),
}

/// Parsed AST for a 68020+ memory indirect / full-format EA operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryIndirectOperand {
    /// Base register: `Some(n)` for `An`, `None` for suppressed base or `PC`.
    pub base_reg: Option<u8>,
    /// True if the base register is `PC` rather than `An` (and `base_reg` is `None`).
    pub base_is_pc: bool,
    /// Base displacement (`bd`), if present.
    pub base_disp: Option<i32>,
    /// Index register: bits 0-7 encode `Dn`, 8-15 encode `An` (matching `AddrRegIndirectIndex`).
    pub index_reg: Option<u8>,
    /// Index size: `false` = `.W`, `true` = `.L`.
    pub index_long: bool,
    /// Index scale: 1, 2, 4, or 8.
    pub index_scale: u8,
    /// Outer displacement (`od`), if present.
    pub outer_disp: Option<i32>,
    /// True for post-indexed form `([bd,An],Xn,od)` (index applied after the
    /// memory indirection); false for pre-indexed form `([bd,An,Xn],od)`
    /// (index applied before, i.e. inside the brackets).
    pub is_postindexed: bool,
}

/// Offset or width field of a bitfield operand: either a data register or a constant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BitfieldSpec {
    DataReg(u8),
    Immediate(i64),
}

impl Operand {
    pub fn is_data_reg(&self) -> bool {
        matches!(self, Operand::DataReg(_))
    }

    pub fn is_addr_reg(&self) -> bool {
        matches!(self, Operand::AddrReg(_))
    }

    pub fn reg_num(&self) -> Option<u8> {
        match self {
            Operand::DataReg(n) | Operand::AddrReg(n) => Some(*n),
            Operand::AddrRegIndirect(n)
            | Operand::AddrRegPostInc(n)
            | Operand::AddrRegPreDec(n)
            | Operand::AddrRegIndirectDisp(n, _, _) => Some(*n),
            _ => None,
        }
    }
}
