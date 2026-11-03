//! Atari ST / TOS program output (`-f prg`).
//!
//! Lays the `text`, `data` and `bss` sections out back to back, the way
//! GEMDOS loads them, and writes the 28-byte header, the two segments, a
//! DRI symbol table and the relocation table described in
//! [`m68k_core::atari_prg`].
//!
//! # Symbols
//!
//! Every label gets an entry flagged with its section; equates, register
//! names and struct offsets do not, as they are not addresses. A name
//! longer than 8 characters is written as a GST extended entry, which
//! holds 22; anything longer is cut to 22, since the format has no room
//! for more.
//!
//! # Relocations
//!
//! As for the Amiga Hunk writer, the assembler does not track which
//! longwords hold addresses, so the program is assembled a second time
//! with every section moved by [`RELOC_PROBE`]: a longword that moves by
//! exactly the probe is an address and goes into the relocation table.
//! Since a TOS program is a single contiguous block, one extra assembly
//! settles all three sections at once. A value that changes in any other
//! way (an address squeezed into a word, a difference of addresses that
//! should not have changed) is an error, because GEMDOS could not fix it
//! up at load time.
use crate::amiga_hunk_writer::RELOC_PROBE;
use crate::assembler::Assembler;
use crate::directives::{Section, SectionKind};
use m68k_core::atari_prg::{
    HEADER_LEN, PRG_MAGIC, SYM_BSS, SYM_DATA, SYM_DEFINED, SYM_EXTENDED, SYM_GLOBAL, SYM_TEXT,
    SYMBOL_ENTRY_LEN,
};
use m68k_core::errors::AsmError;

/// The longest name a GST extended symbol entry holds.
const MAX_SYMBOL_NAME: usize = 22;

/// The DRI symbol table of `asm`, whose sections start at `origin`:
/// one entry per label, with its offset from the start of the text
/// segment, sorted by offset then name so the output is reproducible.
fn symbol_table(asm: &Assembler, origin: u32) -> Vec<u8> {
    let mut symbols: Vec<(u32, &str, u16)> = asm
        .symbols
        .iter()
        .filter(|(_, entry)| entry.defined)
        .filter_map(|(name, entry)| {
            let flags = match entry.section.as_deref()? {
                "text" => SYM_TEXT,
                "data" => SYM_DATA,
                "bss" => SYM_BSS,
                _ => return None,
            };
            Some((entry.value.wrapping_sub(origin), name.as_str(), flags))
        })
        .collect();
    symbols.sort_unstable();
    let mut out = Vec::with_capacity(symbols.len() * SYMBOL_ENTRY_LEN);
    for (offset, name, flags) in symbols {
        let name: Vec<u8> = name.bytes().take(MAX_SYMBOL_NAME).collect();
        let extended = name.len() > 8;
        let mut kind = SYM_DEFINED | SYM_GLOBAL | flags;
        if extended {
            kind |= SYM_EXTENDED;
        }
        let mut padded = name;
        padded.resize(if extended { MAX_SYMBOL_NAME } else { 8 }, 0);
        out.extend_from_slice(&padded[..8]);
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&offset.to_be_bytes());
        if extended {
            out.extend_from_slice(&padded[8..]);
        }
    }
    out
}

/// The three segments of a TOS program as contiguous byte ranges from the
/// start of the text segment, `(text, data, bss)` lengths.
fn segment_lengths(asm: &Assembler) -> Result<(u32, u32, u32), AsmError> {
    let mut lengths = [0u32; 3];
    for (kind, section) in asm.sections.iter_sections() {
        if section.is_empty() {
            continue;
        }
        let slot = match kind {
            SectionKind::Text => 0,
            SectionKind::Data => 1,
            SectionKind::Bss => 2,
            SectionKind::Named(name) => {
                return Err(AsmError::new(format!(
                    "section '{}': a TOS program has only text, data and bss sections",
                    name
                )));
            }
        };
        // Segments follow each other, so each must end on a word boundary
        // for the next one's code and relocations to stay aligned.
        lengths[slot] = (section.reserved_size() as u32).next_multiple_of(2);
    }
    Ok((lengths[0], lengths[1], lengths[2]))
}

/// The bytes of a section padded to `len`, which covers reserved space
/// (`DS`) after the last emitted instruction.
fn segment_bytes(section: Option<&Section>, len: u32) -> Vec<u8> {
    let mut bytes = section.map(Section::to_bytes).unwrap_or_default();
    bytes.resize(len as usize, 0);
    bytes
}

/// Assembles `base`'s source laid out as a TOS program and returns the
/// program file. `reassemble(origin, overrides)` must assemble the same
/// source with the text section at `origin` and the listed sections at
/// their given origins, as [`crate::amiga_hunk_writer::generate_relocatable_hunk_exe`]
/// expects it.
pub fn generate_prg<F>(base: &Assembler, reassemble: F) -> Result<Vec<u8>, AsmError>
where
    F: Fn(u32, &[(SectionKind, u32)]) -> Result<Assembler, AsmError>,
{
    let (text_len, data_len, bss_len) = segment_lengths(base)?;
    let origin = base.origin();
    let laid_out = |at: u32| -> Result<(Assembler, Vec<u8>), AsmError> {
        let overrides = [
            (SectionKind::Data, at.wrapping_add(text_len)),
            (
                SectionKind::Bss,
                at.wrapping_add(text_len).wrapping_add(data_len),
            ),
        ];
        let asm = reassemble(at, &overrides)?;
        let mut image = segment_bytes(asm.sections.get_section(&SectionKind::Text), text_len);
        image.extend(segment_bytes(
            asm.sections.get_section(&SectionKind::Data),
            data_len,
        ));
        Ok((asm, image))
    };
    let (laid, mut image) = laid_out(origin)?;
    let (_, moved) = laid_out(origin.wrapping_add(RELOC_PROBE)).map_err(|e| AsmError {
        message: format!(
            "{} (the value depends on the program's load address, which a TOS program \
             can only hold in a full 32-bit field)",
            e.message
        ),
        ..e
    })?;
    if moved.len() != image.len() {
        return Err(AsmError::new(
            "program changes size when relocated; TOS programs cannot use \
             absolute-short optimisation for addresses",
        ));
    }

    let mut relocs: Vec<u32> = Vec::new();
    let mut explained = vec![false; image.len()];
    for k in (0..image.len().saturating_sub(3)).step_by(2) {
        let a = u32::from_be_bytes(image[k..k + 4].try_into().unwrap());
        let b = u32::from_be_bytes(moved[k..k + 4].try_into().unwrap());
        if b.wrapping_sub(a) == RELOC_PROBE {
            relocs.push(k as u32);
            explained[k..k + 4].fill(true);
            // The file holds offsets from the text start; GEMDOS adds the
            // load address.
            image[k..k + 4].copy_from_slice(&a.wrapping_sub(origin).to_be_bytes());
        }
    }
    if let Some(k) = (0..image.len()).find(|&k| image[k] != moved[k] && !explained[k]) {
        let msg = "address used in a non-relocatable way (only full 32-bit addresses \
                   can be relocated)";
        let line = laid
            .sections
            .iter_sections()
            .flat_map(|(_, s)| s.instructions.iter().map(move |i| (s, i)))
            .find(|(s, i)| {
                let start = i.pc.wrapping_sub(s.base_addr()) as usize
                    + if *s.effective_kind() == SectionKind::Data {
                        text_len as usize
                    } else {
                        0
                    };
                (start..start + i.size_bytes()).contains(&k)
            })
            .and_then(|(_, i)| i.line_no);
        return Err(match line {
            Some(line) => AsmError::with_line(msg, line),
            None => AsmError::new(msg),
        });
    }

    let symbols = symbol_table(&laid, origin);
    let mut out = Vec::with_capacity(HEADER_LEN + image.len() + symbols.len() + relocs.len() + 8);
    out.extend_from_slice(&PRG_MAGIC.to_be_bytes());
    out.extend_from_slice(&text_len.to_be_bytes());
    out.extend_from_slice(&data_len.to_be_bytes());
    out.extend_from_slice(&bss_len.to_be_bytes());
    out.extend_from_slice(&(symbols.len() as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // reserved
    out.extend_from_slice(&0u32.to_be_bytes()); // program flags
    out.extend_from_slice(&0u16.to_be_bytes()); // absflag: relocate
    out.extend_from_slice(&image);
    out.extend_from_slice(&symbols);

    match relocs.first() {
        None => out.extend_from_slice(&0u32.to_be_bytes()),
        Some(&first) => {
            out.extend_from_slice(&first.to_be_bytes());
            for pair in relocs.windows(2) {
                let mut delta = pair[1] - pair[0];
                while delta > 254 {
                    out.push(1);
                    delta -= 254;
                }
                out.push(delta as u8);
            }
            out.push(0);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use m68k_core::atari_prg::read_prg;

    fn prg(src: &str) -> Result<Vec<u8>, AsmError> {
        let assemble = |origin: u32, overrides: &[(SectionKind, u32)]| {
            let mut asm = Assembler::new(origin);
            for (kind, at) in overrides {
                asm.sections.set_origin_override(kind.clone(), *at);
            }
            asm.assemble(src)?;
            Ok(asm)
        };
        generate_prg(&assemble(0, &[])?, assemble)
    }

    #[test]
    fn segments_follow_each_other_and_pointers_are_relocated() {
        let src = "\tlea\tmessage,a0\n\tmove.l\t#buffer,d0\n\tpea\tnext(pc)\nnext:\trts\n\
                   \tsection data\nmessage:\tdc.b\t'Hi',0\n\
                   \tsection bss\nbuffer:\tds.b\t100\n";
        let file = prg(src).unwrap();
        let loaded = read_prg(&file, 0x20000).unwrap();
        // 6 (lea) + 6 (move.l) + 4 (pea) + 2 (rts) = 18 bytes of text.
        assert_eq!(
            (loaded.text_len, loaded.data_len, loaded.bss_len),
            (18, 4, 100)
        );
        // lea's longword at 2 and move.l's at 8 are the two relocations.
        assert_eq!(loaded.relocs, vec![0x20002, 0x20008]);
        assert_eq!(&loaded.image[2..6], &0x20012u32.to_be_bytes());
        assert_eq!(&loaded.image[8..12], &0x20016u32.to_be_bytes());
        assert_eq!(&loaded.image[18..21], b"Hi\0");
        // The PC-relative push needs no relocation.
        assert_eq!(
            &file[HEADER_LEN + 12..HEADER_LEN + 16],
            &[0x48, 0x7A, 0x00, 0x02]
        );
    }

    #[test]
    fn position_independent_code_has_an_empty_table() {
        let file = prg("\tmoveq\t#0,d0\n\trts\n").unwrap();
        assert_eq!(file.len(), HEADER_LEN + 4 + 4);
        assert_eq!(&file[HEADER_LEN + 4..], &[0, 0, 0, 0]);
        assert!(read_prg(&file, 0x1000).unwrap().relocs.is_empty());
    }

    #[test]
    fn distant_relocations_use_skip_steps() {
        // A table cannot name offset 0 (that longword means "no
        // relocations"), and no real program needs to: it starts with code.
        let src = "\tnop\n\tdc.l\tfar\n\tds.b\t600\nfar:\tdc.l\tfar\n";
        let file = prg(src).unwrap();
        let loaded = read_prg(&file, 0x1000).unwrap();
        assert_eq!(loaded.relocs, vec![0x1002, 0x1000 + 606]);
        assert_eq!(&loaded.image[606..610], &(0x1000u32 + 606).to_be_bytes());
        // 604 bytes apart: two skips of 254 and a step of 96. One symbol
        // entry (`far`) sits between the segments and the table.
        let table = HEADER_LEN + 610 + SYMBOL_ENTRY_LEN + 4;
        assert_eq!(&file[table..], &[1, 1, 96, 0]);
    }

    #[test]
    fn labels_are_written_as_dri_symbols_and_read_back() {
        let src = "start:\tmoveq\t#0,d0\n\trts\n\
                   a_rather_long_label_name_indeed:\trts\n\
                   SIZE\tequ\t2\n\
                   \tsection data\nmessage:\tdc.b\t'Hi',0\n\
                   \tsection bss\nbuffer:\tds.b\tSIZE\n";
        let file = prg(src).unwrap();
        let loaded = read_prg(&file, 0x20000).unwrap();
        assert_eq!(
            loaded.symbols,
            vec![
                (0x20000, "start".to_string()),
                (0x20004, "a_rather_long_label_na".to_string()),
                (0x20006, "message".to_string()),
                (0x2000A, "buffer".to_string()),
            ]
        );
        // Three short entries and one extended entry of two.
        assert_eq!(&file[14..18], &(5 * SYMBOL_ENTRY_LEN as u32).to_be_bytes());
        assert_eq!(loaded.relocs, Vec::<u32>::new());
    }

    #[test]
    fn an_address_in_a_word_field_is_refused() {
        let err = prg("\tmove.l\t#here*2,d0\nhere:\trts\n").unwrap_err();
        assert!(err.message.contains("non-relocatable"), "{}", err.message);
        assert_eq!(err.line_no, Some(1));
    }

    #[test]
    fn named_sections_are_refused() {
        let err = prg("\tsection mine\n\trts\n").unwrap_err();
        assert!(
            err.message.contains("only text, data and bss"),
            "{}",
            err.message
        );
    }
}
