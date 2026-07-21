//! Instruction encoder dispatcher - routes mnemonic + operands to encoder functions.

use m68k_core::errors::AsmError;
use m68k_core::operands::Operand;

use crate::enc_bitfield::{enc_bfinv, enc_bitfield};
use crate::enc_flow::*;
use crate::enc_fpu::*;
use crate::enc_logic::*;
use crate::enc_math::*;
use crate::enc_mmu::*;
use crate::enc_move::*;

/// FPU arithmetic-instruction opclass/opmode field (extension word bits 6-0), by mnemonic.
fn fpu_arith_cmd(mnemonic: &str) -> Option<u16> {
    Some(match mnemonic {
        "FINT" => 0x01,
        "FSINH" => 0x02,
        "FINTRZ" => 0x03,
        "FSQRT" => 0x04,
        "FLOGNP1" => 0x06,
        "FETOXM1" => 0x08,
        "FTANH" => 0x09,
        "FATAN" => 0x0A,
        "FTAN" => 0x0B,
        "FASIN" => 0x0C,
        "FATANH" => 0x0D,
        "FSIN" => 0x0E,
        "FTENTOX" => 0x0F,
        "FTWOTOX" => 0x10,
        "FETOX" => 0x11,
        "FLOG10" => 0x12,
        "FLOG2" => 0x14,
        "FLOGN" => 0x15,
        "FABS" => 0x18,
        "FCOSH" => 0x19,
        "FNEG" => 0x1A,
        "FACOS" => 0x1C,
        "FCOS" => 0x1D,
        "FGETEXP" => 0x1E,
        "FGETMAN" => 0x1F,
        "FDIV" => 0x20,
        "FMOD" => 0x21,
        "FADD" => 0x22,
        "FMUL" => 0x23,
        "FSGLDIV" => 0x24,
        "FREM" => 0x25,
        "FSCALE" => 0x26,
        "FSGLMUL" => 0x27,
        "FSUB" => 0x28,
        "FCMP" => 0x38,
        "FTST" => 0x3A,
        _ => return None,
    })
}

/// "Short" (rounding-precision-forcing) FPU instruction opclass/opmode field, by mnemonic.
fn fpu_short_cmd(mnemonic: &str) -> Option<u16> {
    Some(match mnemonic {
        "FSMOVE" => 0x40,
        "FSSQRT" => 0x41,
        "FDMOVE" => 0x44,
        "FDSQRT" => 0x45,
        "FSDIV" => 0x60,
        "FSADD" => 0x62,
        "FSMUL" => 0x63,
        "FDDIV" => 0x64,
        "FDADD" => 0x66,
        "FDMUL" => 0x67,
        "FSSUB" => 0x68,
        "FDSUB" => 0x6C,
        _ => return None,
    })
}

/// FPU condition-code value (0-31), by condition mnemonic suffix (e.g. "EQ", "OGT", "F").
fn fpu_cc(cond: &str) -> Option<u16> {
    Some(match cond {
        "F" => 0x00,
        "EQ" => 0x01,
        "OGT" => 0x02,
        "OGE" => 0x03,
        "OLT" => 0x04,
        "OLE" => 0x05,
        "OGL" => 0x06,
        "OR" => 0x07,
        "UN" => 0x08,
        "UEQ" => 0x09,
        "UGT" => 0x0A,
        "UGE" => 0x0B,
        "ULT" => 0x0C,
        "ULE" => 0x0D,
        "NE" => 0x0E,
        "T" => 0x0F,
        "SF" => 0x10,
        "SEQ" => 0x11,
        "GT" => 0x12,
        "GE" => 0x13,
        "LT" => 0x14,
        "LE" => 0x15,
        "GL" => 0x16,
        "GLE" => 0x17,
        "NGLE" => 0x18,
        "NGL" => 0x19,
        "NLE" => 0x1A,
        "NLT" => 0x1B,
        "NGE" => 0x1C,
        "NGT" => 0x1D,
        "NEQ" => 0x1E,
        "SSF" => 0x1F,
        _ => return None,
    })
}

/// Encode a 68040 cache line operation (CINVL/CINVP/CPUSHL/CPUSHP): takes `#1,An` textually,
/// but the reference encoding is fully static once family/op_type are known.
fn cache_line_op(
    family: u16,
    op_type: u16,
    src: Option<&Operand>,
    dst: Option<&Operand>,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    match (src, dst) {
        (Some(Operand::Immediate(_)), Some(Operand::AddrReg(_))) => {
            enc_cache_line_op(family, op_type, cpu)
        }
        _ => Err(AsmError::new("cache op requires #level,An")),
    }
}

/// Result of encoding a single instruction.
pub struct EncodedInstruction {
    pub words: Vec<u16>,
}

/// Encode a single instruction given mnemonic, size, operands, current PC, and CPU.
pub fn encode_instruction(
    mnemonic: &str,
    size: Option<&str>,
    src: Option<&Operand>,
    dst: Option<&Operand>,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    match mnemonic {
        // No-operand instructions
        "NOP" => enc_nop(),
        "RTS" => enc_rts(),
        "RTE" => enc_rte(),
        "RTR" => enc_rtr(),
        "TRAPV" => enc_trapv(),
        "RESET" => enc_reset(),
        "STOP" => {
            if let Some(Operand::Immediate(v)) = src {
                enc_stop(*v as u16)
            } else {
                Err(AsmError::new("STOP requires immediate operand"))
            }
        }
        "ILLEGAL" => enc_illegal(),

        // Branch instructions
        "BRA" => {
            if let Some(Operand::Address(t)) = dst {
                enc_bra(*t as i32, pc + 2)
            } else {
                Err(AsmError::new("BRA requires address operand"))
            }
        }
        "BSR" => {
            if let Some(Operand::Address(t)) = dst {
                enc_bsr(*t as i32, pc + 2)
            } else {
                Err(AsmError::new("BSR requires address operand"))
            }
        }
        "Bcc" | "BHI" | "BLS" | "BCC" | "BCS" | "BNE" | "BEQ" | "BVC" | "BVS" | "BPL" | "BMI"
        | "BGE" | "BLT" | "BGT" | "BLE" => {
            if let Some(Operand::Address(t)) = dst {
                let cond = match mnemonic {
                    "Bcc" => "cc",
                    "BHI" => "hi",
                    "BLS" => "ls",
                    "BCC" => "cc",
                    "BCS" => "cs",
                    "BNE" => "ne",
                    "BEQ" => "eq",
                    "BVC" => "vc",
                    "BVS" => "vs",
                    "BPL" => "pl",
                    "BMI" => "mi",
                    "BGE" => "ge",
                    "BLT" => "lt",
                    "BGT" => "gt",
                    "BLE" => "le",
                    _ => {
                        return Err(AsmError::new(format!(
                            "unknown branch condition: {}",
                            mnemonic
                        )));
                    }
                };
                enc_bcc(cond, *t as i32, pc + 2)
            } else {
                Err(AsmError::new("Bcc requires address operand"))
            }
        }

        // DBcc instructions
        "DBcc" | "DBT" | "DBF" | "DBHI" | "DBLS" | "DBCC" | "DBCS" | "DBNE" | "DBEQ" | "DBVC"
        | "DBVS" | "DBPL" | "DBMI" | "DBGE" | "DBLT" | "DBGT" | "DBLE" | "DBRA" => {
            if let (Some(Operand::DataReg(rn)), Some(Operand::Address(t))) = (src, dst) {
                let cond = match mnemonic {
                    "DBcc" | "DBRA" => "f",
                    "DBT" => "t",
                    "DBF" => "f",
                    "DBHI" => "hi",
                    "DBLS" => "ls",
                    "DBCC" => "cc",
                    "DBCS" => "cs",
                    "DBNE" => "ne",
                    "DBEQ" => "eq",
                    "DBVC" => "vc",
                    "DBVS" => "vs",
                    "DBPL" => "pl",
                    "DBMI" => "mi",
                    "DBGE" => "ge",
                    "DBLT" => "lt",
                    "DBGT" => "gt",
                    "DBLE" => "le",
                    _ => {
                        return Err(AsmError::new(format!(
                            "unknown DBcc condition: {}",
                            mnemonic
                        )));
                    }
                };
                enc_dbcc(cond, *rn, *t as i32, pc + 4)
            } else {
                Err(AsmError::new("DBcc requires Dn and address operands"))
            }
        }

        // TRAP
        "TRAP" => {
            if let Some(Operand::Immediate(v)) = src {
                if *v < 0 || *v > 15 {
                    return Err(AsmError::new("TRAP vector must be 0-15"));
                }
                enc_trap(*v as u8)
            } else {
                Err(AsmError::new("TRAP requires immediate vector operand"))
            }
        }

        // JMP/JSR
        "JMP" => {
            if let Some(d) = src.or(dst) {
                enc_jmp(d, pc + 2, cpu)
            } else {
                Err(AsmError::new("JMP requires destination operand"))
            }
        }
        "JSR" => {
            if let Some(d) = src.or(dst) {
                enc_jsr(d, pc + 2, cpu)
            } else {
                Err(AsmError::new("JSR requires destination operand"))
            }
        }

        // LEA/PEA
        "LEA" => {
            if let (Some(s), Some(Operand::AddrReg(rn))) = (src, dst) {
                enc_lea(s, *rn, pc + 4, cpu)
            } else {
                Err(AsmError::new("LEA requires source and An operands"))
            }
        }
        "PEA" => {
            if let Some(s) = src {
                enc_pea(s, pc + 4, cpu)
            } else {
                Err(AsmError::new("PEA requires source operand"))
            }
        }

        // LINK/UNLK
        "LINK" => {
            let sz = size.unwrap_or("w");
            if let (Some(Operand::AddrReg(rn)), Some(Operand::Immediate(disp))) = (src, dst) {
                enc_link(*rn, *disp as i32, sz, cpu)
            } else {
                Err(AsmError::new("LINK requires An and displacement"))
            }
        }
        "UNLK" => {
            if let Some(Operand::AddrReg(rn)) = src {
                enc_unlk(*rn)
            } else {
                Err(AsmError::new("UNLK requires An operand"))
            }
        }

        // RTD
        "RTD" => {
            if let Some(Operand::Immediate(disp)) = src {
                enc_rtd(*disp as u16)
            } else {
                Err(AsmError::new("RTD requires immediate displacement"))
            }
        }

        // Arithmetic
        "ADD" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d)) => enc_add(s, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("ADD requires two operands")),
            }
        }
        "SUB" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d)) => enc_sub(s, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("SUB requires two operands")),
            }
        }
        "ADDA" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(v)), Some(Operand::AddrReg(rn))) => {
                    enc_adda_imm(*v, *rn, sz)
                }
                (Some(s), Some(Operand::AddrReg(rn))) => enc_adda_ea(s, *rn, sz, pc + 4, cpu),
                _ => Err(AsmError::new("ADDA requires source and An")),
            }
        }
        "SUBA" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(v)), Some(Operand::AddrReg(rn))) => {
                    enc_suba_imm(*v, *rn, sz)
                }
                (Some(s), Some(Operand::AddrReg(rn))) => enc_suba_ea(s, *rn, sz, pc + 4, cpu),
                _ => Err(AsmError::new("SUBA requires source and An")),
            }
        }
        "ADDQ" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(n)), Some(d)) => enc_addq(*n as u8, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("ADDQ requires immediate and destination")),
            }
        }
        "SUBQ" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(n)), Some(d)) => enc_subq(*n as u8, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("SUBQ requires immediate and destination")),
            }
        }
        "MULS" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d @ (Operand::DataReg(_) | Operand::RegPair(..)))) => {
                    enc_muls(s, d, sz, pc + 4, cpu)
                }
                _ => Err(AsmError::new("MULS requires source and Dn or Dh:Dl")),
            }
        }
        "MULU" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d @ (Operand::DataReg(_) | Operand::RegPair(..)))) => {
                    enc_mulu(s, d, sz, pc + 4, cpu)
                }
                _ => Err(AsmError::new("MULU requires source and Dn or Dh:Dl")),
            }
        }
        "DIVS" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d @ (Operand::DataReg(_) | Operand::RegPair(..)))) => {
                    enc_divs(s, d, sz, pc + 4, cpu)
                }
                _ => Err(AsmError::new("DIVS requires source and Dn or Dr:Dq")),
            }
        }
        "DIVU" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d @ (Operand::DataReg(_) | Operand::RegPair(..)))) => {
                    enc_divu(s, d, sz, pc + 4, cpu)
                }
                _ => Err(AsmError::new("DIVU requires source and Dn or Dr:Dq")),
            }
        }
        "DIVSL" => match (src, dst) {
            (Some(s), Some(d @ Operand::RegPair(..))) => {
                enc_divsl_ul(s, d, pc + 4, cpu, true, "DIVSL")
            }
            _ => Err(AsmError::new("DIVSL requires source and Dr:Dq")),
        },
        "DIVUL" => match (src, dst) {
            (Some(s), Some(d @ Operand::RegPair(..))) => {
                enc_divsl_ul(s, d, pc + 4, cpu, false, "DIVUL")
            }
            _ => Err(AsmError::new("DIVUL requires source and Dr:Dq")),
        },
        "ADDX" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d)) => enc_addx_subx("ADDX", s, d, sz),
                _ => Err(AsmError::new("ADDX requires two operands")),
            }
        }
        "SUBX" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d)) => enc_addx_subx("SUBX", s, d, sz),
                _ => Err(AsmError::new("SUBX requires two operands")),
            }
        }

        // Logic
        "AND" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d)) => enc_and(s, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("AND requires two operands")),
            }
        }
        "OR" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d)) => enc_or(s, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("OR requires two operands")),
            }
        }
        "EOR" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::DataReg(rn)), Some(d)) => {
                    let src_op = Operand::DataReg(*rn);
                    enc_eor(&src_op, d, sz, pc + 4, cpu)
                }
                _ => Err(AsmError::new("EOR requires Dn and destination")),
            }
        }
        "NOT" => {
            let sz = size.unwrap_or("w");
            match src.or(dst) {
                Some(d) => enc_not(d, sz, pc + 2, cpu),
                _ => Err(AsmError::new("NOT requires destination operand")),
            }
        }
        "CLR" => {
            let sz = size.unwrap_or("w");
            match src.or(dst) {
                Some(d) => enc_clr(d, sz, pc + 2, cpu),
                _ => Err(AsmError::new("CLR requires destination operand")),
            }
        }
        "NEG" => {
            let sz = size.unwrap_or("w");
            match src.or(dst) {
                Some(d) => enc_neg(d, sz, pc + 2, cpu),
                _ => Err(AsmError::new("NEG requires destination operand")),
            }
        }
        "NEGX" => {
            let sz = size.unwrap_or("w");
            match src.or(dst) {
                Some(d) => enc_negx(d, sz, pc + 2, cpu),
                _ => Err(AsmError::new("NEGX requires destination operand")),
            }
        }
        "TST" => {
            let sz = size.unwrap_or("w");
            match src.or(dst) {
                Some(d) => enc_tst(d, sz, pc + 2, cpu),
                _ => Err(AsmError::new("TST requires destination operand")),
            }
        }
        "EXT" => {
            let sz = size.unwrap_or("w");
            match src {
                Some(Operand::DataReg(rn)) => enc_ext(*rn, sz),
                _ => match dst {
                    Some(Operand::DataReg(rn)) => enc_ext(*rn, sz),
                    _ => Err(AsmError::new("EXT requires Dn operand")),
                },
            }
        }
        "EXTB" => match src {
            Some(Operand::DataReg(rn)) => enc_extb(*rn, cpu),
            _ => match dst {
                Some(Operand::DataReg(rn)) => enc_extb(*rn, cpu),
                _ => Err(AsmError::new("EXTB requires Dn operand")),
            },
        },
        "SWAP" => match src.or(dst) {
            Some(Operand::DataReg(rn)) => enc_swap(*rn),
            _ => Err(AsmError::new("SWAP requires Dn operand")),
        },

        // Shifts
        "ASL" | "ASR" | "LSL" | "LSR" | "ROL" | "ROR" | "ROXL" | "ROXR" | "asl" | "asr" | "lsl"
        | "lsr" | "rol" | "ror" | "roxl" | "roxr" => {
            let sz = size.unwrap_or("w");
            let mn = mnemonic.to_lowercase();
            match dst {
                Some(Operand::DataReg(rn)) => {
                    if let Some(Operand::Immediate(n)) = src {
                        enc_shift_reg(&mn, *n as u8, *rn, sz)
                    } else {
                        Err(AsmError::new("Shift requires count and Dn"))
                    }
                }
                Some(d) => enc_shift_mem(&mn, d, "w", pc + 2, cpu),
                _ => Err(AsmError::new("Shift requires destination operand")),
            }
        }

        // Bit operations
        "BTST" | "BSET" | "BCLR" | "BCHG" | "btst" | "bset" | "bclr" | "bchg" => {
            let mn = mnemonic.to_lowercase();
            match (src, dst) {
                (Some(Operand::Immediate(n)), Some(d)) => {
                    enc_bit_imm(&mn, *n as u16, d, pc + 4, cpu)
                }
                (Some(Operand::DataReg(rn)), Some(d)) => enc_bit_reg(&mn, *rn, d, pc + 4, cpu),
                _ => Err(AsmError::new("Bit operation requires bit and destination")),
            }
        }

        // MOVE family
        "MOVE" => {
            let sz = size.unwrap_or("w");
            // MOVE.W SR, Dn -> from SR
            if let (Some(Operand::Immediate(-2)), Some(Operand::DataReg(rn))) = (src, dst) {
                let op = 0x40C0 | (*rn as u16);
                return Ok(vec![op]);
            }
            // MOVE.W Dn, CCR -> to CCR
            if let (Some(Operand::DataReg(rn)), Some(Operand::Immediate(-1))) = (src, dst) {
                let op = 0x44C0 | (*rn as u16);
                return Ok(vec![op]);
            }
            // MOVE.W Dn, SR -> to SR
            if let (Some(Operand::DataReg(rn)), Some(Operand::Immediate(-2))) = (src, dst) {
                let op = 0x46C0 | (*rn as u16);
                return Ok(vec![op]);
            }
            // MOVE.L An, USP -> from USP (0x4E60 | An)
            // vasm convention: MOVE An,USP = move from USP to An
            if let (Some(Operand::AddrReg(rn)), Some(Operand::Immediate(0x800))) = (src, dst) {
                return Ok(vec![0x4E60 | (*rn as u16)]);
            }
            // MOVE.L USP, An -> to USP (0x4E68 | An)
            // vasm convention: MOVE USP,An = move An to USP
            if let (Some(Operand::Immediate(0x800)), Some(Operand::AddrReg(rn))) = (src, dst) {
                return Ok(vec![0x4E68 | (*rn as u16)]);
            }
            match (src, dst) {
                (Some(s), Some(d)) => enc_move(s, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("MOVE requires source and destination")),
            }
        }
        "MOVEA" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(Operand::AddrReg(rn))) => enc_movea(s, *rn, sz, pc + 4, cpu),
                _ => Err(AsmError::new("MOVEA requires source and An")),
            }
        }
        "MOVEQ" => match (src, dst) {
            (Some(Operand::Immediate(v)), Some(Operand::DataReg(rn))) => enc_moveq(*v as i8, *rn),
            _ => Err(AsmError::new("MOVEQ requires immediate and Dn")),
        },
        "MOVEM" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(mask)), Some(d)) => {
                    enc_movem_rm(*mask as u16, d, sz, pc + 4, cpu)
                }
                (Some(s), Some(Operand::Immediate(mask))) => {
                    enc_movem_mr(s, *mask as u16, sz, pc + 4, cpu)
                }
                _ => Err(AsmError::new(
                    "MOVEM requires register mask and destination/source",
                )),
            }
        }

        // CMP family
        "CMP" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d)) => enc_cmp(s, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("CMP requires two operands")),
            }
        }
        "CMPA" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(Operand::AddrReg(rn))) => enc_cmpa(s, *rn, sz, pc + 4, cpu),
                _ => Err(AsmError::new("CMPA requires source and An")),
            }
        }
        "CMPI" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(v)), Some(d)) => enc_cmpi(*v, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("CMPI requires immediate and destination")),
            }
        }
        "CMPM" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::AddrRegPostInc(rn1)), Some(Operand::AddrRegPostInc(rn2))) => {
                    enc_cmpm(*rn1, *rn2, sz)
                }
                _ => Err(AsmError::new("CMPM requires (An)+ operands")),
            }
        }

        // Immediate instructions
        "ADDI" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(v)), Some(d)) => enc_addi(*v, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("ADDI requires immediate and destination")),
            }
        }
        "SUBI" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(v)), Some(d)) => enc_subi(*v, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("SUBI requires immediate and destination")),
            }
        }
        "ANDI" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(v)), Some(Operand::Immediate(_))) => {
                    // ANDI to CCR/SR
                    enc_andi_sr(*v as u16)
                }
                (Some(Operand::Immediate(v)), Some(d)) => enc_andi(*v, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("ANDI requires immediate and destination")),
            }
        }
        "ORI" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(v)), Some(Operand::Immediate(_))) => enc_ori_sr(*v as u16),
                (Some(Operand::Immediate(v)), Some(d)) => enc_ori(*v, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("ORI requires immediate and destination")),
            }
        }
        "EORI" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(Operand::Immediate(v)), Some(Operand::Immediate(_))) => {
                    enc_eori_sr(*v as u16)
                }
                (Some(Operand::Immediate(v)), Some(d)) => enc_eori(*v, d, sz, pc + 4, cpu),
                _ => Err(AsmError::new("EORI requires immediate and destination")),
            }
        }

        // Set conditionally (Scc)
        "ST" | "SF" | "SHI" | "SLS" | "SCC" | "SCS" | "SNE" | "SEQ" | "SVC" | "SVS" | "SPL"
        | "SMI" | "SGE" | "SLT" | "SGT" | "SLE" => {
            let cond = match mnemonic {
                "ST" => "t",
                "SF" => "f",
                "SHI" => "hi",
                "SLS" => "ls",
                "SCC" => "cc",
                "SCS" => "cs",
                "SNE" => "ne",
                "SEQ" => "eq",
                "SVC" => "vc",
                "SVS" => "vs",
                "SPL" => "pl",
                "SMI" => "mi",
                "SGE" => "ge",
                "SLT" => "lt",
                "SGT" => "gt",
                "SLE" => "le",
                _ => unreachable!(),
            };
            let d = src.or(dst);
            match d {
                Some(d) => enc_scc(cond, d, pc + 2, cpu),
                _ => Err(AsmError::new("Scc requires destination operand")),
            }
        }

        // SBCD/ABCD
        "SBCD" => match (src, dst) {
            (Some(Operand::DataReg(rn1)), Some(Operand::DataReg(rn2))) => enc_sbcd_reg(*rn1, *rn2),
            (Some(Operand::AddrRegPostInc(rn1)), Some(Operand::AddrRegPostInc(rn2))) => {
                enc_sbcd_mem(*rn1, *rn2)
            }
            _ => Err(AsmError::new("SBCD requires Dn or (An)+ operands")),
        },
        "ABCD" => match (src, dst) {
            (Some(Operand::DataReg(rn1)), Some(Operand::DataReg(rn2))) => enc_abcd_reg(*rn1, *rn2),
            (Some(Operand::AddrRegPostInc(rn1)), Some(Operand::AddrRegPostInc(rn2))) => {
                enc_abcd_mem(*rn1, *rn2)
            }
            _ => Err(AsmError::new("ABCD requires Dn or (An)+ operands")),
        },

        // NBCD/TAS
        "NBCD" => {
            let d = src.or(dst);
            match d {
                Some(d) => enc_nbcd(d, pc + 2, cpu),
                _ => Err(AsmError::new("NBCD requires destination operand")),
            }
        }
        "TAS" => {
            let d = src.or(dst);
            match d {
                Some(d) => enc_tas(d, pc + 2, cpu),
                _ => Err(AsmError::new("TAS requires destination operand")),
            }
        }

        // TRAPcc instructions (68020+)
        "TRAPT" | "TRAPF" | "TRAPHI" | "TRAPLS" | "TRAPCC" | "TRAPCS" | "TRAPNE" | "TRAPEQ"
        | "TRAPVC" | "TRAPVS" | "TRAPPL" | "TRAPMI" | "TRAPGE" | "TRAPLT" | "TRAPGT" | "TRAPLE" => {
            let cond = match mnemonic {
                "TRAPT" => "t",
                "TRAPF" => "f",
                "TRAPHI" => "hi",
                "TRAPLS" => "ls",
                "TRAPCC" => "cc",
                "TRAPCS" => "cs",
                "TRAPNE" => "ne",
                "TRAPEQ" => "eq",
                "TRAPVC" => "vc",
                "TRAPVS" => "vs",
                "TRAPPL" => "pl",
                "TRAPMI" => "mi",
                "TRAPGE" => "ge",
                "TRAPLT" => "lt",
                "TRAPGT" => "gt",
                "TRAPLE" => "le",
                _ => unreachable!(),
            };
            let imm = match (src, size) {
                (Some(Operand::Immediate(v)), sz) => Some((*v, sz.unwrap_or("w"))),
                _ => None,
            };
            enc_trapcc(cond, imm, cpu)
        }

        // MOVE to/from SR/CCR/USP
        "MOVEC" => match (src, dst) {
            (Some(s), Some(d)) => crate::enc_flow::enc_movec(s, d),
            _ => Err(AsmError::new("MOVEC requires two operands")),
        },

        // CHK
        "CHK" => match (src, dst) {
            (Some(s), Some(Operand::DataReg(rn))) => enc_chk(s, *rn, pc + 4, cpu),
            _ => Err(AsmError::new("CHK requires source and Dn")),
        },

        // EXG
        "EXG" => match (src, dst) {
            (Some(Operand::DataReg(rn1)), Some(Operand::DataReg(rn2))) => enc_exg_dd(*rn1, *rn2),
            (Some(Operand::AddrReg(rn1)), Some(Operand::AddrReg(rn2))) => enc_exg_aa(*rn1, *rn2),
            (Some(Operand::DataReg(rn1)), Some(Operand::AddrReg(rn2))) => enc_exg_da(*rn1, *rn2),
            (Some(Operand::AddrReg(rn1)), Some(Operand::DataReg(rn2))) => enc_exg_da(*rn2, *rn1),
            _ => Err(AsmError::new("EXG requires two register operands")),
        },

        // BKPT
        "BKPT" => {
            if let Some(Operand::Immediate(v)) = src {
                enc_bkpt(*v as u8)
            } else {
                Err(AsmError::new("BKPT requires immediate vector operand"))
            }
        }

        // MOVEP
        "MOVEP" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d)) => enc_movep(s, d, sz),
                _ => Err(AsmError::new("MOVEP requires two operands")),
            }
        }

        // MOVES (68010+)
        "MOVES" => {
            let sz = size.unwrap_or("w");
            match (src, dst) {
                (Some(s), Some(d)) => enc_moves(s, d, sz, pc + 2, cpu),
                _ => Err(AsmError::new("MOVES requires two operands")),
            }
        }

        // MOVE16 (68040+)
        "MOVE16" => match (src, dst) {
            (Some(s), Some(d)) => enc_move16(s, d, pc + 2, cpu),
            _ => Err(AsmError::new("MOVE16 requires two operands")),
        },

        // CHK2/CMP2 (68020+)
        "CHK2" | "CMP2" => {
            let sz = size.unwrap_or("w");
            let is_chk2 = mnemonic == "CHK2";
            match (src, dst) {
                (Some(s), Some(d)) => enc_chk2_cmp2(s, d, sz, pc + 2, cpu, is_chk2),
                _ => Err(AsmError::new(format!("{} requires <ea>,Dn/An", mnemonic))),
            }
        }

        // RTM (68020+)
        "RTM" => match src {
            Some(r) => enc_rtm(r),
            None => Err(AsmError::new("RTM requires one operand")),
        },

        // CALLM (68020+)
        "CALLM" => match (src, dst) {
            (Some(a), Some(e)) => enc_callm(a, e, pc + 2, cpu),
            _ => Err(AsmError::new("CALLM requires #arg,<ea>")),
        },

        // Bitfield instructions (68020+): BFxxx ea{offset:width}[,Dn]
        "BFTST" | "BFCHG" | "BFCLR" | "BFSET" | "BFEXTU" | "BFEXTS" | "BFFFO" => match (src, dst) {
            (Some(bf @ Operand::Bitfield(..)), Some(Operand::DataReg(rn))) => {
                enc_bitfield(mnemonic, bf, Some(*rn), pc + 2, cpu)
            }
            (Some(bf @ Operand::Bitfield(..)), None) => {
                enc_bitfield(mnemonic, bf, None, pc + 2, cpu)
            }
            _ => Err(AsmError::new(format!(
                "{} requires bitfield operand: ea{{offset:width}}[,Dn]",
                mnemonic
            ))),
        },
        // BFINS Dn,ea{offset:width} - operand order is reversed vs. the other BFxxx forms
        "BFINS" => match (src, dst) {
            (Some(Operand::DataReg(rn)), Some(bf @ Operand::Bitfield(..))) => {
                enc_bitfield("BFINS", bf, Some(*rn), pc + 2, cpu)
            }
            _ => Err(AsmError::new("BFINS requires Dn,ea{offset:width}")),
        },
        // BFINV (non-standard bitfield invert)
        "BFINV" => match src {
            Some(bf @ Operand::Bitfield(..)) => enc_bfinv(bf, pc + 2, cpu),
            _ => Err(AsmError::new(
                "BFINV requires bitfield operand: ea{offset:width}",
            )),
        },

        // FPU arithmetic instructions (68020/68881+): FADD, FSQRT, FSIN, FCMP, FTST, ...
        _ if fpu_arith_cmd(mnemonic).is_some() => {
            let cmd = fpu_arith_cmd(mnemonic).unwrap();
            match (src, dst) {
                (Some(s), None) => enc_fpu_arith(cmd, s, None, size, pc + 2, cpu),
                (Some(s), Some(d)) => enc_fpu_arith(cmd, s, Some(d), size, pc + 2, cpu),
                _ => Err(AsmError::new(format!(
                    "{} requires an fp register or EA operand",
                    mnemonic
                ))),
            }
        }

        // "Short" FPU instructions (68020/68881+): FSMOVE, FDADD, ... - always <ea>,FPn
        _ if fpu_short_cmd(mnemonic).is_some() => {
            let cmd = fpu_short_cmd(mnemonic).unwrap();
            match (src, dst) {
                (Some(s), Some(Operand::FpReg(fpd))) => enc_fpu_short(cmd, s, *fpd, pc + 2, cpu),
                _ => Err(AsmError::new(format!("{} requires <ea>,FPn", mnemonic))),
            }
        }

        // FMOVE: FPn<->FPn, <ea><->FPn, FPU control register<->EA
        "FMOVE" => match (src, dst) {
            (Some(s), Some(d)) => enc_fmove(s, d, size, pc + 2, cpu),
            _ => Err(AsmError::new("FMOVE requires two operands")),
        },

        // FMOVECR #rom_offset,FPn
        "FMOVECR" => match (src, dst) {
            (Some(Operand::Immediate(v)), Some(Operand::FpReg(fpd))) => enc_fmovecr(*v, *fpd, cpu),
            _ => Err(AsmError::new("FMOVECR requires #rom_offset,FPn")),
        },

        // FMOVEM: FPn-list<->mem, ctrl-reg-list<->mem
        "FMOVEM" => match (src, dst) {
            (Some(Operand::Immediate(mask)), Some(d)) => {
                enc_fmovem_regs_to_mem(FpRegSet(*mask as u8), d, pc + 2, cpu)
            }
            (Some(s), Some(Operand::Immediate(mask))) => {
                enc_fmovem_mem_to_regs(s, FpRegSet(*mask as u8), pc + 2, cpu)
            }
            (Some(Operand::FpCtrlList(mask)), Some(d)) => {
                enc_fmovem_ctrl_to_mem(*mask, d, pc + 2, cpu)
            }
            (Some(s), Some(Operand::FpCtrlList(mask))) => {
                enc_fmovem_mem_to_ctrl(s, *mask, pc + 2, cpu)
            }
            _ => Err(AsmError::new(
                "FMOVEM requires a register list/range and an EA operand",
            )),
        },

        // FBcc: FBEQ, FBOGT, FBUN, ... (32 condition codes)
        _ if mnemonic.starts_with("FB") && fpu_cc(&mnemonic[2..]).is_some() => {
            let cc = fpu_cc(&mnemonic[2..]).unwrap();
            match dst.or(src) {
                Some(Operand::Address(t)) => enc_fbcc(cc, *t, pc + 2, size),
                _ => Err(AsmError::new(format!(
                    "{} requires an address operand",
                    mnemonic
                ))),
            }
        }

        // FDBcc: FDBEQ, FDBOGT, ... (32 condition codes)
        _ if mnemonic.starts_with("FDB") && fpu_cc(&mnemonic[3..]).is_some() => {
            let cc = fpu_cc(&mnemonic[3..]).unwrap();
            match (src, dst) {
                (Some(Operand::DataReg(rn)), Some(Operand::Address(t))) => {
                    enc_fdbcc(cc, *rn, *t, pc + 4)
                }
                _ => Err(AsmError::new(format!(
                    "{} requires Dn and an address operand",
                    mnemonic
                ))),
            }
        }

        // FScc: FSEQ, FSOGT, ... (32 condition codes) - note: distinct from the FS<op> short-FPU
        // mnemonics (FSMOVE, FSADD, ...), which are matched earlier via fpu_short_cmd.
        _ if mnemonic.starts_with("FS") && fpu_cc(&mnemonic[2..]).is_some() => {
            let cc = fpu_cc(&mnemonic[2..]).unwrap();
            match src.or(dst) {
                Some(d) => enc_fscc(cc, d, pc + 2, cpu),
                _ => Err(AsmError::new(format!(
                    "{} requires a destination operand",
                    mnemonic
                ))),
            }
        }

        // FTRAPcc: FTRAPEQ, FTRAPOGT, ... (32 condition codes)
        _ if mnemonic.starts_with("FTRAP") && fpu_cc(&mnemonic[5..]).is_some() => {
            let cc = fpu_cc(&mnemonic[5..]).unwrap();
            let imm = match (src, size) {
                (Some(Operand::Immediate(v)), sz) => Some((*v, sz.unwrap_or("w"))),
                _ => None,
            };
            enc_ftrapcc(cc, imm)
        }

        // FSAVE / FRESTORE
        "FSAVE" => match src.or(dst) {
            Some(d) => enc_fsave(d, pc + 2, cpu),
            _ => Err(AsmError::new("FSAVE requires a destination operand")),
        },
        "FRESTORE" => match src.or(dst) {
            Some(s) => enc_frestore(s, pc + 2, cpu),
            _ => Err(AsmError::new("FRESTORE requires a source operand")),
        },

        // FNOP
        "FNOP" => Ok(enc_fnop()),

        // PMOVE <ea>,MMUreg or PMOVE MMUreg,<ea> (68030+)
        "PMOVE" => match (src, dst) {
            (Some(mmu @ (Operand::Special(_) | Operand::Immediate(_))), Some(ea_op)) => {
                enc_pmove(ea_op, mmu, false, pc + 2, cpu)
            }
            (Some(ea_op), Some(mmu @ (Operand::Special(_) | Operand::Immediate(_)))) => {
                enc_pmove(ea_op, mmu, true, pc + 2, cpu)
            }
            _ => Err(AsmError::new(
                "PMOVE requires one MMU register and one EA operand",
            )),
        },

        // PTEST #level,<ea> (68030+)
        "PTEST" => match (src, dst) {
            (Some(Operand::Immediate(level)), Some(ea_op)) => enc_ptest(*level, ea_op, pc + 2, cpu),
            _ => Err(AsmError::new("PTEST requires #level,<ea>")),
        },

        "PFLUSHA" => enc_pflusha(cpu),
        "PFLUSHAN" => enc_pflushan(cpu),

        "PTESTW" => match src.or(dst) {
            Some(d) => enc_mmu_single_reg(0xF548, d, cpu),
            _ => Err(AsmError::new("PTESTW requires An or (An)")),
        },
        "PTESTR" => match src.or(dst) {
            Some(d) => enc_mmu_single_reg(0xF568, d, cpu),
            _ => Err(AsmError::new("PTESTR requires An or (An)")),
        },
        "PFLUSHN" => match src.or(dst) {
            Some(d) => enc_mmu_single_reg(0xF518, d, cpu),
            _ => Err(AsmError::new("PFLUSHN requires An or (An)")),
        },
        "PLPAW" => match src.or(dst) {
            Some(d) => enc_mmu_single_reg(0xF588, d, cpu),
            _ => Err(AsmError::new("PLPAW requires An or (An)")),
        },
        "PLPAR" => match src.or(dst) {
            Some(d) => enc_mmu_single_reg(0xF5C8, d, cpu),
            _ => Err(AsmError::new("PLPAR requires An or (An)")),
        },

        // LPSTOP #data (68060)
        "LPSTOP" => match src {
            Some(Operand::Immediate(v)) => enc_lpstop(*v, cpu),
            _ => Err(AsmError::new("LPSTOP requires an immediate operand")),
        },

        // Cache line ops (68040): CINVL/CINVP/CPUSHL/CPUSHP take #1,An but the encoding is
        // fully static once family/op_type are known, so the operands are accepted but unused
        // beyond validating arity (matches the Python reference).
        "CINVL" => cache_line_op(1, 0, src, dst, cpu),
        "CINVP" => cache_line_op(1, 1, src, dst, cpu),
        "CPUSHL" => cache_line_op(3, 0, src, dst, cpu),
        "CPUSHP" => cache_line_op(3, 1, src, dst, cpu),

        "CINVA" => match src {
            Some(a) => enc_cinva(a, cpu),
            _ => Err(AsmError::new("CINVA requires An")),
        },
        "CPUSHA" => match src {
            Some(a) => enc_cpusha(a, cpu),
            _ => Err(AsmError::new("CPUSHA requires An")),
        },
        "CPUSH" => match (src, dst) {
            (Some(Operand::Immediate(level)), Some(a)) => enc_cpush(*level, a, cpu),
            _ => Err(AsmError::new("CPUSH requires #level,An")),
        },
        "CINV" => match (src, dst) {
            (Some(Operand::Immediate(level)), Some(a)) => enc_cinv(*level, a, cpu),
            _ => Err(AsmError::new("CINV requires #level,An")),
        },

        // PSAVE / PRESTORE (68030+)
        "PSAVE" => match src.or(dst) {
            Some(d) => enc_psave(d, pc + 2, cpu),
            _ => Err(AsmError::new("PSAVE requires a destination operand")),
        },
        "PRESTORE" => match src.or(dst) {
            Some(s) => enc_prestore(s, pc + 2, cpu),
            _ => Err(AsmError::new("PRESTORE requires a source operand")),
        },

        // FSINCOS/PFLUSH/CAS/PACK/UNPK take 3 operands and must go through
        // encode_instruction_ex instead, since this function only carries two.
        "FSINCOS" => Err(AsmError::new(
            "FSINCOS requires 3 operands (src,FPcos,FPsin); use encode_instruction_ex",
        )),
        "PFLUSH" => Err(AsmError::new(
            "PFLUSH requires 3 operands (#fc,#mask,<ea>); use encode_instruction_ex",
        )),
        // CAS/CAS2/PACK/UNPK (68020+) are handled directly in assembler.rs's pass-2 dispatch
        // (they need raw operand text, or 3+ operands not representable by this signature).

        // Unknown
        _ => Err(AsmError::new(format!("unknown mnemonic: {}", mnemonic))),
    }
}

/// Encode a single instruction that may need a third operand (currently only FSINCOS).
/// Delegates to [`encode_instruction`] for every other mnemonic.
pub fn encode_instruction_ex(
    mnemonic: &str,
    size: Option<&str>,
    src: Option<&Operand>,
    dst: Option<&Operand>,
    extra: Option<&Operand>,
    pc: u32,
    cpu: &str,
) -> Result<Vec<u16>, AsmError> {
    if mnemonic == "FSINCOS" {
        return match (src, dst, extra) {
            (Some(s), Some(Operand::FpReg(cos_dst)), Some(Operand::FpReg(sin_dst))) => {
                enc_fsincos(s, *cos_dst, *sin_dst, size, pc + 2, cpu)
            }
            _ => Err(AsmError::new("FSINCOS requires src,FPcos,FPsin")),
        };
    }
    if mnemonic == "PFLUSH" {
        return match (src, dst, extra) {
            (Some(Operand::Immediate(fc)), Some(Operand::Immediate(mask)), Some(ea_op)) => {
                enc_pflush(*fc, *mask, ea_op, pc + 2, cpu)
            }
            _ => Err(AsmError::new("PFLUSH requires #fc,#mask,<ea>")),
        };
    }
    encode_instruction(mnemonic, size, src, dst, pc, cpu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use m68k_core::operands::BitfieldSpec;

    fn words_to_bytes(words: &[u16]) -> String {
        words
            .iter()
            .flat_map(|w| w.to_be_bytes())
            .map(|b| format!("{:02x}", b))
            .collect()
    }

    // Reference bytes generated from the Python assembler (source of truth):
    //   bftst d0{4:8}          -> e8c00108
    //   bfextu d1{d2:d3},d4    -> e9c1e818
    //   bfins d0,d1{4:8}       -> efc10108
    //   bfset (a0){0:32}       -> eed00000
    //   bfinv (a0){0:8}        -> efd00028
    //   bfffo d2{8:d5},d3      -> edc23a28
    //   bfchg d1{d2:16}        -> eac1a010

    #[test]
    fn test_bftst_matches_python_reference() {
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(0)),
            Box::new(BitfieldSpec::Immediate(4)),
            Box::new(BitfieldSpec::Immediate(8)),
        );
        let words = encode_instruction("BFTST", None, Some(&bf), None, 0, "68020").unwrap();
        assert_eq!(words_to_bytes(&words), "e8c00108");
    }

    #[test]
    fn test_bfextu_matches_python_reference() {
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(1)),
            Box::new(BitfieldSpec::DataReg(2)),
            Box::new(BitfieldSpec::DataReg(3)),
        );
        let dst = Operand::DataReg(4);
        let words = encode_instruction("BFEXTU", None, Some(&bf), Some(&dst), 0, "68020").unwrap();
        assert_eq!(words_to_bytes(&words), "e9c1e818");
    }

    #[test]
    fn test_bfins_matches_python_reference() {
        let src = Operand::DataReg(0);
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(1)),
            Box::new(BitfieldSpec::Immediate(4)),
            Box::new(BitfieldSpec::Immediate(8)),
        );
        let words = encode_instruction("BFINS", None, Some(&src), Some(&bf), 0, "68020").unwrap();
        assert_eq!(words_to_bytes(&words), "efc10108");
    }

    #[test]
    fn test_bfset_matches_python_reference() {
        let bf = Operand::Bitfield(
            Box::new(Operand::AddrRegIndirect(0)),
            Box::new(BitfieldSpec::Immediate(0)),
            Box::new(BitfieldSpec::Immediate(32)),
        );
        let words = encode_instruction("BFSET", None, Some(&bf), None, 0, "68020").unwrap();
        assert_eq!(words_to_bytes(&words), "eed00000");
    }

    #[test]
    fn test_bfinv_matches_python_reference() {
        let bf = Operand::Bitfield(
            Box::new(Operand::AddrRegIndirect(0)),
            Box::new(BitfieldSpec::Immediate(0)),
            Box::new(BitfieldSpec::Immediate(8)),
        );
        let words = encode_instruction("BFINV", None, Some(&bf), None, 0, "68020").unwrap();
        assert_eq!(words_to_bytes(&words), "efd00028");
    }

    #[test]
    fn test_bfffo_matches_python_reference() {
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(2)),
            Box::new(BitfieldSpec::Immediate(8)),
            Box::new(BitfieldSpec::DataReg(5)),
        );
        let dst = Operand::DataReg(3);
        let words = encode_instruction("BFFFO", None, Some(&bf), Some(&dst), 0, "68020").unwrap();
        assert_eq!(words_to_bytes(&words), "edc23a28");
    }

    #[test]
    fn test_bfchg_matches_python_reference() {
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(1)),
            Box::new(BitfieldSpec::DataReg(2)),
            Box::new(BitfieldSpec::Immediate(16)),
        );
        let words = encode_instruction("BFCHG", None, Some(&bf), None, 0, "68020").unwrap();
        assert_eq!(words_to_bytes(&words), "eac1a010");
    }

    #[test]
    fn test_bftst_68000_fails() {
        let bf = Operand::Bitfield(
            Box::new(Operand::DataReg(0)),
            Box::new(BitfieldSpec::Immediate(0)),
            Box::new(BitfieldSpec::Immediate(8)),
        );
        assert!(encode_instruction("BFTST", None, Some(&bf), None, 0, "68000").is_err());
    }

    #[test]
    fn test_encode_nop() {
        let words = encode_instruction("NOP", None, None, None, 0, "68000").unwrap();
        assert_eq!(words, vec![0x4E71]);
    }

    #[test]
    fn test_encode_add_b() {
        let src = Operand::DataReg(0);
        let dst = Operand::DataReg(1);
        let words =
            encode_instruction("ADD", Some("b"), Some(&src), Some(&dst), 0, "68000").unwrap();
        assert_eq!(words, vec![0xD200]);
    }

    #[test]
    fn test_encode_moveq() {
        let src = Operand::Immediate(42);
        let dst = Operand::DataReg(0);
        let words = encode_instruction("MOVEQ", None, Some(&src), Some(&dst), 0, "68000").unwrap();
        assert_eq!(words, vec![0x702A]);
    }

    #[test]
    fn test_encode_bra() {
        let dst = Operand::Address(0x100);
        let words = encode_instruction("BRA", None, None, Some(&dst), 0xFE, "68000").unwrap();
        // PC after opword = 0xFE + 2 = 0x100, disp = 0x100 - 0x100 = 0
        // Wait: target=0x100, pc=0xFE
        // pc passed to enc_bra = pc + 2 = 0x100
        // disp = 0x100 - 0x100 = 0
        // op = 0x6000 | 0 = 0x6000
        assert_eq!(words, vec![0x6000]);
    }

    #[test]
    fn test_encode_rts() {
        let words = encode_instruction("RTS", None, None, None, 0, "68000").unwrap();
        assert_eq!(words, vec![0x4E75]);
    }

    #[test]
    fn test_encode_negx() {
        let dst = Operand::DataReg(0);
        let words = encode_instruction("NEGX", Some("b"), None, Some(&dst), 0, "68000").unwrap();
        assert_eq!(words, vec![0x4000]);
    }

    #[test]
    fn test_encode_extb() {
        let src = Operand::DataReg(0);
        let words = encode_instruction("EXTB", None, Some(&src), None, 0, "68020").unwrap();
        assert_eq!(words, vec![0x49C0]);
    }

    #[test]
    fn test_encode_addx() {
        let src = Operand::DataReg(0);
        let dst = Operand::DataReg(1);
        let words =
            encode_instruction("ADDX", Some("l"), Some(&src), Some(&dst), 0, "68000").unwrap();
        assert_eq!(words, vec![0xD380]);
    }

    #[test]
    fn test_encode_scc_eq() {
        let dst = Operand::DataReg(0);
        let words = encode_instruction("SEQ", None, None, Some(&dst), 0, "68000").unwrap();
        assert_eq!(words, vec![0x57C0]);
    }

    #[test]
    fn test_encode_scc_gt() {
        let dst = Operand::DataReg(2);
        let words = encode_instruction("SGT", None, None, Some(&dst), 0, "68000").unwrap();
        assert_eq!(words, vec![0x5EC2]);
    }

    #[test]
    fn test_encode_trapcc_eq() {
        let words = encode_instruction("TRAPEQ", None, None, None, 0, "68020").unwrap();
        assert_eq!(words, vec![0x57FA]);
    }

    #[test]
    fn test_encode_trapcc_ne_word() {
        let src = Operand::Immediate(0x1234);
        let words = encode_instruction("TRAPNE", Some("w"), Some(&src), None, 0, "68020").unwrap();
        assert_eq!(words, vec![0x56FB, 0x1234]);
    }

    #[test]
    fn test_encode_move_an_to_usp() {
        // MOVE A0,USP → from USP (0x4E60 | 0 = 0x4E60)
        let src = Operand::AddrReg(0);
        let dst = Operand::Immediate(0x800);
        let words =
            encode_instruction("MOVE", Some("l"), Some(&src), Some(&dst), 0, "68000").unwrap();
        assert_eq!(words, vec![0x4E60]);
    }

    #[test]
    fn test_encode_move_usp_to_an() {
        // MOVE USP,A0 → to USP (0x4E68 | 0 = 0x4E68)
        let src = Operand::Immediate(0x800);
        let dst = Operand::AddrReg(0);
        let words =
            encode_instruction("MOVE", Some("l"), Some(&src), Some(&dst), 0, "68000").unwrap();
        assert_eq!(words, vec![0x4E68]);
    }

    #[test]
    fn test_encode_trapcc_68000_fails() {
        assert!(encode_instruction("TRAPEQ", None, None, None, 0, "68000").is_err());
    }

    #[test]
    fn test_encode_extb_68000_fails() {
        let src = Operand::DataReg(0);
        assert!(encode_instruction("EXTB", None, Some(&src), None, 0, "68000").is_err());
    }

    // FPU dispatcher tests. Reference bytes from the Python assembler unless noted otherwise.

    #[test]
    fn test_dispatch_fadd_reg_reg() {
        let src = Operand::FpReg(1);
        let dst = Operand::FpReg(2);
        let words = encode_instruction("FADD", None, Some(&src), Some(&dst), 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x0522]);
    }

    #[test]
    fn test_dispatch_fsqrt_single_operand() {
        let fp0 = Operand::FpReg(0);
        let words = encode_instruction("FSQRT", None, Some(&fp0), None, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x0004]);
    }

    #[test]
    fn test_dispatch_fmove_ea_to_reg() {
        let src = Operand::DataReg(0);
        let dst = Operand::FpReg(1);
        let words =
            encode_instruction("FMOVE", Some("s"), Some(&src), Some(&dst), 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x4480]);
    }

    #[test]
    fn test_dispatch_fmovecr() {
        let src = Operand::Immediate(0);
        let dst = Operand::FpReg(0);
        let words =
            encode_instruction("FMOVECR", None, Some(&src), Some(&dst), 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x5C00]);
    }

    #[test]
    fn test_dispatch_fsmove_short_op() {
        let src = Operand::DataReg(0);
        let dst = Operand::FpReg(1);
        let words = encode_instruction("FSMOVE", None, Some(&src), Some(&dst), 0, "68020").unwrap();
        assert_eq!(words, vec![0xF200, 0x5CC0]);
    }

    #[test]
    fn test_dispatch_fmovem_range_to_predec_bugfix() {
        // fmovem fp0-fp3,-(a7): fails in the Python reference (a bug - pure ranges without
        // a '/' aren't recognized there); Rust's parser handles this correctly.
        let src = Operand::Immediate(0b1111);
        let dst = Operand::AddrRegPreDec(7);
        let words = encode_instruction("FMOVEM", None, Some(&src), Some(&dst), 0, "68020").unwrap();
        assert_eq!(words, vec![0xF227, 0xC00F]);
    }

    #[test]
    fn test_dispatch_fbeq() {
        let dst = Operand::Address(0x1010);
        let words = encode_instruction("FBEQ", None, None, Some(&dst), 0x1000, "68020").unwrap();
        assert_eq!(words, vec![0xF281, 0x000E]);
    }

    #[test]
    fn test_dispatch_fdbeq() {
        let src = Operand::DataReg(0);
        let dst = Operand::Address(0x1010);
        let words =
            encode_instruction("FDBEQ", None, Some(&src), Some(&dst), 0x1000, "68020").unwrap();
        assert_eq!(words, vec![0xF2C8, 0x0001, 0x000C]);
    }

    #[test]
    fn test_dispatch_fseq() {
        let dst = Operand::DataReg(0);
        let words = encode_instruction("FSEQ", None, None, Some(&dst), 0, "68020").unwrap();
        assert_eq!(words, vec![0xF240, 0x0001]);
    }

    #[test]
    fn test_dispatch_ftrapeq_no_operand() {
        let words = encode_instruction("FTRAPEQ", None, None, None, 0, "68020").unwrap();
        assert_eq!(words, vec![0xF27A, 0x0001]);
    }

    #[test]
    fn test_dispatch_fsave_frestore_fnop() {
        let predec = Operand::AddrRegPreDec(0);
        assert_eq!(
            encode_instruction("FSAVE", None, Some(&predec), None, 0, "68020").unwrap(),
            vec![0xF320]
        );
        let postinc = Operand::AddrRegPostInc(0);
        assert_eq!(
            encode_instruction("FRESTORE", None, Some(&postinc), None, 0, "68020").unwrap(),
            vec![0xF358]
        );
        assert_eq!(
            encode_instruction("FNOP", None, None, None, 0, "68020").unwrap(),
            vec![0xF280, 0x0000]
        );
    }

    #[test]
    fn test_dispatch_fpu_requires_68020() {
        let fp0 = Operand::FpReg(0);
        assert!(encode_instruction("FSQRT", None, Some(&fp0), None, 0, "68000").is_err());
    }

    #[test]
    fn test_dispatch_fsincos_via_ex() {
        // fsincos fp1,fp2,fp3 -> f2000cb2
        let src = Operand::FpReg(1);
        let cos = Operand::FpReg(2);
        let sin = Operand::FpReg(3);
        let words = encode_instruction_ex(
            "FSINCOS",
            None,
            Some(&src),
            Some(&cos),
            Some(&sin),
            0,
            "68020",
        )
        .unwrap();
        assert_eq!(words, vec![0xF200, 0x0CB2]);
    }

    #[test]
    fn test_dispatch_fsincos_via_plain_encode_instruction_errors() {
        let src = Operand::FpReg(1);
        let cos = Operand::FpReg(2);
        assert!(encode_instruction("FSINCOS", None, Some(&src), Some(&cos), 0, "68020").is_err());
    }
}
