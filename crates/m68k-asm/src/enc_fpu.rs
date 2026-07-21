//! FPU (68881/68882) instruction encoders.

use m68k_core::ea_categories::ea;
use m68k_core::errors::AsmError;
use m68k_core::operands::Operand;

use crate::ea_encode::encode_ea;

fn check_fpu_cpu(cpu: &str) -> Result<(), AsmError> {
    if cpu == "68000" || cpu == "68010" {
        Err(AsmError::new(
            "FPU instructions require 68020/68881 or later",
        ))
    } else {
        Ok(())
    }
}

/// FPU format code (extension word bits 12-10) for a given size suffix.
fn fmt_code(size: Option<&str>) -> u16 {
    match size.unwrap_or("x") {
        "l" => 0,
        "s" => 1,
        "x" => 2,
        "p" => 3,
        "w" => 4,
        "d" => 5,
        "b" => 6,
        _ => 2,
    }
}

/// Encode a monadic/dyadic FPU arithmetic instruction (FADD, FSQRT, FSIN, FCMP, FTST, ...).
///
/// `cmd` is the 7-bit opclass/opmode field (extension word bits 6-0).
pub fn enc_fpu_arith(
    cmd: u16,
    src: &Operand,
    dst: Option<&Operand>,
    size: Option<&str>,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;

    match (src, dst) {
        // Single FPn operand: FPn is both source and destination.
        (Operand::FpReg(fp), None) => {
            let ext = (*fp as u16) << 10 | (*fp as u16) << 7 | cmd;
            Ok(vec![0xF200, ext])
        }
        // FPU reg-reg: vasm convention "FADD FPn,FPm" computes FPn op FPm, result in FPn.
        // The extension word's dest field takes the first (src) operand's register number.
        (Operand::FpReg(fpd), Some(Operand::FpReg(fps))) => {
            let ext = (*fpd as u16) << 10 | (*fps as u16) << 7 | cmd;
            Ok(vec![0xF200, ext])
        }
        // <ea>,FPn : memory/register source, FPn destination.
        (ea_op, Some(Operand::FpReg(fpd))) => {
            let fmt = size.unwrap_or("x");
            let (mode, reg, ext_words) = encode_ea(ea_op, fmt, ext_pc, ea::DATA, cpu)?;
            let ext = (2 << 13) | (fmt_code(size) << 10) | ((*fpd as u16) << 7) | cmd;
            let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
            words.extend(ext_words);
            Ok(words)
        }
        // FPn,<ea> : FPn source, memory destination (only valid for a handful of ops, e.g. none
        // of the monadic/dyadic arith ops actually use this direction, but keep symmetry with
        // FMOVE's EA_DATA_ALT category in case callers route through here).
        (Operand::FpReg(fps), Some(ea_op)) => {
            let fmt = size.unwrap_or("x");
            let (mode, reg, ext_words) = encode_ea(ea_op, fmt, ext_pc, ea::DATA_ALT, cpu)?;
            let ext = (3 << 13) | (fmt_code(size) << 10) | ((*fps as u16) << 7) | cmd;
            let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
            words.extend(ext_words);
            Ok(words)
        }
        _ => Err(AsmError::new("FPU operand must be an fp register or EA")),
    }
}

/// Encode FMOVE: `FPn<->FPn`, `<ea><->FPn`, and FPU-control-register moves.
///
/// Bugfix vs. the Python reference: control-register moves additionally allow
/// Dn as the EA operand (`FMOVE FPIAR,D0` is a valid 68881 instruction).
pub fn enc_fmove(
    src: &Operand,
    dst: &Operand,
    size: Option<&str>,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;

    match (src, dst) {
        // FMOVE FPn,FPm: FPm -> FPn (first operand's register number is the ext word's dest field).
        (Operand::FpReg(fpd), Operand::FpReg(fps)) => {
            let ext = (*fpd as u16) << 10 | (*fps as u16) << 7;
            Ok(vec![0xF200, ext])
        }
        (ea_op, Operand::FpReg(fpd)) => {
            let fmt = size.unwrap_or("x");
            let (mode, reg, ext_words) = encode_ea(ea_op, fmt, ext_pc, ea::DATA, cpu)?;
            let ext = (2 << 13) | (fmt_code(size) << 10) | ((*fpd as u16) << 7);
            let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
            words.extend(ext_words);
            Ok(words)
        }
        (Operand::FpReg(fps), ea_op) => {
            let fmt = size.unwrap_or("x");
            let (mode, reg, ext_words) = encode_ea(ea_op, fmt, ext_pc, ea::DATA_ALT, cpu)?;
            let ext = (3 << 13) | (fmt_code(size) << 10) | ((*fps as u16) << 7);
            let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
            words.extend(ext_words);
            Ok(words)
        }
        (Operand::FpCtrlList(mask), ea_op) => {
            let (mode, reg, ext_words) = encode_ea(
                ea_op,
                "l",
                ext_pc,
                ea::CONTROL_ALT | ea::APREDEC | ea::DREG,
                cpu,
            )?;
            let ext = (4 << 13) | ((*mask as u16) << 10);
            let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
            words.extend(ext_words);
            Ok(words)
        }
        (ea_op, Operand::FpCtrlList(mask)) => {
            let (mode, reg, ext_words) = encode_ea(
                ea_op,
                "l",
                ext_pc,
                ea::CONTROL | ea::APOSTINC | ea::DREG,
                cpu,
            )?;
            let ext = (5 << 13) | ((*mask as u16) << 10);
            let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
            words.extend(ext_words);
            Ok(words)
        }
        _ => Err(AsmError::new("invalid FMOVE operands")),
    }
}

/// Encode FMOVECR #rom_offset,FPn: load a ROM constant into an FPU register.
pub fn enc_fmovecr(rom_offset: i64, fpd: u8, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;
    let ext = (2u16 << 13) | (0x7 << 10) | ((fpd as u16) << 7) | ((rom_offset as u16) & 0x7F);
    Ok(vec![0xF200, ext])
}

/// Encode FSINCOS src,FPc,FPd: computes cos into FPc and sin into FPd.
pub fn enc_fsincos(
    src: &Operand,
    cos_dst: u8,
    sin_dst: u8,
    size: Option<&str>,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;
    let cmd = 0x30 | (cos_dst & 0x7) as u16;

    match src {
        Operand::FpReg(fps) => {
            let ext = ((sin_dst as u16) << 10) | ((*fps as u16) << 7) | cmd;
            Ok(vec![0xF200, ext])
        }
        ea_op => {
            let fmt = size.unwrap_or("x");
            let (mode, reg, ext_words) = encode_ea(ea_op, fmt, ext_pc, ea::DATA, cpu)?;
            let ext = (2 << 13) | (fmt_code(size) << 10) | ((sin_dst as u16) << 7) | cmd;
            let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
            words.extend(ext_words);
            Ok(words)
        }
    }
}

/// Encode a "short" (rounding-precision-forcing) FPU instruction: FSMOVE, FSSQRT,
/// FDMOVE, FDSQRT, FSADD, FSSUB, FSMUL, FSDIV, FDADD, FDSUB, FDMUL, FDDIV.
/// Always takes `<ea>,FPn`; the EA is read as extended precision.
pub fn enc_fpu_short(
    cmd: u16,
    src: &Operand,
    fpd: u8,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;
    let (mode, reg, ext_words) = encode_ea(src, "x", ext_pc, ea::DATA, cpu)?;
    let ext = (2u16 << 13) | (0x7 << 10) | ((fpd as u16) << 7) | cmd;
    let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// A parsed FMOVEM FPU-data-register list/range, e.g. `FP0/FP2-FP4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FpRegSet(pub u8);

/// Reverse an 8-bit FPU register mask (used when the EA is not predecrement).
fn reverse_fp_mask(mask: u8) -> u8 {
    let mut out = 0u8;
    for i in 0..8 {
        if (mask >> i) & 1 != 0 {
            out |= 1 << (7 - i);
        }
    }
    out
}

/// Encode FMOVEM between a static FPU-register list and memory (register-to-memory direction).
pub fn enc_fmovem_regs_to_mem(
    regs: FpRegSet,
    dst: &Operand,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;
    let (mode, reg, ext_words) = encode_ea(dst, "x", ext_pc, ea::CONTROL_ALT | ea::APREDEC, cpu)?;
    let is_predec = mode == 4;
    let mask = if is_predec {
        regs.0
    } else {
        reverse_fp_mask(regs.0)
    };
    let ext = (6u16 << 13) | (mask as u16);
    let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// Encode FMOVEM between memory and a static FPU-register list (memory-to-register direction).
///
/// This direction is missing from the Python reference implementation (a bug); it is
/// implemented here to match the 68881 reference: the mask is used as-is (postincrement
/// reads registers in ascending order, so no bit-reversal is needed).
pub fn enc_fmovem_mem_to_regs(
    src: &Operand,
    regs: FpRegSet,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;
    let (mode, reg, ext_words) = encode_ea(src, "x", ext_pc, ea::CONTROL | ea::APOSTINC, cpu)?;
    let ext = (6u16 << 13) | (regs.0 as u16);
    let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// Encode FMOVEM between FPU control registers (FPCR/FPSR/FPIAR) and memory (ctrl-to-memory).
pub fn enc_fmovem_ctrl_to_mem(
    ctrl_mask: u8,
    dst: &Operand,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;
    let (mode, reg, ext_words) = encode_ea(dst, "l", ext_pc, ea::CONTROL_ALT | ea::APREDEC, cpu)?;
    let ext = (4u16 << 13) | ((ctrl_mask as u16) << 10);
    let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// Encode FMOVEM between memory and FPU control registers (memory-to-ctrl).
pub fn enc_fmovem_mem_to_ctrl(
    src: &Operand,
    ctrl_mask: u8,
    ext_pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;
    let (mode, reg, ext_words) = encode_ea(src, "l", ext_pc, ea::CONTROL | ea::APOSTINC, cpu)?;
    let ext = (5u16 << 13) | ((ctrl_mask as u16) << 10);
    let mut words = vec![0xF200 | ((mode as u16) << 3) | (reg as u16), ext];
    words.extend(ext_words);
    Ok(words)
}

/// Encode FBcc target: 16-bit or 32-bit PC-relative branch, condition code `cc` (0-31).
pub fn enc_fbcc(
    cc: u16,
    target: i64,
    pc_after_opword: u32,
    size: Option<&str>,
) -> Result<Vec<u16>, AsmError> {
    let disp = target - pc_after_opword as i64;
    if size == Some("l") {
        let base = 0xF2C0 | cc;
        let mut words = vec![base];
        words.push((disp >> 16) as u16);
        words.push(disp as u16);
        Ok(words)
    } else {
        if !(-32768..=32767).contains(&disp) {
            return Err(AsmError::new(format!(
                "fbcc displacement out of range: {}",
                disp
            )));
        }
        let base = 0xF280 | cc;
        Ok(vec![base, disp as u16])
    }
}

/// Encode FDBcc Dn,target: 16-bit displacement, condition code `cc` (0-31).
pub fn enc_fdbcc(cc: u16, dn: u8, target: i64, pc_after_ext: u32) -> Result<Vec<u16>, AsmError> {
    let disp = target - pc_after_ext as i64;
    if !(-32768..=32767).contains(&disp) {
        return Err(AsmError::new(format!(
            "fdbcc displacement out of range: {}",
            disp
        )));
    }
    Ok(vec![0xF2C8 | (dn as u16), cc, disp as u16])
}

/// Encode `FScc <ea>`: set byte at `ea` to all-ones/all-zeros based on FPU condition `cc`.
pub fn enc_fscc(cc: u16, dst: &Operand, ext_pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;
    let (mode, reg, ext_words) = encode_ea(dst, "b", ext_pc, ea::DATA_ALT, cpu)?;
    let mut words = vec![0xF240 | ((mode as u16) << 3) | (reg as u16), cc];
    words.extend(ext_words);
    Ok(words)
}

/// Encode FTRAPcc: trap-on-FPU-condition, with 0, 1 (word), or 2 (long) operand words.
pub fn enc_ftrapcc(cc: u16, imm: Option<(i64, &str)>) -> Result<Vec<u16>, AsmError> {
    match imm {
        None => Ok(vec![0xF278 | 2, cc]),
        Some((val, "l")) => Ok(vec![0xF278 | 4, cc, (val >> 16) as u16, val as u16]),
        Some((val, _)) => Ok(vec![0xF278 | 3, cc, val as u16]),
    }
}

/// Encode `FSAVE <ea>`: save FPU internal state (68881/68882 only, EA_CONTROL_ALT|predecrement).
pub fn enc_fsave(dst: &Operand, ext_pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;
    let (mode, reg, ext_words) = encode_ea(dst, "b", ext_pc, ea::CONTROL_ALT | ea::APREDEC, cpu)?;
    let mut words = vec![0xF300 | ((mode as u16) << 3) | (reg as u16)];
    words.extend(ext_words);
    Ok(words)
}

/// Encode `FRESTORE <ea>`: restore FPU internal state (EA_CONTROL|postincrement).
pub fn enc_frestore(src: &Operand, ext_pc: u32, cpu: &str) -> Result<Vec<u16>, AsmError> {
    check_fpu_cpu(cpu)?;
    let (mode, reg, ext_words) = encode_ea(src, "b", ext_pc, ea::CONTROL | ea::APOSTINC, cpu)?;
    let mut words = vec![0xF340 | ((mode as u16) << 3) | (reg as u16)];
    words.extend(ext_words);
    Ok(words)
}

/// Encode FNOP: a coprocessor no-op, encoded as FBF with a zero 16-bit displacement.
pub fn enc_fnop() -> Vec<u16> {
    vec![0xF280, 0x0000]
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference bytes generated from the Python assembler (source of truth):
    //   fadd fp1,fp2       -> f2000522
    //   fadd.x d0,fp2      -> f2004922
    //   fmove.s d0,fp1     -> f2004480
    //   fmove.x fp1,-(a0)  -> f2206880
    //   fmovecr #0,fp0     -> f2005c00
    //   fsincos fp1,fp2,fp3 -> f2000cb2
    //   fsqrt fp0          -> f2000004
    //   ftst fp0           -> f200003a

    #[test]
    fn test_fadd_reg_reg_matches_python() {
        // fadd fp1,fp2 -> ops[0]="fp1" is src (encodes as fpd), ops[1]="fp2" is dst (encodes as fps)
        let src = Operand::FpReg(1);
        let dst = Operand::FpReg(2);
        let words = enc_fpu_arith(0x22, &src, Some(&dst), None, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x0522]);
    }

    #[test]
    fn test_fadd_ea_to_reg_matches_python() {
        let src = Operand::DataReg(0);
        let dst = Operand::FpReg(2);
        let words = enc_fpu_arith(0x22, &src, Some(&dst), Some("x"), 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x4922]);
    }

    #[test]
    fn test_fmove_ea_to_reg_matches_python() {
        let src = Operand::DataReg(0);
        let dst = Operand::FpReg(1);
        let words = enc_fmove(&src, &dst, Some("s"), 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x4480]);
    }

    #[test]
    fn test_fmove_reg_to_ea_matches_python() {
        let src = Operand::FpReg(1);
        let dst = Operand::AddrRegPreDec(0);
        let words = enc_fmove(&src, &dst, Some("x"), 0, "68020").unwrap();
        assert_eq!(words, vec![0xF220, 0x6880]);
    }

    #[test]
    fn test_fmovecr_matches_python() {
        let words = enc_fmovecr(0, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x5C00]);
    }

    #[test]
    fn test_fsincos_reg_reg_matches_python() {
        // fsincos fp1,fp2,fp3: src=FP1, cos_dst=FP2, sin_dst=FP3
        let src = Operand::FpReg(1);
        let words = enc_fsincos(&src, 2, 3, None, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x0CB2]);
    }

    #[test]
    fn test_fsqrt_single_operand_matches_python() {
        let fp0 = Operand::FpReg(0);
        let words = enc_fpu_arith(0x04, &fp0, None, None, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x0004]);
    }

    #[test]
    fn test_ftst_single_operand_matches_python() {
        let fp0 = Operand::FpReg(0);
        let words = enc_fpu_arith(0x3A, &fp0, None, None, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x003A]);
    }

    #[test]
    fn test_fpu_requires_68020_or_later() {
        let fp0 = Operand::FpReg(0);
        assert!(enc_fpu_arith(0x04, &fp0, None, None, 0, "68000").is_err());
        assert!(enc_fmove(&fp0, &Operand::FpReg(1), None, 0, "68000").is_err());
    }

    #[test]
    fn test_fmove_ctrl_reg_to_dn_bugfix() {
        // Bugfix vs. Python: FMOVE FPIAR,D0 is a valid instruction (Dn must be allowed as EA).
        let src = Operand::FpCtrlList(1); // FPIAR
        let dst = Operand::DataReg(0);
        let words = enc_fmove(&src, &dst, None, 0, "68020").unwrap();
        assert_eq!(words[0], 0xF200);
        assert_eq!(words[1], (4u16 << 13) | (1 << 10));
    }

    #[test]
    fn test_fmove_ctrl_reg_to_predec_matches_python() {
        // fmove fpiar,-(a0) -> f2208400
        let src = Operand::FpCtrlList(1);
        let dst = Operand::AddrRegPreDec(0);
        let words = enc_fmove(&src, &dst, None, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF220, 0x8400]);
    }

    // Reference bytes for FMOVEM / short-FPU-op tests (source: Python assembler):
    //   fsmove d0,fp1              -> f2005cc0
    //   fssqrt d0,fp1              -> f2005cc1
    //   fdadd d0,fp2               -> f2005d66
    //   fmovem fp0/fp1,-(a7)       -> f227c003
    //   fmovem fp0/fp1/fp3,-(a7)   -> f227c00b
    //   fmovem fp0,-(a0)           -> f220c001
    //   fmovem (a0)+,fpcr/fpsr     -> f218b800
    //   fmovem fp0/fp1,(a0)        -> f210c0c0 (non-predecrement: mask is reversed)

    #[test]
    fn test_fsmove_matches_python() {
        let src = Operand::DataReg(0);
        let words = enc_fpu_short(0x40, &src, 1, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x5CC0]);
    }

    #[test]
    fn test_fssqrt_matches_python() {
        let src = Operand::DataReg(0);
        let words = enc_fpu_short(0x41, &src, 1, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x5CC1]);
    }

    #[test]
    fn test_fdadd_matches_python() {
        let src = Operand::DataReg(0);
        let words = enc_fpu_short(0x66, &src, 2, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x5D66]);
    }

    #[test]
    fn test_fmovem_regs_to_predec_matches_python() {
        let dst = Operand::AddrRegPreDec(7);
        let words = enc_fmovem_regs_to_mem(FpRegSet(0b0000_0011), &dst, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF227, 0xC003]);
    }

    #[test]
    fn test_fmovem_regs_range_to_predec_matches_python() {
        // fp0/fp1/fp3 -> mask 0b1011
        let dst = Operand::AddrRegPreDec(7);
        let words = enc_fmovem_regs_to_mem(FpRegSet(0b0000_1011), &dst, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF227, 0xC00B]);
    }

    #[test]
    fn test_fmovem_single_reg_to_predec_matches_python() {
        let dst = Operand::AddrRegPreDec(0);
        let words = enc_fmovem_regs_to_mem(FpRegSet(0b0000_0001), &dst, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF220, 0xC001]);
    }

    #[test]
    fn test_fmovem_regs_to_non_predec_reverses_mask() {
        // fmovem fp0/fp1,(a0) -> f210c0c0: mask 0b011 reversed to 0b11000000
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_fmovem_regs_to_mem(FpRegSet(0b0000_0011), &dst, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF210, 0xC0C0]);
    }

    #[test]
    fn test_fmovem_mem_to_ctrl_matches_python() {
        // fmovem (a0)+,fpcr/fpsr -> f218b800; fpcr=4, fpsr=2, mask=0b110
        let src = Operand::AddrRegPostInc(0);
        let words = enc_fmovem_mem_to_ctrl(&src, 0b110, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF218, 0xB800]);
    }

    #[test]
    fn test_fmovem_mem_to_regs_bugfix_not_reversed() {
        // Missing from the Python reference; postincrement reads registers in ascending
        // order so the mask must NOT be bit-reversed (unlike the non-predecrement write case).
        let src = Operand::AddrRegPostInc(0);
        let words = enc_fmovem_mem_to_regs(&src, FpRegSet(0b0000_0011), 0, "68020").unwrap();
        assert_eq!(words[1] & 0xFF, 0b0000_0011);
    }

    // Reference bytes (source: Python assembler, `org $1000` then the instruction):
    //   fbeq $1010       -> f281000e   (cc=1 "eq", pc_after_opword=$1002, disp=$0e)
    //   fbeq.l $1010     -> f2c10000000e (pc_after_opword=$1002, disp32=$0e)
    //   fdbeq d0,$1010   -> f2c80001000c (pc_after_ext=$1004, disp=$0c)
    //   fseq d0          -> f2400001
    //   fseq.b (a0)      -> f2500001
    //   ftrapeq          -> f27a0001
    //   ftrapeq.w #1234  -> f27b000104d2
    //   ftrapeq.l #12345678 -> f27c000100bc614e
    //   fnop             -> f2800000
    //   fsave -(a0)      -> f320
    //   frestore (a0)+   -> f358

    const FEQ: u16 = 1;

    #[test]
    fn test_fbeq_word_matches_python() {
        let words = enc_fbcc(FEQ, 0x1010, 0x1002, None).unwrap();
        assert_eq!(words, vec![0xF281, 0x000E]);
    }

    #[test]
    fn test_fbeq_long_matches_python() {
        let words = enc_fbcc(FEQ, 0x1010, 0x1002, Some("l")).unwrap();
        assert_eq!(words, vec![0xF2C1, 0x0000, 0x000E]);
    }

    #[test]
    fn test_fdbeq_matches_python() {
        let words = enc_fdbcc(FEQ, 0, 0x1010, 0x1004).unwrap();
        assert_eq!(words, vec![0xF2C8, 0x0001, 0x000C]);
    }

    #[test]
    fn test_fseq_dn_matches_python() {
        let dst = Operand::DataReg(0);
        let words = enc_fscc(FEQ, &dst, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF240, 0x0001]);
    }

    #[test]
    fn test_fseq_indirect_matches_python() {
        let dst = Operand::AddrRegIndirect(0);
        let words = enc_fscc(FEQ, &dst, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF250, 0x0001]);
    }

    #[test]
    fn test_ftrapeq_no_operand_matches_python() {
        let words = enc_ftrapcc(FEQ, None).unwrap();
        assert_eq!(words, vec![0xF27A, 0x0001]);
    }

    #[test]
    fn test_ftrapeq_word_matches_python() {
        let words = enc_ftrapcc(FEQ, Some((0x1234, "w"))).unwrap();
        assert_eq!(words, vec![0xF27B, 0x0001, 0x1234]);
    }

    #[test]
    fn test_ftrapeq_long_matches_python() {
        // ftrapeq.l #12345678 (decimal) -> f27c000100bc614e
        let words = enc_ftrapcc(FEQ, Some((12345678, "l"))).unwrap();
        assert_eq!(words, vec![0xF27C, 0x0001, 0x00BC, 0x614E]);
    }

    #[test]
    fn test_fnop_matches_python() {
        assert_eq!(enc_fnop(), vec![0xF280, 0x0000]);
    }

    #[test]
    fn test_fsave_matches_python() {
        let dst = Operand::AddrRegPreDec(0);
        let words = enc_fsave(&dst, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF320]);
    }

    #[test]
    fn test_frestore_matches_python() {
        let src = Operand::AddrRegPostInc(0);
        let words = enc_frestore(&src, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF358]);
    }

    #[test]
    fn test_fbcc_out_of_range_word_disp_errors() {
        assert!(enc_fbcc(FEQ, 0x20000, 0, None).is_err());
    }
}
