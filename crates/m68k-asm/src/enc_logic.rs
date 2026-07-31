//! Instruction encoders for logic operations: AND, OR, EOR, NOT, Shifts.

use m68k_core::ea_categories::ea::*;
use m68k_core::errors::AsmError;
use m68k_core::operands::Operand;

use crate::ea_encode::encode_ea;

fn size_code(size: &str) -> Result<u8, AsmError> {
    match size.to_lowercase().as_str() {
        "b" => Ok(0),
        "w" => Ok(1),
        "l" => Ok(2),
        _ => Err(AsmError::new("invalid size")),
    }
}

/// Encode AND instruction.
pub fn enc_and(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = size_code(size)?;
    if let Operand::DataReg(dst_reg) = dst {
        if let Operand::DataReg(src_reg) = src {
            return Ok(vec![
                0xC000 | ((sz as u16) << 6) | ((*dst_reg as u16) << 9) | (*src_reg as u16),
            ]);
        }
        let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, DATA, cpu)?;
        let mut words = vec![
            0xC000
                | ((sz as u16) << 6)
                | ((*dst_reg as u16) << 9)
                | ((src_mode as u16) << 3)
                | (src_reg as u16),
        ];
        words.extend(src_ext);
        return Ok(words);
    }
    if let Operand::DataReg(src_reg) = src {
        let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALTERABLE_MEMORY, cpu)?;
        let mut words = vec![
            0xC0C0
                | ((sz as u16) << 6)
                | ((*src_reg as u16) << 9)
                | ((dst_mode as u16) << 3)
                | (dst_reg as u16),
        ];
        words.extend(dst_ext);
        return Ok(words);
    }
    Err(AsmError::new("AND: one operand must be a data register"))
}

/// Encode OR instruction.
pub fn enc_or(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = size_code(size)?;
    if let Operand::DataReg(dst_reg) = dst {
        if let Operand::DataReg(src_reg) = src {
            return Ok(vec![
                0x8000 | ((sz as u16) << 6) | ((*dst_reg as u16) << 9) | (*src_reg as u16),
            ]);
        }
        let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, DATA, cpu)?;
        let mut words = vec![
            0x8000
                | ((sz as u16) << 6)
                | ((*dst_reg as u16) << 9)
                | ((src_mode as u16) << 3)
                | (src_reg as u16),
        ];
        words.extend(src_ext);
        return Ok(words);
    }
    if let Operand::DataReg(src_reg) = src {
        let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALTERABLE_MEMORY, cpu)?;
        let mut words = vec![
            0x80C0
                | ((sz as u16) << 6)
                | ((*src_reg as u16) << 9)
                | ((dst_mode as u16) << 3)
                | (dst_reg as u16),
        ];
        words.extend(dst_ext);
        return Ok(words);
    }
    Err(AsmError::new("OR: one operand must be a data register"))
}

/// Encode EOR instruction.
pub fn enc_eor(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = size_code(size)?;
    let Operand::DataReg(src_reg) = src else {
        return Err(AsmError::new("EOR source must be a data register"));
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, DREG, cpu)?;
    let mut words = vec![
        0xB100
            | ((sz as u16) << 6)
            | ((*src_reg as u16) << 9)
            | ((dst_mode as u16) << 3)
            | (dst_reg as u16),
    ];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode NOT instruction.
pub fn enc_not(dst: &Operand, size: &str, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let sz = size_code(size)?;
    let allowed = ALTERABLE_MEMORY | DREG;
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, allowed, cpu)?;
    let mut words = vec![0x4600 | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16)];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode CLR instruction.
pub fn enc_clr(dst: &Operand, size: &str, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let sz = size_code(size)?;
    let allowed = ALTERABLE_MEMORY | DREG;
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, allowed, cpu)?;
    let mut words = vec![0x4200 | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16)];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode TST instruction.
pub fn enc_tst(dst: &Operand, size: &str, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let sz = size_code(size)?;
    let allowed = DATA | DREG;
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, allowed, cpu)?;
    let mut words = vec![0x4A00 | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16)];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode ASL/ASR/LSL/LSR (register count) instruction.
pub fn enc_shift_reg(
    mnemonic: &str,
    count: u8,
    dst_reg: u8,
    size: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = size_code(size)?;
    let base = match mnemonic {
        "asl" => 0xE100,
        "asr" => 0xE000,
        "lsl" => 0xE108,
        "lsr" => 0xE008,
        "rol" => 0xE118,
        "ror" => 0xE018,
        "roxl" => 0xE110,
        "roxr" => 0xE010,
        _ => return Err(AsmError::new("unknown shift mnemonic")),
    };
    let c = if count == 8 { 0 } else { count & 0x7 };
    Ok(vec![
        base | ((sz as u16) << 6) | ((c as u16) << 9) | (dst_reg as u16),
    ])
}

/// Encode the register-count shift/rotate form `<shift>.<size> Dn,Dm`,
/// where the shift amount comes from a data register at runtime rather
/// than an immediate. Distinguished from the immediate form by bit 5
/// (i/r); the count register occupies bits 11-9, exactly where the
/// immediate count sits in [`enc_shift_reg`]. Verified against
/// reference encodings: `asr.w d1,d0` -> 0xE260.
pub fn enc_shift_reg_count(
    mnemonic: &str,
    count_reg: u8,
    dst_reg: u8,
    size: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = size_code(size)?;
    let base = match mnemonic {
        "asl" => 0xE120,
        "asr" => 0xE020,
        "lsl" => 0xE128,
        "lsr" => 0xE028,
        "rol" => 0xE138,
        "ror" => 0xE038,
        "roxl" => 0xE130,
        "roxr" => 0xE030,
        _ => return Err(AsmError::new("unknown shift mnemonic")),
    };
    Ok(vec![
        base | ((sz as u16) << 6) | ((count_reg as u16) << 9) | (dst_reg as u16),
    ])
}

/// Encode ASL/ASR/LSL/LSR (memory) instruction.
pub fn enc_shift_mem(
    mnemonic: &str,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = size_code(size)?;
    let base = match mnemonic {
        "asl" => 0xE1C0,
        "asr" => 0xE0C0,
        "lsl" => 0xE1C8,
        "lsr" => 0xE0C8,
        "rol" => 0xE1D8,
        "ror" => 0xE0D8,
        "roxl" => 0xE1D0,
        "roxr" => 0xE0D0,
        _ => return Err(AsmError::new("unknown shift mnemonic")),
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALTERABLE_MEMORY, cpu)?;
    let mut words = vec![base | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16)];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode BTST/BSET/BCLR/BCHG (register) instruction.
pub fn enc_bit_reg(
    mnemonic: &str,
    src_reg: u8,
    dst: &Operand,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let base = match mnemonic {
        "btst" => 0x0100,
        "bset" => 0x01C0,
        "bclr" => 0x0180,
        "bchg" => 0x0140,
        _ => return Err(AsmError::new("unknown bit mnemonic")),
    };
    let is_data = matches!(dst, Operand::DataReg(_));
    let size = if is_data { "l" } else { "b" };
    // Memory-form category depends on the instruction per the M68000PRM:
    // BTST's memory operand is read-only (DATA — no address-register,
    // matching alterable/non-alterable data addressing), while
    // BSET/BCLR/BCHG read-modify-write memory and so require DATA_ALT
    // (alterable data — excludes PC-relative/immediate too). The Dn case
    // above always uses `l` size regardless of mnemonic. Previously this
    // used `ALL` (0xFFFF) for every memory form, which `check_ea` treats
    // as a blanket bypass of validation entirely (see check_ea in
    // ea_encode.rs) — e.g. `BTST #1,A0` or `BSET #1,(label,PC)` silently
    // encoded a nonsensical EA instead of being rejected.
    let allowed = if is_data || mnemonic == "btst" {
        DATA
    } else {
        DATA_ALT
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, allowed, cpu)?;
    let mut words =
        vec![base | ((src_reg as u16) << 9) | ((dst_mode as u16) << 3) | (dst_reg as u16)];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode BTST/BSET/BCLR/BCHG (immediate) instruction.
pub fn enc_bit_imm(
    mnemonic: &str,
    bit: u16,
    dst: &Operand,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let base = match mnemonic {
        "btst" => 0x0800,
        "bset" => 0x08C0,
        "bclr" => 0x0880,
        "bchg" => 0x0840,
        _ => return Err(AsmError::new("unknown bit mnemonic")),
    };
    let is_data = matches!(dst, Operand::DataReg(_));
    let size = if is_data { "l" } else { "b" };
    // See enc_bit_reg above: BTST's memory operand is read-only (DATA),
    // BSET/BCLR/BCHG's is read-modify-write (DATA_ALT). Previously `ALL`
    // (0xFFFF) bypassed validation entirely for every memory form.
    let allowed = if is_data || mnemonic == "btst" {
        DATA
    } else {
        DATA_ALT
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, allowed, cpu)?;
    let mut words = vec![base | ((dst_mode as u16) << 3) | (dst_reg as u16), bit];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode CAS instruction (68020+): Compare and Swap.
pub fn enc_cas(
    dc: &Operand,
    du: &Operand,
    ea: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    if cpu == "68000" || cpu == "68010" {
        return Err(AsmError::new("CAS requires 68020 or later"));
    }
    if !matches!(dc, Operand::DataReg(_)) || !matches!(du, Operand::DataReg(_)) {
        return Err(AsmError::new(
            "CAS first two operands must be data registers",
        ));
    }
    let size_code = match size {
        "b" => 0u16,
        "w" => 1,
        "l" => 3,
        _ => return Err(AsmError::new("invalid size for CAS")),
    };
    let dc_reg = match dc {
        Operand::DataReg(r) => *r,
        _ => 0,
    };
    let du_reg = match du {
        Operand::DataReg(r) => *r,
        _ => 0,
    };
    let (ea_mode, ea_reg, ea_ext) = encode_ea(ea, size, pc, ALTERABLE_MEMORY, cpu)?;
    let base: u16 = match size_code {
        0 => 0x0AC0,
        1 => 0x0CC0,
        3 => 0x0EC0,
        _ => unreachable!(),
    };
    let op = base | ((ea_mode as u16) << 3) | (ea_reg as u16);
    let ext = ((du_reg as u16) << 6) | (dc_reg as u16);
    let mut words = vec![op, ext];
    words.extend(ea_ext);
    Ok(words)
}

/// Encode CAS2 instruction (68020+): Dual Compare and Swap.
pub fn enc_cas2(
    dc1: &Operand,
    dc2: &Operand,
    du1: &Operand,
    du2: &Operand,
    rn1: &Operand,
    rn2: &Operand,
    size: &str,
) -> Result<Vec<u16>, AsmError> {
    let size_code = match size {
        "w" => 1u16,
        "l" => 3,
        _ => return Err(AsmError::new("invalid size for CAS2")),
    };
    if !all_dreg(&[dc1, dc2, du1, du2]) {
        return Err(AsmError::new("CAS2 Dc and Du must be data registers"));
    }
    let r1_num = match rn1 {
        Operand::DataReg(r) | Operand::AddrReg(r) => *r,
        _ => return Err(AsmError::new("CAS2 Rn must be Dn or An")),
    };
    let r2_num = match rn2 {
        Operand::DataReg(r) | Operand::AddrReg(r) => *r,
        _ => return Err(AsmError::new("CAS2 Rn must be Dn or An")),
    };
    let r1_is_a = matches!(rn1, Operand::AddrReg(_));
    let r2_is_a = matches!(rn2, Operand::AddrReg(_));

    let base: u16 = match size_code {
        1 => 0x0CFC,
        3 => 0x0DFC,
        _ => unreachable!(),
    };
    let dc1_reg = match dc1 {
        Operand::DataReg(r) => *r,
        _ => 0,
    };
    let dc2_reg = match dc2 {
        Operand::DataReg(r) => *r,
        _ => 0,
    };
    let du1_reg = match du1 {
        Operand::DataReg(r) => *r,
        _ => 0,
    };
    let du2_reg = match du2 {
        Operand::DataReg(r) => *r,
        _ => 0,
    };
    let ext1 = ((r1_is_a as u16) << 15)
        | ((r1_num as u16) << 12)
        | ((du1_reg as u16) << 6)
        | (dc1_reg as u16);
    let ext2 = ((r2_is_a as u16) << 15)
        | ((r2_num as u16) << 12)
        | ((du2_reg as u16) << 6)
        | (dc2_reg as u16);
    Ok(vec![base, ext1, ext2])
}

fn all_dreg(regs: &[&Operand]) -> bool {
    regs.iter().all(|r| matches!(r, Operand::DataReg(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: BTST/BSET/BCLR/BCHG's memory-form EA category used to
    /// be `ALL` (0xFFFF), which check_ea previously treated as a blanket
    /// bypass of ea_matches entirely — so invalid destinations (An for
    /// BTST, which the PRM restricts to a read-only DATA operand; PC-
    /// relative for BSET/BCLR/BCHG, which need a writable DATA_ALT
    /// operand) were silently accepted instead of rejected.
    #[test]
    fn test_bit_ops_reject_invalid_memory_ea() {
        // BTST #imm,An: An is not a valid BTST destination.
        assert!(enc_bit_imm("btst", 1, &Operand::AddrReg(0), 0, "68000").is_err());
        // BSET Dn,(d16,PC): PC-relative is read-only, not valid for a
        // read-modify-write BSET destination.
        assert!(enc_bit_reg("bset", 0, &Operand::PcRelativeDisp(4, false), 2, "68000").is_err());
    }

    #[test]
    fn test_bit_ops_accept_valid_memory_ea() {
        // BTST #1,D0 -> 0800 0001 (data-register form, size forced to .l).
        let words = enc_bit_imm("btst", 1, &Operand::DataReg(0), 0, "68000").unwrap();
        assert_eq!(words, vec![0x0800, 0x0001]);
        // BSET #1,(A0) -> 08D0 0001.
        let words = enc_bit_imm("bset", 1, &Operand::AddrRegIndirect(0), 0, "68000").unwrap();
        assert_eq!(words, vec![0x08D0, 0x0001]);
    }

    #[test]
    fn test_and_b_d0_d1() {
        let words = enc_and(&Operand::DataReg(0), &Operand::DataReg(1), "b", 0, "68000").unwrap();
        assert_eq!(words, vec![0xC200]);
    }

    #[test]
    fn test_or_w_d0_d1() {
        let words = enc_or(&Operand::DataReg(0), &Operand::DataReg(1), "w", 0, "68000").unwrap();
        assert_eq!(words, vec![0x8240]);
    }

    #[test]
    fn test_eor_l_d0_d1() {
        let words = enc_eor(&Operand::DataReg(0), &Operand::DataReg(1), "l", 0, "68000").unwrap();
        assert_eq!(words, vec![0xB181]);
    }

    #[test]
    fn test_asl_d0() {
        let words = enc_shift_reg("asl", 1, 0, "w").unwrap();
        assert_eq!(words, vec![0xE340]);
    }

    #[test]
    fn test_lsl_d0() {
        let words = enc_shift_reg("lsl", 1, 0, "l").unwrap();
        assert_eq!(words, vec![0xE388]);
    }

    #[test]
    fn test_btst_d0_d1() {
        let words = enc_bit_reg("btst", 0, &Operand::DataReg(1), 0, "68000").unwrap();
        assert_eq!(words, vec![0x0101]);
    }
}
