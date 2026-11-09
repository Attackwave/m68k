//! Atari ST floppy images: raw sector dumps (`.ST`) and Magic Shadow
//! Archiver files (`.MSA`).
//!
//! An `.ST` file is the disk's sectors in order, track by track, side
//! by side within a track, 512 bytes each. The geometry is in the boot
//! sector's BIOS parameter block, which TOS writes in little-endian order
//! like a PC; a disk formatted without one is read by its size.
//!
//! An `.MSA` file holds the same sectors behind a 10-byte header, each
//! track-side either stored as is or run-length compressed:
//!
//! ```text
//! offset  size  field (big-endian)
//!      0     2  magic $0E0F
//!      2     2  sectors per track
//!      4     2  sides - 1
//!      6     2  first track
//!      8     2  last track
//! ```
//!
//! Each track-side follows as a 16-bit length and that many bytes. A
//! length equal to the track's full size means the data is uncompressed;
//! otherwise `$E5 byte count` (count 16-bit) stands for `count` copies of
//! `byte`, and any other byte stands for itself.

use std::path::Path;

use crate::floppy_base::{FloppyError, FloppyImageReader, require_file};

const SECTOR_SIZE: usize = 512;
const MSA_MAGIC: u16 = 0x0E0F;
const MSA_RUN: u8 = 0xE5;

/// Disk geometry: how the sectors of an image are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub sectors_per_track: u32,
    pub sides: u32,
    pub tracks: u32,
}

impl Geometry {
    /// Geometry from the boot sector's BIOS parameter block, if its
    /// fields describe a floppy and agree with the image size.
    fn from_boot_sector(boot: &[u8], image_len: usize) -> Option<Self> {
        if boot.len() < 28 {
            return None;
        }
        let word = |at: usize| u16::from_le_bytes([boot[at], boot[at + 1]]) as u32;
        let bytes_per_sector = word(11);
        let total_sectors = word(19);
        let sectors_per_track = word(24);
        let sides = word(26);
        if bytes_per_sector != SECTOR_SIZE as u32
            || !(1..=36).contains(&sectors_per_track)
            || !(1..=2).contains(&sides)
            || total_sectors == 0
            || total_sectors as usize * SECTOR_SIZE > image_len
        {
            return None;
        }
        Some(Self {
            sectors_per_track,
            sides,
            tracks: total_sectors.div_ceil(sectors_per_track * sides),
        })
    }

    /// Geometry guessed from the image size alone: the common Atari
    /// formats, then any 80-track double-sided layout the size divides
    /// into.
    fn from_size(image_len: usize) -> Option<Self> {
        let sectors = image_len / SECTOR_SIZE;
        let known = [
            (9, 1, 80),
            (9, 2, 80),
            (10, 1, 80),
            (10, 2, 80),
            (11, 2, 80),
        ];
        for (spt, sides, tracks) in known {
            if sectors == spt * sides * tracks {
                return Some(Self {
                    sectors_per_track: spt as u32,
                    sides: sides as u32,
                    tracks: tracks as u32,
                });
            }
        }
        let per_cylinder = sectors / 80;
        (per_cylinder > 0 && per_cylinder.is_multiple_of(2) && sectors == per_cylinder * 80)
            .then_some(Self {
                sectors_per_track: (per_cylinder / 2) as u32,
                sides: 2,
                tracks: 80,
            })
    }
}

/// A raw Atari ST sector image held in memory.
#[derive(Debug)]
pub struct StBackend {
    image: Vec<u8>,
    pub geometry: Geometry,
}

impl StBackend {
    /// Open a raw `.ST` sector dump.
    pub fn open(filepath: impl AsRef<Path>) -> Result<Self, FloppyError> {
        let filepath = require_file(filepath.as_ref())?;
        let image = std::fs::read(&filepath).map_err(|e| FloppyError::new(e.to_string()))?;
        Self::from_image(image)
    }

    /// Open an `.MSA` archive, decompressing it into a sector image.
    pub fn open_msa(filepath: impl AsRef<Path>) -> Result<Self, FloppyError> {
        let filepath = require_file(filepath.as_ref())?;
        let data = std::fs::read(&filepath).map_err(|e| FloppyError::new(e.to_string()))?;
        Self::from_msa(&data)
    }

    /// Wrap a sector image already in memory.
    pub fn from_image(image: Vec<u8>) -> Result<Self, FloppyError> {
        if image.len() < SECTOR_SIZE || !image.len().is_multiple_of(SECTOR_SIZE) {
            return Err(FloppyError::new(format!(
                "Invalid ST image size: {} bytes (not a multiple of {})",
                image.len(),
                SECTOR_SIZE
            )));
        }
        let geometry = Geometry::from_boot_sector(&image[..SECTOR_SIZE], image.len())
            .or_else(|| Geometry::from_size(image.len()))
            .ok_or_else(|| {
                FloppyError::new(format!(
                    "cannot determine the disk geometry of a {}-byte image without a \
                     boot sector",
                    image.len()
                ))
            })?;
        Ok(Self { image, geometry })
    }

    /// Decode an `.MSA` archive.
    pub fn from_msa(data: &[u8]) -> Result<Self, FloppyError> {
        if data.len() < 10 || u16::from_be_bytes([data[0], data[1]]) != MSA_MAGIC {
            return Err(FloppyError::new("not an MSA archive (bad magic)"));
        }
        let word = |at: usize| u16::from_be_bytes([data[at], data[at + 1]]) as u32;
        let sectors_per_track = word(2);
        let sides = word(4) + 1;
        let (first, last) = (word(6), word(8));
        // Bounds of real Atari disks (ED: 36 sectors, at most 86 tracks).
        // The image is preallocated from these fields, so an unchecked
        // header could ask for terabytes.
        if sectors_per_track == 0
            || sectors_per_track > 36
            || sides > 2
            || last < first
            || last > 85
        {
            return Err(FloppyError::new("MSA header describes an impossible disk"));
        }
        let track_len = sectors_per_track as usize * SECTOR_SIZE;
        let mut image =
            Vec::with_capacity((last - first + 1) as usize * sides as usize * track_len);
        let mut at = 10usize;
        for _ in first..=last {
            for _ in 0..sides {
                let len = *data
                    .get(at..at + 2)
                    .and_then(|l| <&[u8; 2]>::try_from(l).ok())
                    .ok_or_else(|| FloppyError::new("MSA archive ends inside a track header"))?;
                let len = u16::from_be_bytes(len) as usize;
                at += 2;
                let track = data
                    .get(at..at + len)
                    .ok_or_else(|| FloppyError::new("MSA archive ends inside a track"))?;
                at += len;
                if len == track_len {
                    image.extend_from_slice(track);
                } else {
                    let start = image.len();
                    let mut k = 0;
                    while k < track.len() {
                        if track[k] == MSA_RUN {
                            let [_, byte, hi, lo] = *track
                                .get(k..k + 4)
                                .and_then(|r| <&[u8; 4]>::try_from(r).ok())
                                .ok_or_else(|| {
                                    FloppyError::new("MSA run marker at the end of a track")
                                })?;
                            let run = u16::from_be_bytes([hi, lo]) as usize;
                            // Checked per run: a track of nothing but runs
                            // would otherwise expand 16384-fold before the
                            // length check after the loop.
                            if image.len() - start + run > track_len {
                                return Err(FloppyError::new(
                                    "MSA track decompresses past its length",
                                ));
                            }
                            image.resize(image.len() + run, byte);
                            k += 4;
                        } else {
                            if image.len() - start >= track_len {
                                return Err(FloppyError::new(
                                    "MSA track decompresses past its length",
                                ));
                            }
                            image.push(track[k]);
                            k += 1;
                        }
                    }
                    if image.len() - start != track_len {
                        return Err(FloppyError::new(format!(
                            "MSA track decompresses to {} bytes, expected {}",
                            image.len() - start,
                            track_len
                        )));
                    }
                }
            }
        }
        // The archive's own geometry outranks whatever the boot sector
        // says; a partial archive (first track > 0) is still addressed by
        // its own tracks.
        Ok(Self {
            image,
            geometry: Geometry {
                sectors_per_track,
                sides,
                tracks: last - first + 1,
            },
        })
    }
}

impl FloppyImageReader for StBackend {
    fn read_sector(&mut self, track: u32, side: u32, sector: u32) -> Result<Vec<u8>, FloppyError> {
        let g = self.geometry;
        if track >= g.tracks || side >= g.sides || sector >= g.sectors_per_track {
            return Err(FloppyError::new(format!(
                "sector {}/{}/{} outside the disk ({} tracks, {} sides, {} sectors per track)",
                track, side, sector, g.tracks, g.sides, g.sectors_per_track
            )));
        }
        let index = ((track * g.sides + side) * g.sectors_per_track + sector) as usize;
        let start = index * SECTOR_SIZE;
        self.image
            .get(start..start + SECTOR_SIZE)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| FloppyError::new("sector lies beyond the end of the image"))
    }
}

#[cfg(test)]
pub(crate) mod tests {

    #[test]
    fn msa_header_and_runs_cannot_force_huge_allocations() {
        let header = |spt: u16, last: u16| {
            let mut h = vec![0x0E, 0x0F];
            for w in [spt, 0, 0, last] {
                h.extend_from_slice(&w.to_be_bytes());
            }
            h
        };
        assert!(StBackend::from_msa(&header(0xFFFF, 0xFFFF)).is_err());
        assert!(StBackend::from_msa(&header(9, 200)).is_err());
        // One track of 9 sectors whose runs expand far past 4608 bytes.
        let mut msa = header(9, 0);
        let runs: Vec<u8> = (0..64).flat_map(|_| [0xE5, 0x00, 0xFF, 0xFF]).collect();
        msa.extend_from_slice(&(runs.len() as u16).to_be_bytes());
        msa.extend_from_slice(&runs);
        assert!(StBackend::from_msa(&msa).is_err());
    }
    use super::*;

    /// A boot sector whose BPB describes `spt` sectors per track on
    /// `sides` sides with `total` sectors in all.
    pub(crate) fn boot_sector(spt: u16, sides: u16, total: u16) -> Vec<u8> {
        let mut boot = vec![0u8; SECTOR_SIZE];
        boot[11..13].copy_from_slice(&512u16.to_le_bytes());
        boot[13] = 2; // sectors per cluster
        boot[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved
        boot[16] = 2; // FATs
        boot[17..19].copy_from_slice(&112u16.to_le_bytes()); // root entries
        boot[19..21].copy_from_slice(&total.to_le_bytes());
        boot[21] = 0xF9;
        boot[22..24].copy_from_slice(&3u16.to_le_bytes()); // sectors per FAT
        boot[24..26].copy_from_slice(&spt.to_le_bytes());
        boot[26..28].copy_from_slice(&sides.to_le_bytes());
        boot
    }

    #[test]
    fn geometry_comes_from_the_boot_sector() {
        let mut image = boot_sector(10, 2, 1600);
        image.resize(1600 * SECTOR_SIZE, 0);
        // Track 1, side 1, sector 3: the fourth sector of the third track-side.
        image[(3 * 10 + 3) * SECTOR_SIZE] = 0xAB;
        let mut st = StBackend::from_image(image).unwrap();
        assert_eq!(
            st.geometry,
            Geometry {
                sectors_per_track: 10,
                sides: 2,
                tracks: 80
            }
        );
        assert_eq!(st.read_sector(1, 1, 3).unwrap()[0], 0xAB);
        assert!(st.read_sector(0, 0, 10).is_err());
    }

    #[test]
    fn geometry_falls_back_to_the_size() {
        let st = StBackend::from_image(vec![0u8; 80 * 9 * SECTOR_SIZE]).unwrap();
        assert_eq!(st.geometry.sides, 1);
        assert_eq!(st.geometry.sectors_per_track, 9);
        assert!(StBackend::from_image(vec![0u8; 7 * SECTOR_SIZE]).is_err());
        assert!(StBackend::from_image(vec![0u8; 100]).is_err());
    }

    #[test]
    fn msa_tracks_are_decompressed() {
        // One track, one side, two sectors: the first stored raw, the
        // second as a run of $E5 bytes followed by literals.
        let mut msa = Vec::new();
        msa.extend_from_slice(&MSA_MAGIC.to_be_bytes());
        msa.extend_from_slice(&2u16.to_be_bytes()); // sectors per track
        msa.extend_from_slice(&0u16.to_be_bytes()); // one side
        msa.extend_from_slice(&0u16.to_be_bytes());
        msa.extend_from_slice(&0u16.to_be_bytes());
        let mut track = Vec::new();
        track.extend_from_slice(&[MSA_RUN, 0x11, 0x03, 0xFC]); // 1020 x $11
        track.extend_from_slice(&[MSA_RUN, MSA_RUN, 0x00, 0x01]); // one literal $E5
        track.extend_from_slice(&[0x22, 0x33, 0x44]);
        msa.extend_from_slice(&(track.len() as u16).to_be_bytes());
        msa.extend_from_slice(&track);
        let mut st = StBackend::from_msa(&msa).unwrap();
        let second = st.read_sector(0, 0, 1).unwrap();
        assert_eq!(&second[508..], &[MSA_RUN, 0x22, 0x33, 0x44]);
        assert_eq!(st.read_sector(0, 0, 0).unwrap(), vec![0x11; 512]);

        msa.truncate(msa.len() - 2);
        assert!(StBackend::from_msa(&msa).is_err());
    }
}
