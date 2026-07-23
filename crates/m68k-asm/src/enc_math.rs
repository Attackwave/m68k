//! Instruction encoders for math operations: ADD, SUB, MUL, DIV, Quick.

use m68k_core::ea_categories::ea::*;
use m68k_core::errors::AsmError;
use m68k_core::operands::Operand;

use crate::ea_encode::encode_ea;

/// Encode an ADD instruction.
pub fn enc_add(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let mut words = Vec::new();
    let size_code = size_code(size)?;

    // Check if dst is data register (Dn to Dn form)
    if let Operand::DataReg(dst_reg) = dst
        && let Operand::DataReg(src_reg) = src
    {
        // ADD Dx, Dy: 1101 00sz 00YY YXXX
        let op = 0xD000 | ((size_code as u16) << 6) | ((*dst_reg as u16) << 9) | (*src_reg as u16);
        words.push(op);
        return Ok(words);
    }

    // EA to Dn form
    if let Operand::DataReg(dst_reg) = dst {
        let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, DATA, cpu)?;
        let op = 0xD000
            | ((size_code as u16) << 6)
            | ((*dst_reg as u16) << 9)
            | ((src_mode as u16) << 3)
            | (src_reg as u16);
        words.push(op);
        words.extend(src_ext);
        return Ok(words);
    }

    Err(AsmError::new("ADD destination must be a data register"))
}

/// Encode a SUB instruction.
pub fn enc_sub(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let mut words = Vec::new();
    let size_code = size_code(size)?;

    if let Operand::DataReg(dst_reg) = dst
        && let Operand::DataReg(src_reg) = src
    {
        let op = 0x9000 | ((size_code as u16) << 6) | ((*dst_reg as u16) << 9) | (*src_reg as u16);
        words.push(op);
        return Ok(words);
    }

    if let Operand::DataReg(dst_reg) = dst {
        let (src_mode, src_reg, src_ext) = encode_ea(src, size, pc, DATA, cpu)?;
        let op = 0x9000
            | ((size_code as u16) << 6)
            | ((*dst_reg as u16) << 9)
            | ((src_mode as u16) << 3)
            | (src_reg as u16);
        words.push(op);
        words.extend(src_ext);
        return Ok(words);
    }

    Err(AsmError::new("SUB destination must be a data register"))
}

/// Encode MULU/MULU.L. `is_signed` selects MULS.L's ext-word bit 11 when
/// `size == "l"`; shared by [`enc_mulu`] (`is_signed = false`) and
/// [`enc_muls`] (`is_signed = true`).
fn enc_mul(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
    is_signed: bool,
    mnemonic: &str,
) -> Result<Vec<u16>, AsmError> {
    match size {
        "w" => {
            let Operand::DataReg(dst_reg) = dst else {
                return Err(AsmError::new(format!(
                    "{} destination must be a data register",
                    mnemonic
                )));
            };
            let (src_mode, src_reg, src_ext) = encode_ea(src, "w", pc, DATA, cpu)?;
            let base = if is_signed { 0xC1C0 } else { 0xC0C0 };
            let op = base | ((*dst_reg as u16) << 9) | ((src_mode as u16) << 3) | (src_reg as u16);
            let mut words = vec![op];
            words.extend(src_ext);
            Ok(words)
        }
        "l" => {
            if cpu == "68000" {
                return Err(AsmError::new(format!(
                    "{}.L requires 68020 or later",
                    mnemonic
                )));
            }
            // Dn (32-bit product, dh=dl) or Dh:Dl (64-bit product).
            let (dh, dl) = match dst {
                Operand::DataReg(dn) => (*dn as u16, *dn as u16),
                Operand::RegPair(dh, dl) => (*dh as u16, *dl as u16),
                _ => {
                    return Err(AsmError::new(format!(
                        "{}.L destination must be Dn or Dh:Dl",
                        mnemonic
                    )));
                }
            };
            let is_64bit = matches!(dst, Operand::RegPair(..));
            let (src_mode, src_reg, src_ext) = encode_ea(src, "l", pc, DATA, cpu)?;
            let op = 0x4C00 | ((src_mode as u16) << 3) | (src_reg as u16);
            let ext = (dl << 12)
                | (if is_signed { 0x0800 } else { 0 })
                | (if is_64bit { 0x0400 } else { 0 })
                | dh;
            let mut words = vec![op, ext];
            words.extend(src_ext);
            Ok(words)
        }
        _ => Err(AsmError::new(format!("invalid size for {}", mnemonic))),
    }
}

/// Encode MULU/MULU.L. `MULU.L Dn` (32-bit) and `MULU.L Dh:Dl` (64-bit
/// product) are both supported for `size == "l"` on 68020+.
pub fn enc_mulu(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    enc_mul(src, dst, size, pc, cpu, false, "MULU")
}

/// Encode MULS/MULS.L. `MULS.L Dn` (32-bit) and `MULS.L Dh:Dl` (64-bit
/// product) are both supported for `size == "l"` on 68020+.
pub fn enc_muls(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    enc_mul(src, dst, size, pc, cpu, true, "MULS")
}

/// Encode DIVU/DIVU.L or DIVS/DIVS.L. `is_signed` selects DIVS's bit 11 (word
/// form base 0x81C0 vs 0x80C0) and the ext-word signed bit (long form);
/// shared by [`enc_divu`] (`is_signed = false`) and [`enc_divs`]
/// (`is_signed = true`).
fn enc_div(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
    is_signed: bool,
    mnemonic: &str,
) -> Result<Vec<u16>, AsmError> {
    match size {
        "w" => {
            let Operand::DataReg(dst_reg) = dst else {
                return Err(AsmError::new(format!(
                    "{} destination must be a data register",
                    mnemonic
                )));
            };
            let (src_mode, src_reg, src_ext) = encode_ea(src, "w", pc, DATA, cpu)?;
            let base = if is_signed { 0x81C0 } else { 0x80C0 };
            let op = base | ((*dst_reg as u16) << 9) | ((src_mode as u16) << 3) | (src_reg as u16);
            let mut words = vec![op];
            words.extend(src_ext);
            Ok(words)
        }
        "l" => {
            if cpu == "68000" {
                return Err(AsmError::new(format!(
                    "{}.L requires 68020 or later",
                    mnemonic
                )));
            }
            // Dn (32-bit quotient, dr=dq) or Dr:Dq (64-bit dividend, remainder in Dr).
            let (dr, dq) = match dst {
                Operand::DataReg(dn) => (*dn as u16, *dn as u16),
                Operand::RegPair(dr, dq) => (*dr as u16, *dq as u16),
                _ => {
                    return Err(AsmError::new(format!(
                        "{}.L destination must be Dn or Dr:Dq",
                        mnemonic
                    )));
                }
            };
            let is_64bit = matches!(dst, Operand::RegPair(..));
            let (src_mode, src_reg, src_ext) = encode_ea(src, "l", pc, DATA, cpu)?;
            let op = 0x4C40 | ((src_mode as u16) << 3) | (src_reg as u16);
            let ext = (dq << 12)
                | (if is_signed { 0x0800 } else { 0 })
                | (if is_64bit { 0x0400 } else { 0 })
                | dr;
            let mut words = vec![op, ext];
            words.extend(src_ext);
            Ok(words)
        }
        _ => Err(AsmError::new(format!("invalid size for {}", mnemonic))),
    }
}

/// Encode DIVU/DIVU.L. `DIVU.L Dn` (32-bit) and `DIVU.L Dr:Dq` (64-bit
/// dividend, remainder in Dr) are both supported for `size == "l"` on 68020+.
pub fn enc_divu(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    enc_div(src, dst, size, pc, cpu, false, "DIVU")
}

/// Encode DIVS/DIVS.L. `DIVS.L Dn` (32-bit) and `DIVS.L Dr:Dq` (64-bit
/// dividend, remainder in Dr) are both supported for `size == "l"` on 68020+.
pub fn enc_divs(
    src: &Operand,
    dst: &Operand,
    size: &str,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    enc_div(src, dst, size, pc, cpu, true, "DIVS")
}

/// Encode DIVSL/DIVUL (32-bit quotient, `Dr:Dq` remainder form required —
/// unlike DIVS.L/DIVU.L, `Dn` alone is not valid syntax for these mnemonics).
/// These never set the ext-word's 64-bit-dividend bit (bit 10) — DIVSL/DIVUL
/// always operate on a 32-bit dividend, just with the remainder captured in
/// a separate register from the quotient.
pub fn enc_divsl_ul(
    src: &Operand,
    dst: &Operand,
    pc: u32,
    cpu: &str,
    is_signed: bool,
    mnemonic: &str,
) -> Result<Vec<u16>, AsmError> {
    if cpu == "68000" || cpu == "68010" {
        return Err(AsmError::new(format!(
            "{} requires 68020 or later",
            mnemonic
        )));
    }
    let Operand::RegPair(dr, dq) = dst else {
        return Err(AsmError::new(format!(
            "{} destination must be Dr:Dq",
            mnemonic
        )));
    };
    let (src_mode, src_reg, src_ext) = encode_ea(src, "l", pc, DATA, cpu)?;
    let op = 0x4C40 | ((src_mode as u16) << 3) | (src_reg as u16);
    let ext = ((*dq as u16) << 12) | (if is_signed { 0x0800 } else { 0 }) | (*dr as u16);
    let mut words = vec![op, ext];
    words.extend(src_ext);
    Ok(words)
}

/// Encode ADDQ/SUBQ instruction.
pub fn enc_quick(
    data: u8,
    dst: &Operand,
    size: &str,
    pc: u32,
    is_add: bool,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    let size_code = size_code(size)?;
    let data_enc = if data == 8 { 0 } else { data & 0x7 };
    let base = if is_add { 0x5000 } else { 0x5100 };

    if let Operand::DataReg(dst_reg) = dst {
        let op =
            base | ((size_code as u16) << 6) | ((data_enc as u16) << 9) | ((*dst_reg as u16) << 3);
        return Ok(vec![op]);
    }

    let (dst_mode, dst_reg, dst_ext) = encode_ea(dst, size, pc, ALTERABLE_MEMORY, cpu)?;
    let op = base
        | ((size_code as u16) << 6)
        | ((data_enc as u16) << 9)
        | ((dst_mode as u16) << 3)
        | (dst_reg as u16);
    let mut words = vec![op];
    words.extend(dst_ext);
    Ok(words)
}

/// Encode ADDX/SUBX instruction.
/// ADDX Dx,Dy or SUBX Dx,Dy (register form, type_bit=0)
/// ADDX -(Ax),-(Ay) or SUBX -(Ax),-(Ay) (predecrement form, type_bit=1)
pub fn enc_addx_subx(
    mnemonic: &str,
    src: &Operand,
    dst: &Operand,
    size: &str,
) -> Result<Vec<u16>, AsmError> {
    let sz = match size {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return Err(AsmError::new("invalid size")),
    };
    let base = match mnemonic.to_uppercase().as_str() {
        "ADDX" => 0xD100,
        "SUBX" => 0x9100,
        _ => return Err(AsmError::new("invalid mnemonic for ADDX/SUBX")),
    };
    let (type_bit, ry, rx) = match (src, dst) {
        (Operand::DataReg(ry_reg), Operand::DataReg(rx_reg)) => (0, *ry_reg, *rx_reg),
        (Operand::AddrRegPreDec(ry_reg), Operand::AddrRegPreDec(rx_reg)) => (1, *ry_reg, *rx_reg),
        _ => {
            return Err(AsmError::new(
                "ADDX/SUBX operands must be both data registers or both predecrement",
            ));
        }
    };
    let op =
        base | ((sz as u16) << 6) | ((type_bit as u16) << 3) | ((rx as u16) << 9) | (ry as u16);
    Ok(vec![op])
}

fn size_code(size: &str) -> Result<u8, AsmError> {
    match size.to_lowercase().as_str() {
        "b" => Ok(0),
        "w" => Ok(1),
        "l" => Ok(2),
        _ => Err(AsmError::new("invalid size")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_b_d0_d1() {
        let words = enc_add(&Operand::DataReg(0), &Operand::DataReg(1), "b", 0, "68000").unwrap();
        assert_eq!(words, vec![0xD200]);
    }

    #[test]
    fn test_add_w_d0_d1() {
        let words = enc_add(&Operand::DataReg(0), &Operand::DataReg(1), "w", 0, "68000").unwrap();
        assert_eq!(words, vec![0xD240]);
    }

    #[test]
    fn test_add_l_d0_d1() {
        let words = enc_add(&Operand::DataReg(0), &Operand::DataReg(1), "l", 0, "68000").unwrap();
        assert_eq!(words, vec![0xD280]);
    }

    #[test]
    fn test_sub_b_d0_d1() {
        let words = enc_sub(&Operand::DataReg(0), &Operand::DataReg(1), "b", 0, "68000").unwrap();
        assert_eq!(words, vec![0x9200]);
    }

    #[test]
    fn test_addq_1_d0() {
        let words = enc_quick(1, &Operand::DataReg(0), "l", 0, true, "68000").unwrap();
        assert_eq!(words, vec![0x5280]);
    }

    #[test]
    fn test_subq_1_d0() {
        let words = enc_quick(1, &Operand::DataReg(0), "l", 0, false, "68000").unwrap();
        assert_eq!(words, vec![0x5380]);
    }

    #[test]
    fn test_mulu_l() {
        let words = enc_mulu(&Operand::DataReg(1), &Operand::DataReg(2), "l", 0, "68020").unwrap();
        // word1: 0x4C00 | (0<<3) | 1 = 0x4C01, ext: (2<<12) | 2 = 0x2002
        assert_eq!(words, vec![0x4C01, 0x2002]);
    }

    #[test]
    fn test_muls_l() {
        let words = enc_muls(&Operand::DataReg(1), &Operand::DataReg(2), "l", 0, "68020").unwrap();
        // word1: 0x4C00 | 1 = 0x4C01, ext: (2<<12) | 0x0800 | 2 = 0x2802
        assert_eq!(words, vec![0x4C01, 0x2802]);
    }

    #[test]
    fn test_mulu_l_68000_fails() {
        assert!(enc_mulu(&Operand::DataReg(1), &Operand::DataReg(2), "l", 0, "68000").is_err());
    }

    #[test]
    fn test_divu_l() {
        let words = enc_divu(&Operand::DataReg(1), &Operand::DataReg(2), "l", 0, "68020").unwrap();
        // word1: 0x4C40 | 1 = 0x4C41, ext: (2<<12) | 2 = 0x2002
        assert_eq!(words, vec![0x4C41, 0x2002]);
    }

    #[test]
    fn test_divs_l() {
        let words = enc_divs(&Operand::DataReg(1), &Operand::DataReg(2), "l", 0, "68020").unwrap();
        // word1: 0x4C40 | 1 = 0x4C41, ext: (2<<12) | 0x0800 | 2 = 0x2802
        assert_eq!(words, vec![0x4C41, 0x2802]);
    }

    #[test]
    fn test_divu_l_68000_fails() {
        assert!(enc_divu(&Operand::DataReg(1), &Operand::DataReg(2), "l", 0, "68000").is_err());
    }

    // B4: MULS.L/MULU.L Dh:Dl (64-bit product) and B3: DIVSL/DIVUL/DIVS.L/
    // DIVU.L Dr:Dq (64-bit dividend, remainder form), for D1,D3:D2 style operands.

    #[test]
    fn test_mulu_l_64bit_dh_dl() {
        // MULU.L D1,D3:D2 -> word1=0x4C01, ext=(2<<12)|0x0400|3 = 0x2403
        let words = enc_mulu(
            &Operand::DataReg(1),
            &Operand::RegPair(3, 2),
            "l",
            0,
            "68020",
        )
        .unwrap();
        assert_eq!(words, vec![0x4C01, 0x2403]);
    }

    #[test]
    fn test_muls_l_64bit_dh_dl() {
        // MULS.L D1,D3:D2 -> ext=(2<<12)|0x0800|0x0400|3 = 0x2C03
        let words = enc_muls(
            &Operand::DataReg(1),
            &Operand::RegPair(3, 2),
            "l",
            0,
            "68020",
        )
        .unwrap();
        assert_eq!(words, vec![0x4C01, 0x2C03]);
    }

    #[test]
    fn test_divu_l_64bit_dr_dq() {
        // DIVU.L D1,D3:D2 -> word1=0x4C41, ext=(2<<12)|0x0400|3 = 0x2403
        let words = enc_divu(
            &Operand::DataReg(1),
            &Operand::RegPair(3, 2),
            "l",
            0,
            "68020",
        )
        .unwrap();
        assert_eq!(words, vec![0x4C41, 0x2403]);
    }

    #[test]
    fn test_divs_l_64bit_dr_dq() {
        // DIVS.L D1,D3:D2 -> ext=(2<<12)|0x0800|0x0400|3 = 0x2C03
        let words = enc_divs(
            &Operand::DataReg(1),
            &Operand::RegPair(3, 2),
            "l",
            0,
            "68020",
        )
        .unwrap();
        assert_eq!(words, vec![0x4C41, 0x2C03]);
    }

    #[test]
    fn test_divsl_dr_dq() {
        // DIVSL D1,D3:D2 -> word1=0x4C41, ext=(2<<12)|0x0800|3 = 0x2803
        // (no bit 10, unlike DIVS.L's 64-bit form)
        let words = enc_divsl_ul(
            &Operand::DataReg(1),
            &Operand::RegPair(3, 2),
            0,
            "68020",
            true,
            "DIVSL",
        )
        .unwrap();
        assert_eq!(words, vec![0x4C41, 0x2803]);
    }

    #[test]
    fn test_divul_dr_dq() {
        // DIVUL D1,D3:D2 -> ext=(2<<12)|3 = 0x2003 (unsigned, no bit 10)
        let words = enc_divsl_ul(
            &Operand::DataReg(1),
            &Operand::RegPair(3, 2),
            0,
            "68020",
            false,
            "DIVUL",
        )
        .unwrap();
        assert_eq!(words, vec![0x4C41, 0x2003]);
    }

    #[test]
    fn test_divsl_requires_regpair() {
        // DIVSL Dn (bare data register) is not valid syntax for this mnemonic.
        assert!(
            enc_divsl_ul(
                &Operand::DataReg(1),
                &Operand::DataReg(2),
                0,
                "68020",
                true,
                "DIVSL",
            )
            .is_err()
        );
    }

    #[test]
    fn test_divsl_requires_68020() {
        assert!(
            enc_divsl_ul(
                &Operand::DataReg(1),
                &Operand::RegPair(3, 2),
                0,
                "68010",
                true,
                "DIVSL",
            )
            .is_err()
        );
    }

    #[test]
    fn test_addx_d0_d1() {
        let words = enc_addx_subx("ADDX", &Operand::DataReg(0), &Operand::DataReg(1), "l").unwrap();
        // ADDX.L D0,D1: 0xD180 | 0<<3 | 0 | 1<<9 = 0xD180 | 0x0200 = 0xD380
        // Wait: rx=dst=D1=1 (bits 11-9), ry=src=D0=0 (bits 2-0)
        // base=0xD100 | (2<<6)=0xD180 | (0<<3)=0xD180 | (1<<9)=0xD380 | 0 = 0xD380
        // Actually: bits 11-9 = rx, bits 2-0 = ry
        // op = 0xD100 | (2 << 6) | (0 << 3) | (1 << 9) | 0
        // = 0xD100 | 0x80 | 0x000 | 0x200 | 0 = 0xD380
        assert_eq!(words, vec![0xD380]);
    }

    #[test]
    fn test_subx_predec() {
        let words = enc_addx_subx(
            "SUBX",
            &Operand::AddrRegPreDec(0),
            &Operand::AddrRegPreDec(1),
            "w",
        )
        .unwrap();
        // SUBX.W -(A0),-(A1): 0x9100 | (1<<6) | (1<<3) | (1<<9) | 0
        // = 0x9100 | 0x40 | 0x08 | 0x200 | 0 = 0x9348
        assert_eq!(words, vec![0x9348]);
    }

    #[test]
    fn test_addx_invalid_operands() {
        assert!(enc_addx_subx("ADDX", &Operand::DataReg(0), &Operand::AddrReg(1), "w").is_err());
    }
}
