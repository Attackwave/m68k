//! Instruction decoding for the disassembly view, stepping and the call
//! stack heuristic.

use std::collections::HashMap;

use m68k_core::addressing::InstructionStream;
use m68k_disasm::decoder::{DecodeResult, decode_next};

/// Decode for the widest CPU: the debugger shows what is there, it does
/// not enforce a target.
const CPU: &str = "68060";

#[derive(Debug, Clone, PartialEq)]
pub struct Insn {
    pub address: u32,
    pub bytes: Vec<u8>,
    /// Lower-case mnemonic, e.g. `bsr.w`; empty for undecodable data.
    pub mnemonic: String,
    pub text: String,
}

/// Decode one instruction from the start of `bytes`. Undecodable words and
/// a truncated buffer come back as a 2-byte `dc.w`, so a caller can always
/// advance.
pub fn decode_one(bytes: &[u8], address: u32, labels: &HashMap<u32, String>) -> Insn {
    let mut stream = InstructionStream::new(bytes, address);
    match decode_next(&mut stream, CPU) {
        Ok((_, DecodeResult::Instruction(i))) => Insn {
            address,
            mnemonic: i.mnemonic.to_lowercase(),
            text: i.format(labels),
            bytes: i.raw_bytes,
        },
        Ok((_, DecodeResult::DataWord(d))) => Insn {
            address,
            mnemonic: String::new(),
            text: d.format(labels),
            bytes: d.raw_bytes,
        },
        Err(_) => {
            let word = bytes.get(..2).unwrap_or(&[0, 0]);
            Insn {
                address,
                bytes: word.to_vec(),
                mnemonic: String::new(),
                text: format!(
                    "dc.w     ${:02x}{:02x}",
                    word[0],
                    word.get(1).copied().unwrap_or(0)
                ),
            }
        }
    }
}

/// Decode `bytes` (loaded at `start`) front to back.
pub fn decode_range(bytes: &[u8], start: u32, labels: &HashMap<u32, String>) -> Vec<Insn> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset + 2 <= bytes.len() {
        let insn = decode_one(&bytes[offset..], start + offset as u32, labels);
        offset += insn.bytes.len().max(2);
        out.push(insn);
    }
    out
}

pub fn is_call(mnemonic: &str) -> bool {
    mnemonic.starts_with("jsr") || mnemonic.starts_with("bsr")
}

/// If `ret` directly follows a `jsr`/`bsr`, the length of that call, given
/// the 6 bytes before `ret`. Calls are 2, 4 or 6 bytes long, so each length
/// is tried; the decode has to end exactly at `ret`.
pub fn call_before(before: &[u8; 6], ret: u32) -> Option<u32> {
    let none = HashMap::new();
    [2u32, 4, 6].into_iter().find(|&len| {
        let insn = decode_one(&before[6 - len as usize..], ret.wrapping_sub(len), &none);
        insn.bytes.len() == len as usize && is_call(&insn.mnemonic)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_and_resyncs_on_data() {
        // moveq #1,d0 ; dc.w $ffff ; rts
        let insns = decode_range(
            &[0x70, 0x01, 0xff, 0xff, 0x4e, 0x75],
            0x100,
            &HashMap::new(),
        );
        let addrs: Vec<u32> = insns.iter().map(|i| i.address).collect();
        assert_eq!(addrs, vec![0x100, 0x102, 0x104]);
        assert_eq!(insns[2].mnemonic, "rts");
    }

    #[test]
    fn recognises_return_addresses() {
        // bsr.s +4 (2 bytes), preceded by filler
        assert_eq!(
            call_before(&[0x4e, 0x71, 0x4e, 0x71, 0x61, 0x04], 0x1006),
            Some(2)
        );
        // jsr $00001234.l (6 bytes)
        assert_eq!(
            call_before(&[0x4e, 0xb9, 0x00, 0x00, 0x12, 0x34], 0x1006),
            Some(6)
        );
        // bsr.w (4 bytes)
        assert_eq!(
            call_before(&[0x4e, 0x71, 0x61, 0x00, 0x00, 0x10], 0x1006),
            Some(4)
        );
        // moveq ; rts: not a call site
        assert_eq!(
            call_before(&[0x4e, 0x71, 0x70, 0x01, 0x4e, 0x75], 0x1006),
            None
        );
    }
}
