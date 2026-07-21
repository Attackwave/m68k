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
    let allowed = if is_data { DATA } else { ALL };
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
    let allowed = if is_data { DATA } else { ALL };
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
