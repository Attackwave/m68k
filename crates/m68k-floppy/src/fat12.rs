//! FAT12, the filesystem of Atari ST (and PC) floppies, read through any
//! [`FloppyImageReader`]. Writing is in [`crate::fat12_write`].
//!
//! The boot sector's BIOS parameter block gives the layout in logical
//! sectors: reserved sectors, then the FATs, then the fixed-size root
//! directory, then the data area in clusters. Logical sector `n` is
//! track `n / (sectors per track * sides)`, side `(n / sectors per
//! track) % sides`, sector `n % sectors per track`, which is how any
//! [`FloppyImageReader`] is addressed.
//!
//! A directory entry is 32 bytes: an 8.3 name, an attribute byte, the
//! first cluster and the size. Long-name entries, volume labels and the
//! `.`/`..` entries are skipped.

use crate::amigados::DirEntry;
use crate::floppy_base::{FloppyError, FloppyImageReader};

pub(crate) const SECTOR_SIZE: usize = 512;
pub(crate) const ENTRY_SIZE: usize = 32;
pub(crate) const ATTR_DIRECTORY: u8 = 0x10;
pub(crate) const ATTR_VOLUME: u8 = 0x08;
const ATTR_LONG_NAME: u8 = 0x0F;
pub(crate) const FIRST_DATA_CLUSTER: u32 = 2;
const END_OF_CHAIN: u32 = 0xFF8;

/// The layout a boot sector's BIOS parameter block describes, in logical
/// sectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout {
    pub sectors_per_track: u32,
    pub sides: u32,
    pub sectors_per_cluster: u32,
    pub reserved: u32,
    pub fats: u32,
    pub sectors_per_fat: u32,
    pub root_start: u32,
    pub root_sectors: u32,
    pub data_start: u32,
    pub total_sectors: u32,
    /// Clusters in the data area, the bound on any chain's length.
    pub cluster_count: u32,
}

impl Layout {
    pub(crate) fn parse(boot: &[u8]) -> Result<Self, FloppyError> {
        if boot.len() < 32 {
            return Err(FloppyError::new("boot sector is shorter than 32 bytes"));
        }
        let bytes_per_sector = le16(boot, 11) as u32;
        let sectors_per_cluster = boot[13] as u32;
        let reserved = le16(boot, 14) as u32;
        let fats = boot[16] as u32;
        let root_entries = le16(boot, 17) as u32;
        let total_sectors = le16(boot, 19) as u32;
        let sectors_per_fat = le16(boot, 22) as u32;
        let sectors_per_track = le16(boot, 24) as u32;
        let sides = le16(boot, 26) as u32;
        if bytes_per_sector != SECTOR_SIZE as u32
            || sectors_per_cluster == 0
            || fats == 0
            || sectors_per_fat == 0
            || sectors_per_track == 0
            || !(1..=2).contains(&sides)
            || total_sectors == 0
        {
            return Err(FloppyError::new(
                "not a FAT12 floppy (boot sector has no usable BIOS parameter block)",
            ));
        }
        let root_start = reserved + fats * sectors_per_fat;
        let root_sectors = (root_entries * ENTRY_SIZE as u32).div_ceil(SECTOR_SIZE as u32);
        let data_start = root_start + root_sectors;
        if data_start >= total_sectors {
            return Err(FloppyError::new(
                "FAT12 layout leaves no room for a data area",
            ));
        }
        Ok(Self {
            sectors_per_track,
            sides,
            sectors_per_cluster,
            reserved,
            fats,
            sectors_per_fat,
            root_start,
            root_sectors,
            data_start,
            total_sectors,
            cluster_count: (total_sectors - data_start) / sectors_per_cluster,
        })
    }

    /// The 12-bit FAT entry for `cluster` in `fat`, if the FAT is long
    /// enough to hold it.
    pub(crate) fn fat_entry(fat: &[u8], cluster: u32) -> Option<u32> {
        let at = (cluster as usize * 3) / 2;
        let pair = fat.get(at..at + 2)?;
        let value = u16::from_le_bytes([pair[0], pair[1]]);
        Some(if cluster.is_multiple_of(2) {
            (value & 0x0FFF) as u32
        } else {
            (value >> 4) as u32
        })
    }

    /// The clusters of a chain starting at `first`, in order, following
    /// `next`. A chain longer than the disk has clusters is a loop and an
    /// error.
    pub(crate) fn chain(
        &self,
        first: u32,
        next: impl Fn(u32) -> Option<u32>,
    ) -> Result<Vec<u32>, FloppyError> {
        let mut clusters = Vec::new();
        let mut cluster = first;
        while (FIRST_DATA_CLUSTER..FIRST_DATA_CLUSTER + self.cluster_count).contains(&cluster) {
            clusters.push(cluster);
            if clusters.len() > self.cluster_count as usize {
                return Err(FloppyError::new(format!(
                    "cluster chain starting at {} loops",
                    first
                )));
            }
            cluster = next(cluster).ok_or_else(|| {
                FloppyError::new(format!("cluster {} is beyond the FAT", cluster))
            })?;
        }
        if cluster < END_OF_CHAIN && !clusters.is_empty() && cluster != 0 {
            return Err(FloppyError::new(format!(
                "cluster chain starting at {} runs into reserved entry {:#05x}",
                first, cluster
            )));
        }
        Ok(clusters)
    }

    /// First logical sector of `cluster`.
    pub(crate) fn cluster_sector(&self, cluster: u32) -> u32 {
        self.data_start + (cluster - FIRST_DATA_CLUSTER) * self.sectors_per_cluster
    }
}

/// A mounted FAT12 filesystem over a floppy image. In the [`DirEntry`]s
/// it hands out, `block` is the entry's first cluster.
pub struct Fat12<'a> {
    reader: &'a mut dyn FloppyImageReader,
    layout: Layout,
    fat: Vec<u8>,
}

pub(crate) fn le16(block: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([block[at], block[at + 1]])
}

/// The directory entries of a region: the root directory's sectors, or a
/// subdirectory's cluster chain. Deleted, long-name, volume-label and
/// `.`/`..` entries are skipped.
pub(crate) fn entries_in(region: &[u8]) -> Vec<DirEntry> {
    let mut entries = Vec::new();
    for raw in region.chunks_exact(ENTRY_SIZE) {
        match raw[0] {
            0x00 => break,
            0xE5 => continue,
            _ => {}
        }
        let attr = raw[11];
        if attr & ATTR_LONG_NAME == ATTR_LONG_NAME || attr & ATTR_VOLUME != 0 {
            continue;
        }
        let base = String::from_utf8_lossy(&raw[0..8]).trim_end().to_string();
        let ext = String::from_utf8_lossy(&raw[8..11]).trim_end().to_string();
        if base == "." || base == ".." {
            continue;
        }
        // $05 stands in for a leading $E5, which would mean "deleted".
        let base = match base.strip_prefix('\u{5}') {
            Some(rest) => format!("\u{e5}{rest}"),
            None => base,
        };
        let name = if ext.is_empty() {
            base
        } else {
            format!("{base}.{ext}")
        };
        let is_dir = attr & ATTR_DIRECTORY != 0;
        entries.push(DirEntry {
            name,
            is_dir,
            size: if is_dir {
                0
            } else {
                u32::from_le_bytes(raw[28..32].try_into().unwrap())
            },
            block: le16(raw, 26) as u32,
        });
    }
    entries
}

impl<'a> Fat12<'a> {
    /// Mount the filesystem by its boot sector.
    pub fn mount(reader: &'a mut dyn FloppyImageReader) -> Result<Self, FloppyError> {
        let boot = reader.read_sector(0, 0, 0)?;
        let layout = Layout::parse(&boot)?;
        let mut fs = Self {
            reader,
            layout,
            fat: Vec::new(),
        };
        fs.fat = fs.read_logical(layout.reserved, layout.sectors_per_fat)?;
        Ok(fs)
    }

    /// `count` logical sectors from `start`.
    fn read_logical(&mut self, start: u32, count: u32) -> Result<Vec<u8>, FloppyError> {
        let (spt, sides) = (self.layout.sectors_per_track, self.layout.sides);
        let mut out = Vec::with_capacity(count as usize * SECTOR_SIZE);
        for n in start..start + count {
            out.extend(
                self.reader
                    .read_sector(n / (spt * sides), (n / spt) % sides, n % spt)?,
            );
        }
        Ok(out)
    }

    /// The bytes of a whole cluster chain.
    fn read_chain(&mut self, first: u32) -> Result<Vec<u8>, FloppyError> {
        let chain = self
            .layout
            .chain(first, |c| Layout::fat_entry(&self.fat, c))?;
        let mut out = Vec::new();
        for cluster in chain {
            let start = self.layout.cluster_sector(cluster);
            out.extend(self.read_logical(start, self.layout.sectors_per_cluster)?);
        }
        Ok(out)
    }

    /// List the root directory.
    pub fn list_root(&mut self) -> Result<Vec<DirEntry>, FloppyError> {
        let region = self.read_logical(self.layout.root_start, self.layout.root_sectors)?;
        Ok(entries_in(&region))
    }

    /// List the directory whose first cluster is `cluster`.
    pub fn list_dir(&mut self, cluster: u32) -> Result<Vec<DirEntry>, FloppyError> {
        let region = self.read_chain(cluster)?;
        Ok(entries_in(&region))
    }

    /// The entry at `path` (components separated by `/` or `\`, matched
    /// without regard to case), if it exists. An empty path is the root.
    pub fn resolve_path(&mut self, path: &str) -> Result<Option<DirEntry>, FloppyError> {
        let mut current: Option<DirEntry> = None;
        for component in path.split(['/', '\\']).filter(|c| !c.is_empty()) {
            let entries = match &current {
                None => self.list_root()?,
                Some(dir) if dir.is_dir => self.list_dir(dir.block)?,
                Some(_) => return Ok(None),
            };
            match entries
                .into_iter()
                .find(|e| e.name.eq_ignore_ascii_case(component))
            {
                Some(entry) => current = Some(entry),
                None => return Ok(None),
            }
        }
        Ok(current)
    }

    /// List the directory at `path`.
    pub fn list_dir_at_path(&mut self, path: &str) -> Result<Vec<DirEntry>, FloppyError> {
        match self.resolve_path(path)? {
            None if path.trim_matches(['/', '\\']).is_empty() => self.list_root(),
            None => Err(FloppyError::new(format!("path not found: {}", path))),
            Some(entry) if entry.is_dir => self.list_dir(entry.block),
            Some(_) => Err(FloppyError::new(format!("not a directory: {}", path))),
        }
    }

    /// The contents of a file entry.
    pub fn read_file(&mut self, entry: &DirEntry) -> Result<Vec<u8>, FloppyError> {
        if entry.is_dir {
            return Err(FloppyError::new(format!("is a directory: {}", entry.name)));
        }
        let mut data = self.read_chain(entry.block)?;
        data.truncate(entry.size as usize);
        Ok(data)
    }

    /// The contents of the file at `path`.
    pub fn read_file_at_path(&mut self, path: &str) -> Result<Vec<u8>, FloppyError> {
        match self.resolve_path(path)? {
            Some(entry) if !entry.is_dir => self.read_file(&entry),
            Some(_) => Err(FloppyError::new(format!("is a directory: {}", path))),
            None => Err(FloppyError::new(format!("path not found: {}", path))),
        }
    }

    /// Every entry below `path`, with its full path, directories before
    /// their contents, no deeper than `max_depth` levels.
    pub fn walk(
        &mut self,
        path: &str,
        max_depth: usize,
    ) -> Result<Vec<(String, DirEntry)>, FloppyError> {
        let mut out = Vec::new();
        let prefix = path.trim_matches(['/', '\\']);
        let entries = self.list_dir_at_path(prefix)?;
        let mut stack: Vec<(String, DirEntry, usize)> = entries
            .into_iter()
            .rev()
            .map(|e| (prefix.to_string(), e, 1))
            .collect();
        while let Some((dir, entry, depth)) = stack.pop() {
            let full = if dir.is_empty() {
                entry.name.clone()
            } else {
                format!("{dir}/{}", entry.name)
            };
            if entry.is_dir && depth < max_depth {
                let children = self.list_dir(entry.block)?;
                stack.extend(
                    children
                        .into_iter()
                        .rev()
                        .map(|c| (full.clone(), c, depth + 1)),
                );
            }
            out.push((full, entry));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::st::StBackend;

    /// A 720K disk: 2 sectors per cluster, 1 reserved sector, 2 FATs of
    /// 3 sectors, 112 root entries (7 sectors); data starts at sector 14.
    fn disk() -> Vec<u8> {
        let mut image = crate::st::tests::boot_sector(9, 2, 1440);
        image.resize(1440 * SECTOR_SIZE, 0);
        // FAT: media byte entries, then cluster 2 -> 3 -> end (a file of
        // three sectors), cluster 4 = end (subdirectory), cluster 5 = end
        // (file in the subdirectory).
        let chain: [u32; 6] = [0xFF9, 0xFFF, 3, 0xFFF, 0xFFF, 0xFFF];
        for fat in [1usize, 4] {
            let base = fat * SECTOR_SIZE;
            for (n, value) in chain.iter().enumerate() {
                let at = base + n * 3 / 2;
                let pair = u16::from_le_bytes([image[at], image[at + 1]]);
                let pair = if n.is_multiple_of(2) {
                    (pair & 0xF000) | *value as u16
                } else {
                    (pair & 0x000F) | ((*value as u16) << 4)
                };
                image[at..at + 2].copy_from_slice(&pair.to_le_bytes());
            }
        }
        let entry = |name: &[u8; 11], attr: u8, cluster: u16, size: u32| {
            let mut e = [0u8; 32];
            e[..11].copy_from_slice(name);
            e[11] = attr;
            e[26..28].copy_from_slice(&cluster.to_le_bytes());
            e[28..32].copy_from_slice(&size.to_le_bytes());
            e
        };
        let root = 7 * SECTOR_SIZE;
        image[root..root + 32].copy_from_slice(&entry(b"DISK       ", ATTR_VOLUME, 0, 0));
        image[root + 32..root + 64].copy_from_slice(&entry(b"HELLO   PRG", 0x20, 2, 1500));
        image[root + 64..root + 96].copy_from_slice(&entry(b"AUTO       ", ATTR_DIRECTORY, 4, 0));
        image[root + 96..root + 128].copy_from_slice(&entry(b"\xE5OLD    TXT", 0x20, 5, 1));
        // Data: cluster n at sector 14 + (n - 2) * 2.
        let cluster = |n: usize| (14 + (n - 2) * 2) * SECTOR_SIZE;
        for (i, b) in image[cluster(2)..cluster(2) + 2048].iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let sub = cluster(4);
        image[sub..sub + 32].copy_from_slice(&entry(b".          ", ATTR_DIRECTORY, 4, 0));
        image[sub + 32..sub + 64].copy_from_slice(&entry(b"..         ", ATTR_DIRECTORY, 0, 0));
        image[sub + 64..sub + 96].copy_from_slice(&entry(b"BOOT    PRG", 0x20, 5, 4));
        image[cluster(5)..cluster(5) + 4].copy_from_slice(b"boot");
        image
    }

    #[test]
    fn lists_reads_and_walks() {
        let mut st = StBackend::from_image(disk()).unwrap();
        let mut fs = Fat12::mount(&mut st).unwrap();
        let root = fs.list_root().unwrap();
        let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["HELLO.PRG", "AUTO"]);
        assert_eq!(root[0].size, 1500);
        assert!(root[1].is_dir);

        let data = fs.read_file_at_path("hello.prg").unwrap();
        assert_eq!(data.len(), 1500);
        assert_eq!(data[1499], (1499 % 251) as u8);
        assert_eq!(fs.read_file_at_path("AUTO\\BOOT.PRG").unwrap(), b"boot");
        assert!(fs.read_file_at_path("auto").is_err());
        assert!(fs.read_file_at_path("missing").is_err());

        let walked: Vec<String> = fs
            .walk("", 64)
            .unwrap()
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        assert_eq!(walked, ["HELLO.PRG", "AUTO", "AUTO/BOOT.PRG"]);
        assert_eq!(fs.list_dir_at_path("/AUTO/").unwrap()[0].name, "BOOT.PRG");
    }

    #[test]
    fn a_looping_chain_is_an_error() {
        let mut image = disk();
        // Cluster 3 -> 2 instead of end of chain (entry 3 is odd: high 12 bits).
        let at = SECTOR_SIZE + 4;
        let pair = u16::from_le_bytes([image[at], image[at + 1]]) & 0x000F | (2 << 4);
        image[at..at + 2].copy_from_slice(&pair.to_le_bytes());
        let mut st = StBackend::from_image(image).unwrap();
        let mut fs = Fat12::mount(&mut st).unwrap();
        assert!(
            fs.read_file_at_path("HELLO.PRG")
                .unwrap_err()
                .0
                .contains("loops")
        );
    }

    #[test]
    fn an_amiga_bootblock_is_not_fat() {
        let mut image = vec![0u8; 1440 * SECTOR_SIZE];
        image[..3].copy_from_slice(b"DOS");
        let mut st = StBackend::from_image(image).unwrap();
        assert!(Fat12::mount(&mut st).is_err());
    }
}
