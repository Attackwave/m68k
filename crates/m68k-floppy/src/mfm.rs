//! MFM decoder for Amiga floppy disk tracks.
//!
//! Decodes raw MFM bitstreams from Amiga floppy tracks into sector data.
//! Handles the Amiga-specific MFM encoding where odd and even bits are
//! stored separately, with `0x4489` sync words marking sector boundaries.
//!
//! Sector layout in raw MFM (per RKRM / ADF spec):
//! ```text
//! Offset   Type    Count   Description
//! 0x00     word    2       0xAAAA (gap/nulls)
//! 0x04     word    1       0x4489 (sync)
//! 0x06     word    1       0x4489 (sync)
//! 0x08     long    1       info odd bits
//! 0x0C     long    1       info even bits  -> decoded: 0xFF TT SS SG
//! 0x10     long    4       sector label odd (OS recovery info)
//! 0x20     long    4       sector label even
//! 0x30     long    1       header checksum odd
//! 0x34     long    1       header checksum even
//! 0x38     long    1       data checksum odd
//! 0x3C     long    1       data checksum even
//! 0x40     byte    512     data odd bits
//! 0x240    byte    512     data even bits
//! Total: 1088 bytes per sector in raw MFM.
//! ```

use crate::floppy_base::FloppyError;

pub const MFM_SYNC_WORD: u16 = 0x4489;
pub const MFM_GAP_WORD: u16 = 0xAAAA;
pub const SECTOR_RAW_SIZE: usize = 1088;
const HEADER_INFO_SIZE: usize = 4;
const HEADER_LABEL_SIZE: usize = 16;
const DATA_SIZE: usize = 512;
pub const MASK_32: u32 = 0x5555_5555;
pub const MASK_16: u16 = 0x5555;

/// A single decoded sector from an MFM track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSector {
    pub track: u32,
    pub side: u32,
    pub sector: u32,
    pub sectors_per_track: u32,
    pub sectors_to_end: u32,
    pub data: Vec<u8>,
    pub header_checksum_ok: bool,
    pub data_checksum_ok: bool,
}

/// Decode one 32-bit long from odd/even MFM bit pairs.
pub fn decode_mfm_long(odd: u32, even: u32) -> u32 {
    (even & MASK_32) | ((odd & MASK_32) << 1)
}

/// Decode one 16-bit word from odd/even MFM bit pairs.
pub fn decode_mfm_word(odd: u16, even: u16) -> u16 {
    (even & MASK_16) | ((odd & MASK_16) << 1)
}

fn read_u32_be(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// XORs all odd and even MFM longs starting at `raw_data`'s beginning, then
/// masks with `MASK_32`. A valid checksum results in 0 after XORing with the
/// stored checksum. `num_longs` is the count of decoded longs to checksum
/// (info=1 + label=4 = 5 for the header).
fn compute_header_checksum(raw_data: &[u8], num_longs: usize) -> u32 {
    let mut checksum = 0u32;
    for i in 0..num_longs {
        let offset = i * 4;
        let odd = read_u32_be(raw_data, offset);
        let even = read_u32_be(raw_data, offset + num_longs * 4);
        checksum ^= odd;
        checksum ^= even;
    }
    checksum & MASK_32
}

/// XORs all 32-bit chunks of odd and even data, masked with `MASK_32`.
fn compute_data_checksum(data_odd: &[u8], data_even: &[u8]) -> u32 {
    let mut checksum = 0u32;
    for i in 0..DATA_SIZE / 4 {
        let offset = i * 4;
        let odd = read_u32_be(data_odd, offset);
        let even = read_u32_be(data_even, offset);
        checksum ^= odd;
        checksum ^= even;
    }
    checksum & MASK_32
}

/// Find all `0x4489` sync word byte offsets in raw MFM data.
fn find_sync_words(raw_data: &[u8]) -> Vec<usize> {
    let sync_be = MFM_SYNC_WORD.to_be_bytes();
    let mut positions = Vec::new();
    let mut start = 0;
    while start + 2 <= raw_data.len() {
        match raw_data[start..].windows(2).position(|w| w == sync_be) {
            Some(rel) => {
                let idx = start + rel;
                positions.push(idx);
                start = idx + 2;
            }
            None => break,
        }
    }
    positions
}

/// Decode 512 bytes of sector data from odd/even MFM streams.
fn decode_sector_data(data_odd: &[u8], data_even: &[u8]) -> Vec<u8> {
    let mut result = vec![0u8; DATA_SIZE];
    for i in 0..DATA_SIZE {
        let odd_byte = data_odd[i];
        let even_byte = data_even[i];
        result[i] = (even_byte & 0x55) | ((odd_byte & 0x55) << 1);
    }
    result
}

/// Parse raw track bytes into a list of 16-bit big-endian words.
///
/// If the track has an odd number of bytes, the trailing byte is dropped
/// (this should not happen with valid track dumps).
fn parse_raw_track(raw_data: &[u8]) -> Result<Vec<u16>, FloppyError> {
    if raw_data.len() < SECTOR_RAW_SIZE {
        return Err(FloppyError::new(format!(
            "Track data too small: {} bytes",
            raw_data.len()
        )));
    }
    let word_count = raw_data.len() / 2;
    let words = (0..word_count)
        .map(|i| u16::from_be_bytes([raw_data[i * 2], raw_data[i * 2 + 1]]))
        .collect();
    Ok(words)
}

/// Decodes Amiga MFM track data into sectors.
#[derive(Debug)]
pub struct MfmDecoder<'a> {
    raw_data: &'a [u8],
    words: Vec<u16>,
}

impl<'a> MfmDecoder<'a> {
    /// Initialize with raw MFM track bytes.
    ///
    /// # Errors
    /// Returns [`FloppyError`] if the track data is too small to contain any sectors.
    pub fn new(raw_track_data: &'a [u8]) -> Result<Self, FloppyError> {
        let words = parse_raw_track(raw_track_data)?;
        Ok(Self {
            raw_data: raw_track_data,
            words,
        })
    }

    /// Decode all sectors from the raw track data.
    ///
    /// Scans for `0x4489` sync marks preceded by `0xAAAA` gap words, then
    /// decodes each sector's header and data, verifying checksums.
    ///
    /// # Errors
    /// Returns [`FloppyError`] if no valid sectors are found.
    pub fn decode_track(
        &self,
        expected_track: Option<u32>,
        expected_side: Option<u32>,
    ) -> Result<Vec<DecodedSector>, FloppyError> {
        let sync_positions = find_sync_words(self.raw_data);
        let mut sectors = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for sync_pos in sync_positions {
            if let Some(sector) = self.try_decode_sector(sync_pos, expected_track, expected_side) {
                let key = (sector.track, sector.side, sector.sector);
                if seen.insert(key) {
                    sectors.push(sector);
                }
            }
        }

        if sectors.is_empty() {
            return Err(FloppyError::new("No valid MFM sectors found in track data"));
        }

        Ok(sectors)
    }

    /// Attempt to decode a sector starting near a sync word position.
    ///
    /// A valid sector has two `0x4489` sync words preceded by two `0xAAAA`
    /// gap words. The sector's raw data starts at the first `0xAAAA` word.
    fn try_decode_sector(
        &self,
        sync_byte_offset: usize,
        expected_track: Option<u32>,
        expected_side: Option<u32>,
    ) -> Option<DecodedSector> {
        let word_index = sync_byte_offset / 2;

        if word_index < 2 {
            return None;
        }

        if self.words[word_index - 2] != MFM_GAP_WORD || self.words[word_index - 1] != MFM_GAP_WORD
        {
            return None;
        }

        if word_index + 1 >= self.words.len() {
            return None;
        }

        if self.words[word_index] != MFM_SYNC_WORD || self.words[word_index + 1] != MFM_SYNC_WORD {
            return None;
        }

        let sector_word_start = word_index - 2;
        let sector_byte_start = sector_word_start * 2;

        if sector_byte_start + SECTOR_RAW_SIZE > self.raw_data.len() {
            return None;
        }

        let sector_raw = &self.raw_data[sector_byte_start..sector_byte_start + SECTOR_RAW_SIZE];

        Self::decode_sector_from_raw(sector_raw, expected_track, expected_side)
    }

    /// Decode a single sector from its 1088 bytes of raw MFM data.
    fn decode_sector_from_raw(
        sector_raw: &[u8],
        expected_track: Option<u32>,
        expected_side: Option<u32>,
    ) -> Option<DecodedSector> {
        if sector_raw.len() != SECTOR_RAW_SIZE {
            return None;
        }

        let info_odd = read_u32_be(sector_raw, 0x08);
        let info_even = read_u32_be(sector_raw, 0x0C);
        let info_decoded = decode_mfm_long(info_odd, info_even);

        let format_byte = (info_decoded >> 24) & 0xFF;
        let track_num = (info_decoded >> 16) & 0xFF;
        let sector_num = (info_decoded >> 8) & 0xFF;
        let sectors_to_end = info_decoded & 0xFF;

        if format_byte != 0xFF {
            return None;
        }

        if let Some(expected) = expected_track
            && track_num != expected
        {
            return None;
        }

        if sector_num > 22 {
            return None;
        }

        let side_num = track_num % 2;
        if let Some(expected) = expected_side
            && side_num != expected
        {
            return None;
        }

        let checksum_longs = HEADER_INFO_SIZE / 4 + HEADER_LABEL_SIZE / 4;
        let computed_hdr_cksum = compute_header_checksum(&sector_raw[0x08..], checksum_longs);

        let stored_hdr_odd = read_u32_be(sector_raw, 0x30);
        let stored_hdr_even = read_u32_be(sector_raw, 0x34);
        let stored_hdr_cksum = decode_mfm_long(stored_hdr_odd, stored_hdr_even) & MASK_32;

        let header_checksum_ok = computed_hdr_cksum == stored_hdr_cksum;

        let data_odd = &sector_raw[0x40..0x40 + DATA_SIZE];
        let data_even = &sector_raw[0x40 + DATA_SIZE..0x40 + DATA_SIZE * 2];

        let computed_data_cksum = compute_data_checksum(data_odd, data_even);

        let stored_data_odd = read_u32_be(sector_raw, 0x38);
        let stored_data_even = read_u32_be(sector_raw, 0x3C);
        let stored_data_cksum = decode_mfm_long(stored_data_odd, stored_data_even) & MASK_32;

        let data_checksum_ok = computed_data_cksum == stored_data_cksum;

        let decoded_data = decode_sector_data(data_odd, data_even);

        let actual_side = track_num % 2;

        Some(DecodedSector {
            track: track_num / 2,
            side: actual_side,
            sector: sector_num,
            sectors_per_track: sectors_to_end,
            sectors_to_end,
            data: decoded_data,
            header_checksum_ok,
            data_checksum_ok,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_mfm_long(value: u32) -> (u32, u32) {
        let odd = (value & 0xAAAA_AAAA) >> 1;
        let even = value & 0x5555_5555;
        (odd, even)
    }

    fn encode_mfm_word(value: u16) -> (u16, u16) {
        let odd = (value & 0xAAAA) >> 1;
        let even = value & 0x5555;
        (odd, even)
    }

    fn write_u32_be(buf: &mut [u8], offset: usize, value: u32) {
        buf[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn write_u16_be(buf: &mut [u8], offset: usize, value: u16) {
        buf[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }

    fn build_valid_sector(track: u32, sector: u32, sectors_to_end: u32, data: &[u8]) -> Vec<u8> {
        assert_eq!(data.len(), 512, "data must be exactly 512 bytes");

        let mut sector_raw = vec![0u8; SECTOR_RAW_SIZE];

        write_u16_be(&mut sector_raw, 0, MFM_GAP_WORD);
        write_u16_be(&mut sector_raw, 2, MFM_GAP_WORD);
        write_u16_be(&mut sector_raw, 4, MFM_SYNC_WORD);
        write_u16_be(&mut sector_raw, 6, MFM_SYNC_WORD);

        let info_value = (0xFFu32 << 24) | (track << 16) | (sector << 8) | sectors_to_end;
        let (info_odd, info_even) = encode_mfm_long(info_value);
        write_u32_be(&mut sector_raw, 0x08, info_odd);
        write_u32_be(&mut sector_raw, 0x0C, info_even);

        // label_odd = label_even = 0, already zeroed.

        let mut checksum = 0u32;
        checksum ^= info_odd;
        checksum ^= info_even;
        // 4 label longs, each odd=even=0, contribute nothing.
        checksum &= MASK_32;

        let (cksum_odd, cksum_even) = encode_mfm_long(checksum);
        write_u32_be(&mut sector_raw, 0x30, cksum_odd);
        write_u32_be(&mut sector_raw, 0x34, cksum_even);

        let mut data_odd = vec![0u8; 512];
        let mut data_even = vec![0u8; 512];
        for i in 0..512 {
            let byte = data[i];
            data_odd[i] = (byte & 0xAA) >> 1;
            data_even[i] = byte & 0x55;
        }

        let mut data_checksum = 0u32;
        for i in 0..512 / 4 {
            let odd_val = read_u32_be(&data_odd, i * 4);
            let even_val = read_u32_be(&data_even, i * 4);
            data_checksum ^= odd_val;
            data_checksum ^= even_val;
        }
        data_checksum &= MASK_32;

        let (data_cksum_odd, data_cksum_even) = encode_mfm_long(data_checksum);
        write_u32_be(&mut sector_raw, 0x38, data_cksum_odd);
        write_u32_be(&mut sector_raw, 0x3C, data_cksum_even);

        sector_raw[0x40..0x40 + 512].copy_from_slice(&data_odd);
        sector_raw[0x240..0x240 + 512].copy_from_slice(&data_even);

        sector_raw
    }

    fn build_track_with_sectors(track_num: u32, num_sectors: u32, data_pattern: u8) -> Vec<u8> {
        let mut raw_track = Vec::new();
        let gap_words = 50;
        for _ in 0..gap_words {
            raw_track.extend_from_slice(&MFM_GAP_WORD.to_be_bytes());
        }

        for s in 0..num_sectors {
            let data = vec![data_pattern ^ s as u8; 512];
            let sectors_to_end = num_sectors - s;
            raw_track.extend(build_valid_sector(track_num, s, sectors_to_end, &data));
        }

        raw_track
    }

    #[test]
    fn test_decode_mfm_long_roundtrip() {
        let value = 0xFF01_000B;
        let (odd, even) = encode_mfm_long(value);
        assert_eq!(decode_mfm_long(odd, even), value);
    }

    #[test]
    fn test_decode_mfm_long_all_ones() {
        let value = 0xFFFF_FFFF;
        let (odd, even) = encode_mfm_long(value);
        assert_eq!(decode_mfm_long(odd, even), value);
    }

    #[test]
    fn test_decode_mfm_long_zero() {
        let (odd, even) = encode_mfm_long(0);
        assert_eq!(decode_mfm_long(odd, even), 0);
    }

    #[test]
    fn test_decode_mfm_long_alternating() {
        let value = 0xAAAA_AAAA;
        let (odd, even) = encode_mfm_long(value);
        assert_eq!(decode_mfm_long(odd, even), value);
    }

    #[test]
    fn test_decode_mfm_word_roundtrip() {
        let value = 0x4489;
        let (odd, even) = encode_mfm_word(value);
        assert_eq!(decode_mfm_word(odd, even), value);
    }

    #[test]
    fn test_mask_values() {
        assert_eq!(MASK_32, 0x5555_5555);
        assert_eq!(MASK_16, 0x5555);
    }

    #[test]
    fn test_valid_sector_size() {
        let sector = build_valid_sector(0, 0, 11, &[0xAAu8; 512]);
        assert_eq!(sector.len(), SECTOR_RAW_SIZE);
    }

    #[test]
    fn test_valid_sector_info_decodes() {
        let sector = build_valid_sector(3, 5, 6, &[0u8; 512]);
        let info_odd = read_u32_be(&sector, 0x08);
        let info_even = read_u32_be(&sector, 0x0C);
        let info = decode_mfm_long(info_odd, info_even);
        assert_eq!((info >> 24) & 0xFF, 0xFF);
        assert_eq!((info >> 16) & 0xFF, 3);
        assert_eq!((info >> 8) & 0xFF, 5);
        assert_eq!(info & 0xFF, 6);
    }

    #[test]
    fn test_decode_single_sector() {
        let sector = build_valid_sector(0, 3, 8, &[0xABu8; 512]);
        let decoder = MfmDecoder::new(&sector).unwrap();
        let sectors = decoder.decode_track(Some(0), Some(0)).unwrap();
        assert_eq!(sectors.len(), 1);
        let s = &sectors[0];
        assert_eq!(s.track, 0);
        assert_eq!(s.side, 0);
        assert_eq!(s.sector, 3);
        assert_eq!(s.sectors_to_end, 8);
        assert_eq!(s.data, vec![0xABu8; 512]);
        assert!(s.header_checksum_ok);
        assert!(s.data_checksum_ok);
    }

    #[test]
    fn test_decode_sector_various_data() {
        let data: Vec<u8> = (0..=255).chain(0..=255).collect();
        let sector = build_valid_sector(1, 0, 11, &data);
        let decoder = MfmDecoder::new(&sector).unwrap();
        let sectors = decoder.decode_track(None, None).unwrap();
        assert_eq!(sectors.len(), 1);
        assert_eq!(sectors[0].data, data);
    }

    #[test]
    fn test_decode_full_track() {
        let track_data = build_track_with_sectors(0, 11, 0xAA);
        let decoder = MfmDecoder::new(&track_data).unwrap();
        let sectors = decoder.decode_track(Some(0), Some(0)).unwrap();
        assert_eq!(sectors.len(), 11);
    }

    #[test]
    fn test_decode_sectors_unique_data() {
        let track_data = build_track_with_sectors(5, 11, 0x42);
        let decoder = MfmDecoder::new(&track_data).unwrap();
        let sectors = decoder.decode_track(None, None).unwrap();
        for s in &sectors {
            assert_eq!(s.data, vec![0x42u8 ^ s.sector as u8; 512]);
        }
    }

    #[test]
    fn test_decode_track_high_number() {
        let track_data = build_track_with_sectors(159, 11, 0xAA);
        let decoder = MfmDecoder::new(&track_data).unwrap();
        let sectors = decoder.decode_track(Some(159), None).unwrap();
        assert_eq!(sectors.len(), 11);
        for s in &sectors {
            assert_eq!(s.track, 79);
            assert_eq!(s.side, 1);
        }
    }

    #[test]
    fn test_wrong_track_number_filtered() {
        let track_data = build_track_with_sectors(5, 11, 0xAA);
        let decoder = MfmDecoder::new(&track_data).unwrap();
        let err = decoder.decode_track(Some(0), None).unwrap_err();
        assert!(err.0.contains("No valid MFM sectors"));
    }

    #[test]
    fn test_data_too_small() {
        let err = MfmDecoder::new(&[0u8; 100]).unwrap_err();
        assert!(err.0.contains("too small"));
    }

    #[test]
    fn test_no_sync_words() {
        let mut sector = build_valid_sector(0, 0, 11, &[0xAAu8; 512]);
        sector[4..8].copy_from_slice(&[0, 0, 0, 0]);
        let decoder = MfmDecoder::new(&sector).unwrap();
        let err = decoder.decode_track(None, None).unwrap_err();
        assert!(err.0.contains("No valid MFM sectors"));
    }

    #[test]
    fn test_corrupted_header_checksum() {
        let mut sector = build_valid_sector(0, 0, 11, &[0xAAu8; 512]);
        sector[0x37] ^= 0x04;
        let decoder = MfmDecoder::new(&sector).unwrap();
        let sectors = decoder.decode_track(None, None).unwrap();
        assert_eq!(sectors.len(), 1);
        assert!(!sectors[0].header_checksum_ok);
        assert!(sectors[0].data_checksum_ok);
    }

    #[test]
    fn test_corrupted_data_checksum() {
        let mut sector = build_valid_sector(0, 0, 11, &[0xAAu8; 512]);
        sector[0x3F] ^= 0x04;
        let decoder = MfmDecoder::new(&sector).unwrap();
        let sectors = decoder.decode_track(None, None).unwrap();
        assert_eq!(sectors.len(), 1);
        assert!(sectors[0].header_checksum_ok);
        assert!(!sectors[0].data_checksum_ok);
    }

    #[test]
    fn test_corrupted_data_detected() {
        let mut sector = build_valid_sector(0, 0, 11, &[0xAAu8; 512]);
        sector[0x40] ^= 0xFF;
        let decoder = MfmDecoder::new(&sector).unwrap();
        let sectors = decoder.decode_track(None, None).unwrap();
        assert_eq!(sectors.len(), 1);
        assert!(!sectors[0].data_checksum_ok);
    }

    #[test]
    fn test_sync_word_at_buffer_end_does_not_panic() {
        // A gap+sync byte pattern landing exactly at the last 2 bytes of an
        // even-length buffer must not cause an out-of-bounds panic when
        // probing `words[word_index + 1]`.
        let mut raw_track = vec![0u8; SECTOR_RAW_SIZE];
        // Two gap words followed by a single sync word right at the end.
        let sync_word_offset = raw_track.len() - 2;
        write_u16_be(&mut raw_track, sync_word_offset - 4, MFM_GAP_WORD);
        write_u16_be(&mut raw_track, sync_word_offset - 2, MFM_GAP_WORD);
        write_u16_be(&mut raw_track, sync_word_offset, MFM_SYNC_WORD);
        let decoder = MfmDecoder::new(&raw_track).unwrap();
        let err = decoder.decode_track(None, None).unwrap_err();
        assert!(err.0.contains("No valid MFM sectors"));
    }

    #[test]
    fn test_duplicate_sectors_deduplicated() {
        let track_data = build_track_with_sectors(0, 1, 0xAA);
        let extra_sector = build_valid_sector(0, 0, 11, &[0xFFu8; 512]);
        let combined = [track_data, extra_sector].concat();
        let decoder = MfmDecoder::new(&combined).unwrap();
        let sectors = decoder.decode_track(None, None).unwrap();
        assert_eq!(sectors.len(), 1);
    }
}
