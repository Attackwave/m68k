//! Instruction encoders for flow control: BRA, BSR, Bcc, DBcc, JMP, JSR, TRAP.

use m68k_core::ea_categories::ea::*;
use m68k_core::errors::AsmError;
use m68k_core::operands::Operand;

use crate::ea_encode::encode_ea;

const COND_CODES: &[(&str, u8)] = &[
    ("t", 0),
    ("f", 1),
    ("hi", 2),
    ("ls", 3),
    ("cc", 4),
    ("cs", 5),
    ("ne", 6),
    ("eq", 7),
    ("vc", 8),
    ("vs", 9),
    ("pl", 10),
    ("mi", 11),
    ("ge", 12),
    ("lt", 13),
    ("gt", 14),
    ("le", 15),
];

fn cond_code(name: &str) -> Result<u8, AsmError> {
    for (n, c) in COND_CODES {
        if *n == name {
            return Ok(*c);
        }
    }
    Err(AsmError::new(format!("unknown condition: {}", name)))
}

/// Encode Bcc instruction.
pub fn enc_bcc(cond: &str, target: i32, pc: u32) -> Result<Vec<u16>, AsmError> {
    let cc = cond_code(cond)?;
    let disp = target.wrapping_sub(pc as i32);

    if (-127..=127).contains(&disp) {
        let op = 0x6000 | ((cc as u16) << 8) | ((disp as u8) as u16);
        Ok(vec![op])
    } else if (-32768..=32767).contains(&disp) {
        let op = 0x6000 | ((cc as u16) << 8);
        Ok(vec![op, (disp & 0xFFFF) as u16])
    } else {
        // 68020+ long displacement: low byte 0xFF marks the 32-bit form (0x00
        // marks the 16-bit form above), immediately followed by the 32-bit
        // displacement - no extra padding word. Verified against real
        // `vasm -m68020` output for `bra.l far`.
        let op = 0x6000 | ((cc as u16) << 8) | 0xFF;
        Ok(vec![
            op,
            ((disp >> 16) & 0xFFFF) as u16,
            (disp & 0xFFFF) as u16,
        ])
    }
}

/// Encode BRA instruction.
pub fn enc_bra(target: i32, pc: u32) -> Result<Vec<u16>, AsmError> {
    enc_bcc("t", target, pc)
}

/// Encode BSR instruction.
pub fn enc_bsr(target: i32, pc: u32) -> Result<Vec<u16>, AsmError> {
    let disp = target.wrapping_sub(pc as i32);

    if (-127..=127).contains(&disp) {
        let op = 0x6100 | ((disp as u8) as u16);
        Ok(vec![op])
    } else if (-32768..=32767).contains(&disp) {
        let op = 0x6100;
        Ok(vec![op, (disp & 0xFFFF) as u16])
    } else {
        // 68020+ long displacement: same 0xFF-low-byte marker as enc_bcc above.
        let op = 0x6100 | 0xFF;
        Ok(vec![
            op,
            ((disp >> 16) & 0xFFFF) as u16,
            (disp & 0xFFFF) as u16,
        ])
    }
}

/// Encode DBcc instruction.
pub fn enc_dbcc(cond: &str, reg: u8, target: i32, pc: u32) -> Result<Vec<u16>, AsmError> {
    let cc = cond_code(cond)?;
    let disp = target.wrapping_sub(pc as i32);
    if !(-32768..=32767).contains(&disp) {
        return Err(AsmError::new("DBcc displacement out of range"));
    }
    let op = 0x5100 | ((cc as u16) << 8) | 0x00C8 | (reg as u16);
    Ok(vec![op, (disp & 0xFFFF) as u16])
}

/// Encode JMP instruction.
pub fn enc_jmp(dst: &Operand, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let (mode, reg, ext) = encode_ea(dst, "w", pc, CONTROL, cpu)?;
    let op = 0x4EC0 | ((mode as u16) << 3) | (reg as u16);
    let mut words = vec![op];
    words.extend(ext);
    Ok(words)
}

/// Encode JSR instruction.
pub fn enc_jsr(dst: &Operand, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let (mode, reg, ext) = encode_ea(dst, "w", pc, CONTROL, cpu)?;
    let op = 0x4E80 | ((mode as u16) << 3) | (reg as u16);
    let mut words = vec![op];
    words.extend(ext);
    Ok(words)
}

/// Encode TRAP instruction.
pub fn enc_trap(vector: u8) -> Result<Vec<u16>, AsmError> {
    if vector > 0xF {
        return Err(AsmError::new("TRAP vector must be 0-15"));
    }
    Ok(vec![0x4E40 | (vector as u16)])
}

/// Encode TRAPV instruction.
pub fn enc_trapv() -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4E76])
}

/// Encode RTS instruction.
pub fn enc_rts() -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4E75])
}

/// Encode RTE instruction.
pub fn enc_rte() -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4E73])
}

/// Encode NOP instruction.
pub fn enc_nop() -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4E71])
}

/// Encode RTR instruction.
pub fn enc_rtr() -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4E77])
}

/// Encode RESET instruction.
pub fn enc_reset() -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4E70])
}

/// Encode STOP instruction.
pub fn enc_stop(data: u16) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4E72, data])
}

/// Encode ILLEGAL instruction.
pub fn enc_illegal() -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4AFC])
}

/// Encode LEA instruction.
pub fn enc_lea(src: &Operand, dst_reg: u8, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    // LEA allows: (An), (d16,An), (d32,An), (bd,An,Xn), ABSW, ABSL, (d16,PC), (bd,PC,Xn)
    let allowed = AREG_IND | AINDEXED | ABSW | ABSL | PCDISP | PCINDEXED;
    let (src_mode, src_reg, src_ext) = encode_ea(src, "w", pc, allowed, cpu)?;
    let op = 0x41C0 | ((dst_reg as u16) << 9) | ((src_mode as u16) << 3) | (src_reg as u16);
    let mut words = vec![op];
    words.extend(src_ext);
    Ok(words)
}

/// Encode PEA instruction.
pub fn enc_pea(src: &Operand, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let allowed = AREG_IND | AINDEXED | ABSW | ABSL | PCDISP | PCINDEXED;
    let (src_mode, src_reg, src_ext) = encode_ea(src, "w", pc, allowed, cpu)?;
    let op = 0x4840 | ((src_mode as u16) << 3) | (src_reg as u16);
    let mut words = vec![op];
    words.extend(src_ext);
    Ok(words)
}

/// Encode LINK instruction.
/// LINK.W An,#disp (16-bit displacement, all CPUs)
/// LINK.L An,#disp (32-bit displacement, 68020+)
pub fn enc_link(reg: u8, displacement: i32, size: &str, cpu: &str) -> Result<Vec<u16>, AsmError> {
    match size {
        "w" | "" => {
            let op = 0x4E50 | (reg as u16);
            Ok(vec![op, (displacement & 0xFFFF) as u16])
        }
        "l" => {
            if cpu == "68000" {
                return Err(AsmError::new("LINK.L requires 68020 or later"));
            }
            // LINK.L has its own base opcode (0x4808), unrelated to LINK.W's
            // 0x4E50 - the 32-bit displacement follows immediately, with no
            // padding word. Verified against real `vasm -m68020` output for
            // `LINK.L A5,#$12345678` -> 480D 1234 5678.
            let op = 0x4808 | (reg as u16);
            let hi = ((displacement >> 16) & 0xFFFF) as u16;
            let lo = (displacement & 0xFFFF) as u16;
            Ok(vec![op, hi, lo])
        }
        _ => Err(AsmError::new("invalid size for LINK")),
    }
}

/// Encode UNLK instruction.
pub fn enc_unlk(reg: u8) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4E58 | (reg as u16)])
}

/// Encode RTD instruction.
pub fn enc_rtd(displacement: u16) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4E74, displacement])
}

/// Encode MOVEC instruction (68010+).
///
/// MOVEC.L Dn, Cr (Dn → control register, opcode $4E7B, ext = cr|reg<<4)
/// MOVEC.L Cr, Dn (control register → Dn, opcode $4E7A, ext = cr|reg<<4)
pub fn enc_movec(src: &Operand, dst: &Operand) -> Result<Vec<u16>, AsmError> {
    // MOVEC Dn, Cr: 0x4E7B, ext = cr_number | (data_reg << 4)
    if let Operand::DataReg(rn) = src
        && let Operand::Immediate(cr) = dst
    {
        let op = 0x4E7B;
        let ext = ((*cr as u16) & 0xFFF) | ((*rn as u16) << 4);
        return Ok(vec![op, ext]);
    }

    // MOVEC Cr, Dn: 0x4E7A, ext = cr_number | (data_reg << 4)
    if let Operand::DataReg(dst_reg) = dst
        && let Operand::Immediate(cr) = src
    {
        let op = 0x4E7A;
        let ext = ((*cr as u16) & 0xFFF) | ((*dst_reg as u16) << 4);
        return Ok(vec![op, ext]);
    }

    Err(AsmError::new("MOVEC requires Dn,CR or CR,Dn operands"))
}

/// Encode BKPT instruction.
pub fn enc_bkpt(vector: u8) -> Result<Vec<u16>, AsmError> {
    if vector > 7 {
        return Err(AsmError::new("BKPT vector must be 0-7"));
    }
    Ok(vec![0x4848 | (vector as u16)])
}

/// Encode CHK instruction.
pub fn enc_chk(src: &Operand, dst_reg: u8, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let (src_mode, src_reg, src_ext) = encode_ea(src, "w", pc, DATA, cpu)?;
    let op = 0x4180 | ((dst_reg as u16) << 9) | ((src_mode as u16) << 3) | (src_reg as u16);
    let mut words = vec![op];
    words.extend(src_ext);
    Ok(words)
}

/// Encode SWAP instruction.
pub fn enc_swap(reg: u8) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x4840 | (reg as u16)])
}

/// Encode EXT instruction.
pub fn enc_ext(reg: u8, size: &str) -> Result<Vec<u16>, AsmError> {
    let ext_op = if size == "l" { 0x48C0 } else { 0x4880 };
    Ok(vec![ext_op | (reg as u16)])
}

/// Encode EXG Dd,Dd instruction.
pub fn enc_exg_dd(reg1: u8, reg2: u8) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0xC140 | ((reg1 as u16) << 9) | (reg2 as u16)])
}

/// Encode EXG An,An instruction.
pub fn enc_exg_aa(reg1: u8, reg2: u8) -> Result<Vec<u16>, AsmError> {
    // Opmode 01001, not 10001 - verified against real `vasm -m68000` output
    // for `exg a0,a1` -> 0xC149.
    Ok(vec![0xC148 | ((reg1 as u16) << 9) | (reg2 as u16)])
}

/// Encode EXG Dn,An instruction.
pub fn enc_exg_da(dreg: u8, areg: u8) -> Result<Vec<u16>, AsmError> {
    // Opmode 10001, not 01001 (that's AA's opmode) - verified against real
    // `vasm -m68000` output for `exg d2,a3` -> 0xC58B.
    Ok(vec![0xC188 | ((dreg as u16) << 9) | (areg as u16)])
}

/// Encode SBCD Dn,Dn instruction.
pub fn enc_sbcd_reg(reg1: u8, reg2: u8) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x8100 | ((reg1 as u16) << 9) | (reg2 as u16)])
}

/// Encode SBCD (An)+,(An)+ instruction.
pub fn enc_sbcd_mem(reg1: u8, reg2: u8) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x8108 | ((reg1 as u16) << 9) | (reg2 as u16)])
}

/// Encode ABCD Dn,Dn instruction.
pub fn enc_abcd_reg(reg1: u8, reg2: u8) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0xC100 | ((reg1 as u16) << 9) | (reg2 as u16)])
}

/// Encode ABCD (An)+,(An)+ instruction.
pub fn enc_abcd_mem(reg1: u8, reg2: u8) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0xC108 | ((reg1 as u16) << 9) | (reg2 as u16)])
}

/// Encode NBCD instruction.
pub fn enc_nbcd(dst: &Operand, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let allowed = if matches!(dst, Operand::DataReg(_)) {
        DREG
    } else {
        ALTERABLE_MEMORY
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, "b", pc, allowed, cpu)?;
    let op = 0x4800 | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode TAS instruction.
pub fn enc_tas(dst: &Operand, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let allowed = if matches!(dst, Operand::DataReg(_)) {
        DREG
    } else {
        ALTERABLE_MEMORY
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, "b", pc, allowed, cpu)?;
    let op = 0x4AC0 | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode NEG instruction.
pub fn enc_neg(dst: &Operand, size: &str, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    let allowed = ALTERABLE_MEMORY | DREG;
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, allowed, cpu)?;
    let op = 0x4400 | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode NEGX instruction (opcode 0x4000, EA_DATA_ALT).
pub fn enc_negx(dst: &Operand, size: &str, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    let allowed = ALTERABLE_MEMORY | DREG;
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, allowed, cpu)?;
    let op = 0x4000 | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode EXT.B instruction (68020+, opcode 0x49C0 | reg).
pub fn enc_extb(reg: u8, cpu: &str) -> Result<Vec<u16>, AsmError> {
    if cpu == "68000" {
        return Err(AsmError::new("EXTB requires 68020 or later"));
    }
    Ok(vec![0x49C0 | (reg as u16)])
}

/// Encode Scc instruction (set conditionally, opcode 0x50C0).
pub fn enc_scc(cond: &str, dst: &Operand, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    let cc = cond_code(cond)?;
    let allowed = ALTERABLE_MEMORY | DREG;
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, "b", pc, allowed, cpu)?;
    let op = 0x50C0 | ((cc as u16) << 8) | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode TRAPcc instruction (68020+, opcode 0x50F8 | (cc << 8)).
/// 0 operands: TRAPcc (implicit, word-sized trap)
/// 1 operand: TRAPcc #imm (word or long)
pub fn enc_trapcc(cond: &str, imm: Option<(i64, &str)>, cpu: &str) -> Result<Vec<u16>, AsmError> {
    if cpu == "68000" {
        return Err(AsmError::new("TRAPcc requires 68020 or later"));
    }
    let cc = cond_code(cond)?;
    let base = 0x50F8 | ((cc as u16) << 8);
    // Mode field (bits 2-0): 010=word operand, 011=long operand, 100=no operand
    // (PRM "TRAPcc" instruction format) - verified against real `vasm -m68020`
    // output for `trapeq`/`trapne.w #$1234`/`trapmi.l #$12345678`.
    match imm {
        None => Ok(vec![base | 4]),
        Some((val, "w")) | Some((val, "")) => Ok(vec![base | 2, (val & 0xFFFF) as u16]),
        Some((val, "l")) => {
            let hi = ((val >> 16) & 0xFFFF) as u16;
            let lo = (val & 0xFFFF) as u16;
            Ok(vec![base | 3, hi, lo])
        }
        _ => Err(AsmError::new("invalid size for TRAPcc")),
    }
}

/// Encode ADDQ instruction.
pub fn enc_addq(
    data: u8,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    crate::enc_math::enc_quick(data, dst, size, pc, true, cpu)
}

/// Encode SUBQ instruction.
pub fn enc_subq(
    data: u8,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    crate::enc_math::enc_quick(data, dst, size, pc, false, cpu)
}

/// Encode ADDA instruction (immediate).
pub fn enc_adda_imm(value: i64, reg: u8, size: &str) -> Result<Vec<u16>, AsmError> {
    let sz = if size == "l" { 3 } else { 1 };
    let op = 0xD0FC | ((reg as u16) << 9) | ((sz as u16) << 12);
    if size == "l" {
        Ok(vec![
            op,
            ((value >> 16) & 0xFFFF) as u16,
            (value & 0xFFFF) as u16,
        ])
    } else {
        Ok(vec![op, (value & 0xFFFF) as u16])
    }
}

/// Encode ADDA instruction (EA).
pub fn enc_adda_ea(
    src: &Operand,
    reg: u8,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = if size == "l" { 3 } else { 1 };
    let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, DATA, cpu)?;
    let op = 0xD0C0 | ((reg as u16) << 9) | ((src_mode as u16) << 3) | (src_reg as u16);
    let mut words = vec![op | ((sz as u16 - 1) << 12)];
    words.extend(src_ext);
    Ok(words)
}

/// Encode SUBA instruction (immediate).
pub fn enc_suba_imm(value: i64, reg: u8, size: &str) -> Result<Vec<u16>, AsmError> {
    let sz = if size == "l" { 3 } else { 1 };
    let op = 0x90FC | ((reg as u16) << 9) | ((sz as u16) << 12);
    if size == "l" {
        Ok(vec![
            op,
            ((value >> 16) & 0xFFFF) as u16,
            (value & 0xFFFF) as u16,
        ])
    } else {
        Ok(vec![op, (value & 0xFFFF) as u16])
    }
}

/// Encode SUBA instruction (EA).
pub fn enc_suba_ea(
    src: &Operand,
    reg: u8,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = if size == "l" { 3 } else { 1 };
    let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, DATA, cpu)?;
    let op = 0x90C0 | ((reg as u16) << 9) | ((src_mode as u16) << 3) | (src_reg as u16);
    let mut words = vec![op | ((sz as u16 - 1) << 12)];
    words.extend(src_ext);
    Ok(words)
}

/// Encode ADDI instruction.
pub fn enc_addi(
    value: i64,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALL, cpu)?;
    let op = 0x0600 | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    if size == "b" {
        words.push((value & 0xFF) as u16);
    } else if size == "w" {
        words.push((value & 0xFFFF) as u16);
    } else {
        words.push(((value >> 16) & 0xFFFF) as u16);
        words.push((value & 0xFFFF) as u16);
    }
    words.extend(dst_ext);
    Ok(words)
}

/// Encode SUBI instruction.
pub fn enc_subi(
    value: i64,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALL, cpu)?;
    let op = 0x0400 | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    if size == "b" {
        words.push((value & 0xFF) as u16);
    } else if size == "w" {
        words.push((value & 0xFFFF) as u16);
    } else {
        words.push(((value >> 16) & 0xFFFF) as u16);
        words.push((value & 0xFFFF) as u16);
    }
    words.extend(dst_ext);
    Ok(words)
}

/// Encode ANDI to CCR/SR.
pub fn enc_andi_sr(data: u16) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x023C, data])
}

/// Encode ANDI instruction.
pub fn enc_andi(
    value: i64,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALL, cpu)?;
    let op = 0x0200 | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    if size == "b" {
        words.push((value & 0xFF) as u16);
    } else if size == "w" {
        words.push((value & 0xFFFF) as u16);
    } else {
        words.push(((value >> 16) & 0xFFFF) as u16);
        words.push((value & 0xFFFF) as u16);
    }
    words.extend(dst_ext);
    Ok(words)
}

/// Encode ORI to CCR/SR.
pub fn enc_ori_sr(data: u16) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x027C, data])
}

/// Encode ORI instruction.
pub fn enc_ori(
    value: i64,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALL, cpu)?;
    let op = ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    if size == "b" {
        words.push((value & 0xFF) as u16);
    } else if size == "w" {
        words.push((value & 0xFFFF) as u16);
    } else {
        words.push(((value >> 16) & 0xFFFF) as u16);
        words.push((value & 0xFFFF) as u16);
    }
    words.extend(dst_ext);
    Ok(words)
}

/// Encode EORI to CCR/SR.
pub fn enc_eori_sr(data: u16) -> Result<Vec<u16>, AsmError> {
    Ok(vec![0x0A3C, data])
}

/// Encode EORI instruction.
pub fn enc_eori(
    value: i64,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALL, cpu)?;
    let op = 0x0A00 | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    if size == "b" {
        words.push((value & 0xFF) as u16);
    } else if size == "w" {
        words.push((value & 0xFFFF) as u16);
    } else {
        words.push(((value >> 16) & 0xFFFF) as u16);
        words.push((value & 0xFFFF) as u16);
    }
    words.extend(dst_ext);
    Ok(words)
}

/// Encode CMP instruction.
pub fn enc_cmp(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    if let Operand::DataReg(dst_reg) = dst {
        if let Operand::DataReg(src_reg) = src {
            return Ok(vec![
                0xB000 | ((sz as u16) << 6) | ((*dst_reg as u16) << 9) | (*src_reg as u16),
            ]);
        }
        let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, DATA, cpu)?;
        let mut words = vec![
            0xB000
                | ((sz as u16) << 6)
                | ((*dst_reg as u16) << 9)
                | ((src_mode as u16) << 3)
                | (src_reg as u16),
        ];
        words.extend(src_ext);
        return Ok(words);
    }
    Err(AsmError::new("CMP destination must be a data register"))
}

/// Encode CMPA instruction.
pub fn enc_cmpa(
    src: &Operand,
    reg: u8,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = if size == "l" { 3 } else { 1 };
    let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, DATA, cpu)?;
    let op = 0xB0C0 | ((reg as u16) << 9) | ((src_mode as u16) << 3) | (src_reg as u16);
    let mut words = vec![op | ((sz as u16 - 1) << 12)];
    words.extend(src_ext);
    Ok(words)
}

/// Encode CMPI instruction.
pub fn enc_cmpi(
    value: i64,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALL, cpu)?;
    let op = 0x0C00 | ((sz as u16) << 6) | ((dst_mode as u16) << 3) | (dst_reg as u16);
    let mut words = vec![op];
    if size == "b" {
        words.push((value & 0xFF) as u16);
    } else if size == "w" {
        words.push((value & 0xFFFF) as u16);
    } else {
        words.push(((value >> 16) & 0xFFFF) as u16);
        words.push((value & 0xFFFF) as u16);
    }
    words.extend(dst_ext);
    Ok(words)
}

/// Encode CMPM instruction.
pub fn enc_cmpm(reg1: u8, reg2: u8, size: &str) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    Ok(vec![
        0xB108 | ((sz as u16) << 6) | ((reg1 as u16) << 9) | (reg2 as u16),
    ])
}

/// Encode CHK2/CMP2 instruction (68020+).
pub fn enc_chk2_cmp2(
    ea: &Operand,
    reg: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
    is_chk2: bool,
) -> Result<Vec<u16>, AsmError> {
    if cpu == "68000" || cpu == "68010" {
        return Err(AsmError::new("CHK2/CMP2 requires 68020 or later"));
    }
    let size_code = match size {
        "b" => 0u16,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size for CHK2/CMP2")),
    };
    let (reg_type, reg_num) = match reg {
        Operand::DataReg(n) => (0u16, *n as u16),
        Operand::AddrReg(n) => (1u16, *n as u16),
        _ => return Err(AsmError::new("CHK2/CMP2 second operand must be Dn or An")),
    };
    let (ea_mode, ea_reg, ea_ext) = encode_ea(ea, size, pc, CONTROL, cpu)?;
    let base = 0x00C0 | (size_code << 9);
    let op = base | ((ea_mode as u16) << 3) | (ea_reg as u16);
    let ext = (reg_type << 15) | (reg_num << 12) | (if is_chk2 { 0x0800 } else { 0 });
    let mut words = vec![op, ext];
    words.extend(ea_ext);
    Ok(words)
}

/// Encode PACK/UNPK instruction (68020+).
pub fn enc_pack_unpk(
    src: &Operand,
    dst: &Operand,
    adj: &Operand,
    _size: &str,
    is_pack: bool,
) -> Result<Vec<u16>, AsmError> {
    let (type_bit, rx, ry) = match (src, dst) {
        (Operand::DataReg(sr), Operand::DataReg(dr)) => (0u16, *dr as u16, *sr as u16),
        (Operand::AddrRegPreDec(sr), Operand::AddrRegPreDec(dr)) => (1u16, *dr as u16, *sr as u16),
        _ => return Err(AsmError::new("PACK/UNPK requires Dn,Dn or -(An),-(Am)")),
    };
    let adj_val = match adj {
        Operand::Immediate(v) => *v,
        _ => return Err(AsmError::new("PACK/UNPK third operand must be immediate")),
    };
    let base: u16 = if is_pack { 0x8140 } else { 0x8180 };
    let op = base | (type_bit << 3) | (rx << 9) | ry;
    Ok(vec![op, (adj_val as u16)])
}

/// Encode RTM instruction (68020+).
pub fn enc_rtm(reg: &Operand) -> Result<Vec<u16>, AsmError> {
    match reg {
        Operand::AddrReg(n) => Ok(vec![0x06C8 | (*n as u16)]),
        Operand::DataReg(n) => Ok(vec![0x06C0 | (*n as u16)]),
        _ => Err(AsmError::new("RTM operand must be Dn or An")),
    }
}

/// Encode CALLM instruction (68020+).
pub fn enc_callm(arg: &Operand, ea: &Operand, pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    if cpu == "68000" || cpu == "68010" {
        return Err(AsmError::new("CALLM requires 68020 or later"));
    }
    let arg_val = match arg {
        Operand::Immediate(v) => *v as u8,
        _ => return Err(AsmError::new("CALLM first operand must be immediate")),
    };
    let (ea_mode, ea_reg, ea_ext) = encode_ea(ea, "b", pc, CONTROL, cpu)?;
    let op = 0x06C0 | ((ea_mode as u16) << 3) | (ea_reg as u16);
    let mut words = vec![op, ((arg_val as u16) << 8)];
    words.extend(ea_ext);
    Ok(words)
}

/// Encode MOVE16 instruction (68040+).
pub fn enc_move16(src: &Operand, dst: &Operand, _pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    if cpu != "68040" && cpu != "68060" {
        return Err(AsmError::new("MOVE16 requires 68040 or later"));
    }
    let src_is_post = matches!(src, Operand::AddrRegPostInc(_));
    let dst_is_post = matches!(dst, Operand::AddrRegPostInc(_));
    let src_is_ind = matches!(src, Operand::AddrRegIndirect(_));
    let dst_is_ind = matches!(dst, Operand::AddrRegIndirect(_));
    let src_is_abs = matches!(
        src,
        Operand::AbsoluteShort(_) | Operand::AbsoluteLong(_) | Operand::Memory(_)
    );
    let dst_is_abs = matches!(
        dst,
        Operand::AbsoluteShort(_) | Operand::AbsoluteLong(_) | Operand::Memory(_)
    );

    let (src_reg, dst_reg) = match (src, dst) {
        (Operand::AddrRegPostInc(sr), Operand::AddrRegPostInc(dr)) => (*sr, *dr),
        (Operand::AddrRegPostInc(r), _) => (*r, 0u8),
        (_, Operand::AddrRegPostInc(r)) => (0u8, *r),
        (Operand::AddrRegIndirect(r), _) => (*r, 0u8),
        (_, Operand::AddrRegIndirect(r)) => (0u8, *r),
        _ => (0, 0),
    };

    let opmode = match (
        src_is_post,
        dst_is_post,
        src_is_ind,
        dst_is_ind,
        src_is_abs,
        dst_is_abs,
    ) {
        (true, true, _, _, _, _) => 0x0020,
        (true, _, _, _, _, true) => 0x0000,
        (_, true, _, _, true, _) => 0x0008,
        (_, _, true, _, _, true) => 0x0010,
        (_, _, _, true, true, _) => 0x0018,
        _ => return Err(AsmError::new("MOVE16: unsupported operand combination")),
    };

    let base = 0xF600 | opmode;

    match (
        src_is_post,
        dst_is_post,
        src_is_ind,
        dst_is_ind,
        src_is_abs,
        dst_is_abs,
    ) {
        (true, true, _, _, _, _) => {
            // (An)+,(Am)+: extension word is `1 <Ay> 0000000000000` (PRM "MOVE16"
            // postincrement format) - bit 15 is always set, not just the register
            // field. Verified against real `vasm -m68040` output for
            // `MOVE16 (A0)+,(A1)+` -> F620 9000.
            let ext = 0x8000 | ((dst_reg as u16) << 12);
            Ok(vec![base | (src_reg as u16), ext])
        }
        (true, _, _, _, _, true) => {
            // (An)+,abs
            let addr = get_abs_addr(dst)?;
            Ok(vec![
                base | (src_reg as u16),
                (addr >> 16) as u16,
                addr as u16,
            ])
        }
        (_, true, _, _, true, _) => {
            // abs,(An)+
            let addr = get_abs_addr(src)?;
            Ok(vec![
                base | (dst_reg as u16),
                (addr >> 16) as u16,
                addr as u16,
            ])
        }
        (_, _, true, _, _, true) => {
            // (An),abs
            let addr = get_abs_addr(dst)?;
            Ok(vec![
                base | (src_reg as u16),
                (addr >> 16) as u16,
                addr as u16,
            ])
        }
        (_, _, _, true, true, _) => {
            // abs,(An)
            let addr = get_abs_addr(src)?;
            Ok(vec![
                base | (dst_reg as u16),
                (addr >> 16) as u16,
                addr as u16,
            ])
        }
        _ => Err(AsmError::new("MOVE16: unsupported operand combination")),
    }
}

fn get_abs_addr(op: &Operand) -> Result<u32, AsmError> {
    match op {
        Operand::AbsoluteShort(a) => Ok(*a as u32),
        Operand::AbsoluteLong(a) => Ok(*a as u32),
        Operand::Memory(a) => Ok(*a as u32),
        Operand::Immediate(a) => Ok(*a as u32),
        _ => Err(AsmError::new("MOVE16: invalid absolute address")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bra_byte() {
        // BRA.S label with disp=-2 (0xFE = -2)
        let words = enc_bra(0x100, 0x102).unwrap();
        assert_eq!(words, vec![0x60FE]);
    }

    #[test]
    fn test_bne_word() {
        let words = enc_bcc("ne", 0x200, 0x102).unwrap();
        assert_eq!(words, vec![0x6600, 0x00FE]);
    }

    #[test]
    fn test_bsr_byte() {
        let words = enc_bsr(0x100, 0x102).unwrap();
        assert_eq!(words, vec![0x61FE]);
    }

    #[test]
    fn test_bra_long_displacement() {
        // vasm: bra.l far (disp32=0x13886 in the corresponding assembler test) uses
        // opword 0x60FF (low byte 0xFF marks the 32-bit form) followed directly by
        // the 32-bit displacement, with no padding word.
        let words = enc_bcc("t", 0x20000, 0x1002).unwrap();
        assert_eq!(words, vec![0x60FF, 0x0001, 0xEFFE]);
    }

    #[test]
    fn test_bsr_long_displacement() {
        let words = enc_bsr(0x20000, 0x1002).unwrap();
        assert_eq!(words, vec![0x61FF, 0x0001, 0xEFFE]);
    }

    #[test]
    fn test_dbra() {
        let words = enc_dbcc("f", 0, 0x100, 0x104).unwrap();
        assert_eq!(words, vec![0x51C8, 0xFFFC]);
    }

    #[test]
    fn test_jmp_a0() {
        let words = enc_jmp(&Operand::AddrRegIndirect(0), 0, "68000").unwrap();
        assert_eq!(words, vec![0x4ED0]);
    }

    #[test]
    fn test_jsr_a0() {
        let words = enc_jsr(&Operand::AddrRegIndirect(0), 0, "68000").unwrap();
        assert_eq!(words, vec![0x4E90]);
    }

    #[test]
    fn test_trap() {
        let words = enc_trap(5).unwrap();
        assert_eq!(words, vec![0x4E45]);
    }

    #[test]
    fn test_rts() {
        let words = enc_rts().unwrap();
        assert_eq!(words, vec![0x4E75]);
    }

    #[test]
    fn test_nop() {
        let words = enc_nop().unwrap();
        assert_eq!(words, vec![0x4E71]);
    }

    #[test]
    fn test_movec_cr_to_dn() {
        let words = enc_movec(
            &Operand::Immediate(0x801), // VBR
            &Operand::DataReg(0),       // D0
        )
        .unwrap();
        assert_eq!(words, vec![0x4E7A, 0x0801]);
    }

    #[test]
    fn test_movec_dn_to_cr() {
        let words = enc_movec(
            &Operand::DataReg(0),       // D0
            &Operand::Immediate(0x002), // CACR
        )
        .unwrap();
        assert_eq!(words, vec![0x4E7B, 0x0002]);
    }

    #[test]
    fn test_link_l() {
        // vasm: LINK.L A5,#$12345678 -> 480D 1234 5678 (own base opcode 0x4808,
        // no padding word before the 32-bit displacement)
        let words = enc_link(5, 0x12345678, "l", "68020").unwrap();
        assert_eq!(words, vec![0x480D, 0x1234, 0x5678]);
    }

    #[test]
    fn test_link_w() {
        let words = enc_link(5, -16, "w", "68000").unwrap();
        // A5, disp -16 as 16-bit
        assert_eq!(words, vec![0x4E55, 0xFFF0]);
    }

    #[test]
    fn test_link_l_68000_fails() {
        assert!(enc_link(5, -16, "l", "68000").is_err());
    }

    #[test]
    fn test_negx_b_d0() {
        let words = enc_negx(&Operand::DataReg(0), "b", 0, "68000").unwrap();
        assert_eq!(words, vec![0x4000]);
    }

    #[test]
    fn test_negx_l_d0() {
        let words = enc_negx(&Operand::DataReg(0), "l", 0, "68000").unwrap();
        assert_eq!(words, vec![0x4080]);
    }

    #[test]
    fn test_extb_d0() {
        let words = enc_extb(0, "68020").unwrap();
        assert_eq!(words, vec![0x49C0]);
    }

    #[test]
    fn test_extb_68000_fails() {
        assert!(enc_extb(0, "68000").is_err());
    }

    #[test]
    fn test_scc_eq_d0() {
        let words = enc_scc("eq", &Operand::DataReg(0), 0, "68000").unwrap();
        assert_eq!(words, vec![0x57C0]);
    }

    #[test]
    fn test_scc_mi_d0() {
        let words = enc_scc("mi", &Operand::DataReg(1), 0, "68000").unwrap();
        assert_eq!(words, vec![0x5BC1]);
    }

    #[test]
    fn test_scc_gt_d0() {
        let words = enc_scc("gt", &Operand::DataReg(2), 0, "68000").unwrap();
        assert_eq!(words, vec![0x5EC2]);
    }

    #[test]
    fn test_trapcc_no_operand() {
        // vasm: trapeq -> 0x57FC
        let words = enc_trapcc("eq", None, "68020").unwrap();
        assert_eq!(words, vec![0x57FC]);
    }

    #[test]
    fn test_trapcc_word_imm() {
        // vasm: trapne.w #$1234 -> 0x56FA, 0x1234
        let words = enc_trapcc("ne", Some((0x1234, "w")), "68020").unwrap();
        assert_eq!(words, vec![0x56FA, 0x1234]);
    }

    #[test]
    fn test_trapcc_long_imm() {
        // vasm: trapmi.l #$12345678 -> 0x5BFB, 0x1234, 0x5678
        let words = enc_trapcc("mi", Some((0x12345678, "l")), "68020").unwrap();
        assert_eq!(words, vec![0x5BFB, 0x1234, 0x5678]);
    }

    #[test]
    fn test_trapcc_68000_fails() {
        assert!(enc_trapcc("eq", None, "68000").is_err());
    }

    #[test]
    fn test_exg_dd_matches_vasm() {
        // vasm: exg d0,d1 -> 0xC141
        let words = enc_exg_dd(0, 1).unwrap();
        assert_eq!(words, vec![0xC141]);
    }

    #[test]
    fn test_exg_aa_matches_vasm() {
        // vasm: exg a0,a1 -> 0xC149
        let words = enc_exg_aa(0, 1).unwrap();
        assert_eq!(words, vec![0xC149]);
    }

    #[test]
    fn test_exg_da_matches_vasm() {
        // vasm: exg d2,a3 -> 0xC58B - regression test for AA/DA opmode swap
        // (AA and DA previously shared the wrong base opcode).
        let words = enc_exg_da(2, 3).unwrap();
        assert_eq!(words, vec![0xC58B]);
    }

    #[test]
    fn test_bkpt_0() {
        let words = enc_bkpt(0).unwrap();
        assert_eq!(words, vec![0x4848]);
    }

    #[test]
    fn test_bkpt_7() {
        let words = enc_bkpt(7).unwrap();
        assert_eq!(words, vec![0x484F]);
    }

    #[test]
    fn test_bkpt_out_of_range() {
        assert!(enc_bkpt(8).is_err());
    }

    #[test]
    fn test_chk2() {
        let ea = Operand::AddrRegIndirect(0);
        let reg = Operand::DataReg(1);
        let words = enc_chk2_cmp2(&ea, &reg, "w", 0, "68020", true).unwrap();
        // base 0x00C0 | (1<<9) | (2<<3) | 0 = 0x00C0 | 0x200 | 0x10 = 0x02D0
        // ext: 0 | (1<<12) | 0x0800 = 0x1800
        assert_eq!(words, vec![0x02D0, 0x1800]);
    }

    #[test]
    fn test_cmp2() {
        let ea = Operand::AddrRegIndirect(0);
        let reg = Operand::DataReg(1);
        let words = enc_chk2_cmp2(&ea, &reg, "w", 0, "68020", false).unwrap();
        assert_eq!(words, vec![0x02D0, 0x1000]);
    }

    #[test]
    fn test_chk2_68000_fails() {
        let ea = Operand::AddrRegIndirect(0);
        let reg = Operand::DataReg(1);
        assert!(enc_chk2_cmp2(&ea, &reg, "w", 0, "68000", true).is_err());
    }

    #[test]
    fn test_pack_dn_dn() {
        let words = enc_pack_unpk(
            &Operand::DataReg(0),
            &Operand::DataReg(1),
            &Operand::Immediate(0x1234),
            "w",
            true,
        )
        .unwrap();
        assert_eq!(words, vec![0x8140 | (1 << 9), 0x1234]); // ry = D0 (bit 0)
    }

    #[test]
    fn test_unpk_dn_dn() {
        let words = enc_pack_unpk(
            &Operand::DataReg(0),
            &Operand::DataReg(1),
            &Operand::Immediate(0x5678),
            "w",
            false,
        )
        .unwrap();
        assert_eq!(words, vec![0x8180 | (1 << 9), 0x5678]); // ry = D0 (bit 0)
    }

    #[test]
    fn test_rtm_areg() {
        let words = enc_rtm(&Operand::AddrReg(3)).unwrap();
        assert_eq!(words, vec![0x06C8 | 3]);
    }

    #[test]
    fn test_rtm_dreg() {
        let words = enc_rtm(&Operand::DataReg(3)).unwrap();
        assert_eq!(words, vec![0x06C0 | 3]);
    }

    #[test]
    fn test_callm() {
        let words = enc_callm(
            &Operand::Immediate(3),
            &Operand::AddrRegIndirect(0),
            0,
            "68020",
        )
        .unwrap();
        // 0x06C0 | (2 << 3) | 0 = 0x06D0
        assert_eq!(words, vec![0x06D0, 0x0300]);
    }

    #[test]
    fn test_callm_68000_fails() {
        assert!(
            enc_callm(
                &Operand::Immediate(3),
                &Operand::AddrRegIndirect(0),
                0,
                "68000",
            )
            .is_err()
        );
    }

    #[test]
    fn test_move16_post_post() {
        // vasm: MOVE16 (A0)+,(A1)+ -> F620 9000
        let words = enc_move16(
            &Operand::AddrRegPostInc(0),
            &Operand::AddrRegPostInc(1),
            0,
            "68040",
        )
        .unwrap();
        assert_eq!(words, vec![0xF620, 0x9000]);
    }
}
