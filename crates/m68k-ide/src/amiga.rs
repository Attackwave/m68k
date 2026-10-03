//! Amiga-specific artifact construction.

use m68k_floppy::adf_writer::{compute_bootblock_checksum, create_blank_image};

/// Size of an 880 KB double-density Amiga floppy image.
const ADF_SIZE: usize = 901_120;
/// The bootblock is sectors 0 and 1.
const BOOTBLOCK_SIZE: usize = 1024;
/// Kickstart begins executing at offset 12 of the bootblock.
const BOOT_ENTRY: usize = 0x0C;
/// Payload starts on the sector after the bootblock (block 2).
const PAYLOAD_BLOCK: usize = 2;

/// Bytes of bootblock available to the loader stub, after the 12-byte
/// header (`DOS\0`, checksum, root block pointer).
const STUB_CAPACITY: usize = BOOTBLOCK_SIZE - BOOT_ENTRY;

/// Build a bootable 880 KB ADF that loads `code` from disk and runs it.
///
/// The bootblock carries a small position-independent loader: Kickstart
/// reads only sectors 0-1 into memory and jumps to offset 12 with an
/// open `trackdisk.device` request in `A1`, so anything larger than the
/// bootblock has to be pulled in by that stub. The loader reads the
/// payload to `$20000` (clear of Kickstart's own workspace on a
/// 512 KB machine) via `CMD_READ`, then returns it as the start address
/// in `A0` with `D0` zeroed, which is what the strap module requires.
pub fn build_bootable_adf(code: &[u8]) -> Result<Vec<u8>, String> {
    if code.is_empty() {
        return Err("Programm enthält keinen Code für die ADF.".into());
    }

    let payload_capacity = ADF_SIZE - PAYLOAD_BLOCK * 512;
    if code.len() > payload_capacity {
        return Err(format!(
            "Programm ist zu groß für eine Diskette: {} Bytes, {payload_capacity} verfügbar.",
            code.len()
        ));
    }

    let mut image =
        create_blank_image(ADF_SIZE, 0).map_err(|e| format!("ADF nicht anlegbar: {e}"))?;

    // Round the read length up to whole sectors: trackdisk.device
    // transfers in 512-byte units and rejects a partial one.
    let read_len = code.len().div_ceil(512) * 512;
    let stub = bootblock_stub(read_len as u32);
    if stub.len() > STUB_CAPACITY {
        return Err(format!(
            "Bootblock-Lader passt nicht in den Bootblock: {} von {STUB_CAPACITY} Bytes.",
            stub.len()
        ));
    }

    image[0..4].copy_from_slice(b"DOS\0");
    image[BOOT_ENTRY..BOOT_ENTRY + stub.len()].copy_from_slice(&stub);

    let payload_offset = PAYLOAD_BLOCK * 512;
    image[payload_offset..payload_offset + code.len()].copy_from_slice(code);

    // Checksum last: it covers the bootblock as finally written.
    let bootblock: [u8; BOOTBLOCK_SIZE] = image[..BOOTBLOCK_SIZE]
        .try_into()
        .expect("slice is exactly one bootblock");
    let checksum = compute_bootblock_checksum(&bootblock);
    image[4..8].copy_from_slice(&checksum.to_be_bytes());

    Ok(image)
}

/// Address the payload is loaded to and entered at.
const LOAD_ADDRESS: u32 = 0x0002_0000;

/// Assemble the position-independent bootblock loader.
///
/// Hand-encoded rather than run through the assembler: this stub is
/// fixed, and encoding it here keeps the ADF writer independent of
/// assembler state. Kept in lockstep with the listing in the comments,
/// and covered by `stub_encodes_expected_instructions`.
fn bootblock_stub(read_len: u32) -> Vec<u8> {
    let mut s = Vec::new();

    // move.l a1,a5            ; stash the trackdisk IORequest
    s.extend_from_slice(&[0x2A, 0x49]);

    // io_Length($24) = read_len
    // move.l #read_len,$24(a5)
    s.extend_from_slice(&[0x2B, 0x7C]);
    s.extend_from_slice(&read_len.to_be_bytes());
    s.extend_from_slice(&[0x00, 0x24]);

    // io_Data($28) = LOAD_ADDRESS
    // move.l #LOAD_ADDRESS,$28(a5)
    s.extend_from_slice(&[0x2B, 0x7C]);
    s.extend_from_slice(&LOAD_ADDRESS.to_be_bytes());
    s.extend_from_slice(&[0x00, 0x28]);

    // io_Offset($2C) = PAYLOAD_BLOCK * 512
    // move.l #offset,$2C(a5)
    s.extend_from_slice(&[0x2B, 0x7C]);
    s.extend_from_slice(&((PAYLOAD_BLOCK as u32) * 512).to_be_bytes());
    s.extend_from_slice(&[0x00, 0x2C]);

    // io_Command($1C) = CMD_READ (2)
    // move.w #2,$1C(a5)
    s.extend_from_slice(&[0x3B, 0x7C, 0x00, 0x02, 0x00, 0x1C]);

    // move.l a5,a1            ; DoIO(request)
    s.extend_from_slice(&[0x22, 0x4D]);
    // move.l $4.w,a6          ; ExecBase
    s.extend_from_slice(&[0x2C, 0x78, 0x00, 0x04]);
    // jsr -456(a6)            ; DoIO
    s.extend_from_slice(&[0x4E, 0xAE, 0xFE, 0x38]);

    // moveq #0,d0             ; success
    s.extend_from_slice(&[0x70, 0x00]);
    // movea.l #LOAD_ADDRESS,a0 ; start address for the strap module
    s.extend_from_slice(&[0x20, 0x7C]);
    s.extend_from_slice(&LOAD_ADDRESS.to_be_bytes());
    // rts
    s.extend_from_slice(&[0x4E, 0x75]);

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use m68k_floppy::adf_writer::verify_bootblock_checksum;

    #[test]
    fn produces_a_correctly_sized_image_with_valid_checksum() {
        let adf = build_bootable_adf(&[0x4E, 0x71, 0x60, 0xFE]).unwrap();
        assert_eq!(adf.len(), ADF_SIZE);
        assert_eq!(&adf[0..4], b"DOS\0");
        assert!(verify_bootblock_checksum(&adf).unwrap());
    }

    #[test]
    fn payload_lands_on_block_two() {
        let code = [0xDE, 0xAD, 0xBE, 0xEF];
        let adf = build_bootable_adf(&code).unwrap();
        let at = PAYLOAD_BLOCK * 512;
        assert_eq!(&adf[at..at + 4], &code);
    }

    #[test]
    fn rejects_empty_and_oversized_programs() {
        assert!(build_bootable_adf(&[]).is_err());
        let too_big = vec![0u8; ADF_SIZE];
        assert!(build_bootable_adf(&too_big).is_err());
    }

    #[test]
    fn read_length_is_rounded_up_to_whole_sectors() {
        // A one-byte program still reads a full sector.
        let adf = build_bootable_adf(&[0x4E]).unwrap();
        let len_field = u32::from_be_bytes(adf[BOOT_ENTRY + 4..BOOT_ENTRY + 8].try_into().unwrap());
        assert_eq!(len_field, 512);
    }

    /// The stub is hand-encoded, so decode it back and confirm it is the
    /// program the comments claim. Without this the bytes are unverified.
    #[test]
    fn stub_encodes_expected_instructions() {
        // Deliberately not 1024: that is also the payload's disk offset,
        // so an equal value would hide the two fields being swapped.
        let stub = bootblock_stub(4096);
        let mut dis = m68k_disasm::disassembler::Disassembler::new(stub, 0);
        dis.set_cpu("68000");
        let lines = dis.disassemble();
        let text = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();

        // Every instruction must decode: a bootblock that traps on a
        // bad opcode fails with an unhelpful Guru, not an error message.
        assert!(
            !lines.iter().any(|l| l.is_error),
            "stub does not decode:\n{text}"
        );
        // io_Length is the read size, io_Offset the position on disk.
        assert!(text.contains("move.l  #$00001000, $24(a5)"), "{text}");
        assert!(text.contains("move.l  #$00000400, $2c(a5)"), "{text}");
        // The three moves that fill the IORequest, the DoIO call and the
        // return convention are what the boot protocol requires.
        // DoIO is at LVO -456 = -$1C8 from ExecBase.
        assert!(text.contains("jsr     -$1c8(a6)"), "{text}");
        // D0 = 0 and A0 = load address are the strap module's contract.
        assert!(text.contains("moveq   #$00000000, d0"), "{text}");
        assert!(text.contains("movea.l #$00020000, a0"), "{text}");
        assert!(text.trim_end().ends_with("rts"), "{text}");
    }
}
