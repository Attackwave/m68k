//! Atari ST / TOS program files (`.PRG`, `.TOS`, `.TTP`, `.ACC`).
//!
//! A TOS program is one contiguous image: a 28-byte header, the text
//! segment, the data segment, an optional DRI symbol table and a
//! relocation table. GEMDOS loads text and data back to back at whatever
//! address is free, reserves the BSS behind them and adds the load
//! address to every longword the relocation table names.
//!
//! ```text
//! offset  size  field
//!      0     2  magic $601A (`bra.s` past the header, historically)
//!      2     4  text length
//!      6     4  data length
//!     10     4  bss length
//!     14     4  symbol table length
//!     18     4  reserved, 0
//!     22     4  program flags
//!     26     2  absflag: non-zero means "already absolute, no relocation"
//! ```
//!
//! The relocation table starts with a longword offset (from the start of
//! the text segment) of the first relocated longword, 0 when there are
//! none. Each following byte is a step: 0 ends the table, 1 advances the
//! position by 254 without relocating, any other value advances by that
//! many bytes to the next relocated longword.

use std::fmt;

pub const PRG_MAGIC: u16 = 0x601A;
pub const HEADER_LEN: usize = 28;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrgError(pub String);

impl fmt::Display for PrgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PrgError {}

/// A TOS program loaded at `load_base`, the way GEMDOS would place it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TosProgram {
    /// Text followed by data, relocated to `load_base`. The BSS is not
    /// part of the file and not part of the image.
    pub image: Vec<u8>,
    pub load_base: u32,
    pub text_len: u32,
    pub data_len: u32,
    pub bss_len: u32,
    /// Absolute addresses of every relocated longword, ascending.
    pub relocs: Vec<u32>,
}

fn long(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(data[at..at + 4].try_into().unwrap())
}

/// Whether `data` carries a consistent TOS program header.
///
/// The magic alone is a plain `bra.s` opword, which a raw binary can
/// start with just as well, so the segment lengths must also fit the
/// file and the reserved longword must be zero.
pub fn looks_like_prg(data: &[u8]) -> bool {
    if data.len() < HEADER_LEN || u16::from_be_bytes([data[0], data[1]]) != PRG_MAGIC {
        return false;
    }
    let text = long(data, 2) as u64;
    let data_len = long(data, 6) as u64;
    let symbols = long(data, 14) as u64;
    long(data, 18) == 0 && HEADER_LEN as u64 + text + data_len + symbols <= data.len() as u64
}

/// Parses a TOS program and relocates it to `load_base`.
pub fn read_prg(data: &[u8], load_base: u32) -> Result<TosProgram, PrgError> {
    if data.len() < HEADER_LEN {
        return Err(PrgError("file is shorter than a TOS program header".into()));
    }
    if u16::from_be_bytes([data[0], data[1]]) != PRG_MAGIC {
        return Err(PrgError(format!(
            "not a TOS program: magic is ${:04x}, expected ${:04x}",
            u16::from_be_bytes([data[0], data[1]]),
            PRG_MAGIC
        )));
    }
    let text_len = long(data, 2);
    let data_len = long(data, 6);
    let bss_len = long(data, 10);
    let symbols_len = long(data, 14);
    let absolute = u16::from_be_bytes([data[26], data[27]]) != 0;

    let segments = text_len as u64 + data_len as u64;
    let image_end = HEADER_LEN as u64 + segments;
    if image_end > data.len() as u64 {
        return Err(PrgError(format!(
            "text ({}) and data ({}) run past the end of the file ({} bytes)",
            text_len,
            data_len,
            data.len()
        )));
    }
    if segments > u32::MAX as u64 || load_base.checked_add(segments as u32).is_none() {
        return Err(PrgError(
            "program does not fit in the 32-bit address space".into(),
        ));
    }
    let mut image = data[HEADER_LEN..image_end as usize].to_vec();

    let mut relocs = Vec::new();
    let table = image_end.saturating_add(symbols_len as u64);
    // A program flagged absolute carries no table; one that is not but
    // ends right after its segments is taken the same way.
    if !absolute && table as usize + 4 <= data.len() {
        let table = table as usize;
        let mut at = long(data, table) as u64;
        if at != 0 {
            let mut cursor = table + 4;
            loop {
                if at + 4 > segments || !at.is_multiple_of(2) {
                    return Err(PrgError(format!(
                        "relocation at offset {} lies outside the program or is odd",
                        at
                    )));
                }
                let k = at as usize;
                let v = long(&image, k).wrapping_add(load_base);
                image[k..k + 4].copy_from_slice(&v.to_be_bytes());
                relocs.push(load_base.wrapping_add(at as u32));
                let step = *data
                    .get(cursor)
                    .ok_or_else(|| PrgError("relocation table ends without a terminator".into()))?;
                cursor += 1;
                match step {
                    0 => break,
                    1 => {
                        // Skip steps may chain; only the final non-1 step
                        // names a longword.
                        at += 254;
                        while data.get(cursor) == Some(&1) {
                            at += 254;
                            cursor += 1;
                        }
                        match data.get(cursor) {
                            Some(0) | None => break,
                            Some(&n) => {
                                at += n as u64;
                                cursor += 1;
                            }
                        }
                    }
                    n => at += n as u64,
                }
            }
        }
    }

    Ok(TosProgram {
        image,
        load_base,
        text_len,
        data_len,
        bss_len,
        relocs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(text: u32, data: u32, bss: u32, absflag: u16) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(&PRG_MAGIC.to_be_bytes());
        h.extend_from_slice(&text.to_be_bytes());
        h.extend_from_slice(&data.to_be_bytes());
        h.extend_from_slice(&bss.to_be_bytes());
        h.extend_from_slice(&0u32.to_be_bytes()); // symbols
        h.extend_from_slice(&0u32.to_be_bytes()); // reserved
        h.extend_from_slice(&0u32.to_be_bytes()); // flags
        h.extend_from_slice(&absflag.to_be_bytes());
        h
    }

    #[test]
    fn relocates_the_longwords_the_table_names() {
        // text: lea $8,a0 (41 f9 00 00 00 08); rts; data: dc.l $2
        let mut file = header(8, 4, 16, 0);
        file.extend_from_slice(&[0x41, 0xF9, 0x00, 0x00, 0x00, 0x08, 0x4E, 0x75]);
        file.extend_from_slice(&[0, 0, 0, 2]);
        file.extend_from_slice(&2u32.to_be_bytes()); // first reloc at offset 2
        file.extend_from_slice(&[6, 0]); // next at 2 + 6 = 8, end

        let prg = read_prg(&file, 0x10000).unwrap();
        assert_eq!((prg.text_len, prg.data_len, prg.bss_len), (8, 4, 16));
        assert_eq!(prg.relocs, vec![0x10002, 0x10008]);
        assert_eq!(&prg.image[2..6], &0x10008u32.to_be_bytes());
        assert_eq!(&prg.image[8..12], &0x10002u32.to_be_bytes());
        assert!(looks_like_prg(&file));
    }

    #[test]
    fn a_skip_step_advances_without_relocating() {
        let text = 300u32;
        let mut file = header(text, 0, 0, 0);
        file.resize(file.len() + text as usize, 0);
        file.extend_from_slice(&4u32.to_be_bytes());
        // 4 + 254 + 20 = 278
        file.extend_from_slice(&[1, 20, 0]);
        let prg = read_prg(&file, 0x100).unwrap();
        assert_eq!(prg.relocs, vec![0x104, 0x100 + 278]);
    }

    #[test]
    fn no_table_or_absolute_flag_means_no_relocations() {
        let mut file = header(2, 0, 0, 0);
        file.extend_from_slice(&[0x4E, 0x75]);
        assert_eq!(read_prg(&file, 0).unwrap().relocs, Vec::<u32>::new());
        let mut file = header(2, 0, 0, 1);
        file.extend_from_slice(&[0x4E, 0x75]);
        file.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(read_prg(&file, 0).unwrap().relocs, Vec::<u32>::new());
    }

    #[test]
    fn inconsistent_headers_are_rejected_or_not_recognised() {
        let mut file = header(100, 0, 0, 0);
        file.extend_from_slice(&[0x4E, 0x75]);
        assert!(!looks_like_prg(&file));
        assert!(read_prg(&file, 0).unwrap_err().0.contains("past the end"));

        // A raw binary that happens to start with `bra.s`.
        let raw = [0x60, 0x1A, 0x4E, 0x71];
        assert!(!looks_like_prg(&raw));

        let mut file = header(4, 0, 0, 0);
        file.extend_from_slice(&[0x4E, 0x71, 0x4E, 0x75]);
        file.extend_from_slice(&3u32.to_be_bytes());
        file.push(0);
        assert!(read_prg(&file, 0).unwrap_err().0.contains("odd"));
    }
}
