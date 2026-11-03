//! Writing FAT12 on a raw Atari ST sector image (`.ST`).
//!
//! An `.ST` file stores the sectors track by track, side by side within a
//! track, which is exactly the order of FAT's logical sectors, so logical
//! sector `n` is the 512 bytes at offset `n * 512` and the writer works on
//! the image buffer directly. The layout comes from the boot sector's
//! BIOS parameter block, as for reading ([`crate::fat12`]).
//!
//! Every change goes to all FAT copies. Names are 8.3, stored in upper
//! case; date and time fields are left at zero.

use crate::amigados::DirEntry;
use crate::fat12::{
    ATTR_DIRECTORY, ATTR_VOLUME, ENTRY_SIZE, FIRST_DATA_CLUSTER, Layout, SECTOR_SIZE, entries_in,
    le16,
};
use crate::floppy_base::FloppyError;

const ATTR_ARCHIVE: u8 = 0x20;
const FREE_ENTRY: u8 = 0xE5;
/// Characters a short name may not contain, besides control characters
/// and anything outside ASCII.
const INVALID_NAME_CHARS: &[u8] = b"\"*+,/:;<=>?[\\]| .";

/// A directory: the fixed root area, or a subdirectory's cluster chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Root,
    Chain(u32),
}

/// Where an entry lives: its directory and its byte offset in that
/// directory's region.
struct Slot {
    dir: Dir,
    at: usize,
}

/// A FAT12 filesystem on a raw sector image, opened for writing.
pub struct Fat12Writer<'a> {
    image: &'a mut Vec<u8>,
    layout: Layout,
}

/// Encode a file name as the 11-byte 8.3 field of a directory entry.
fn short_name(name: &str) -> Result<[u8; 11], FloppyError> {
    let bad = || FloppyError::new(format!("not a valid 8.3 name: {:?}", name));
    let (base, ext) = match name.rsplit_once('.') {
        Some((base, ext)) => (base, ext),
        None => (name, ""),
    };
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return Err(bad());
    }
    let valid = |b: &u8| b.is_ascii() && !b.is_ascii_control() && !INVALID_NAME_CHARS.contains(b);
    if !base.bytes().chain(ext.bytes()).all(|b| valid(&b)) {
        return Err(bad());
    }
    let mut field = [b' '; 11];
    for (dst, src) in field[..8].iter_mut().zip(base.bytes()) {
        *dst = src.to_ascii_uppercase();
    }
    for (dst, src) in field[8..].iter_mut().zip(ext.bytes()) {
        *dst = src.to_ascii_uppercase();
    }
    if field[0] == FREE_ENTRY {
        field[0] = 0x05;
    }
    Ok(field)
}

impl<'a> Fat12Writer<'a> {
    /// Open the filesystem on `image` for writing.
    pub fn mount(image: &'a mut Vec<u8>) -> Result<Self, FloppyError> {
        if image.len() < SECTOR_SIZE {
            return Err(FloppyError::new("image is shorter than a sector"));
        }
        let layout = Layout::parse(&image[..SECTOR_SIZE])?;
        if image.len() < layout.total_sectors as usize * SECTOR_SIZE {
            return Err(FloppyError::new(format!(
                "image holds {} sectors but its boot sector describes {}",
                image.len() / SECTOR_SIZE,
                layout.total_sectors
            )));
        }
        Ok(Self { image, layout })
    }

    pub fn into_image(self) -> &'a mut Vec<u8> {
        self.image
    }

    fn sectors(&self, start: u32, count: u32) -> std::ops::Range<usize> {
        let from = start as usize * SECTOR_SIZE;
        from..from + count as usize * SECTOR_SIZE
    }

    fn fat_entry(&self, cluster: u32) -> Option<u32> {
        let fat = &self.image[self.sectors(self.layout.reserved, self.layout.sectors_per_fat)];
        Layout::fat_entry(fat, cluster)
    }

    /// Set `cluster`'s FAT entry in every FAT copy.
    fn set_fat_entry(&mut self, cluster: u32, value: u32) -> Result<(), FloppyError> {
        let at = (cluster as usize * 3) / 2;
        if at + 2 > self.layout.sectors_per_fat as usize * SECTOR_SIZE {
            return Err(FloppyError::new(format!(
                "cluster {} is beyond the FAT",
                cluster
            )));
        }
        for copy in 0..self.layout.fats {
            let base = (self.layout.reserved + copy * self.layout.sectors_per_fat) as usize
                * SECTOR_SIZE
                + at;
            let pair = u16::from_le_bytes([self.image[base], self.image[base + 1]]);
            let pair = if cluster.is_multiple_of(2) {
                (pair & 0xF000) | (value as u16 & 0x0FFF)
            } else {
                (pair & 0x000F) | ((value as u16 & 0x0FFF) << 4)
            };
            self.image[base..base + 2].copy_from_slice(&pair.to_le_bytes());
        }
        Ok(())
    }

    fn chain(&self, first: u32) -> Result<Vec<u32>, FloppyError> {
        self.layout.chain(first, |c| self.fat_entry(c))
    }

    /// Free clusters on the disk.
    pub fn free_clusters(&self) -> u32 {
        (FIRST_DATA_CLUSTER..FIRST_DATA_CLUSTER + self.layout.cluster_count)
            .filter(|&c| self.fat_entry(c) == Some(0))
            .count() as u32
    }

    /// Allocate `count` clusters as one chain and return them.
    fn allocate(&mut self, count: usize) -> Result<Vec<u32>, FloppyError> {
        let free: Vec<u32> = (FIRST_DATA_CLUSTER..FIRST_DATA_CLUSTER + self.layout.cluster_count)
            .filter(|&c| self.fat_entry(c) == Some(0))
            .take(count)
            .collect();
        if free.len() < count {
            return Err(FloppyError::new(format!(
                "disk full: {} clusters needed, {} free",
                count,
                free.len()
            )));
        }
        for pair in free.windows(2) {
            self.set_fat_entry(pair[0], pair[1])?;
        }
        if let Some(&last) = free.last() {
            self.set_fat_entry(last, 0xFFF)?;
        }
        Ok(free)
    }

    fn free_chain(&mut self, first: u32) -> Result<(), FloppyError> {
        for cluster in self.chain(first)? {
            self.set_fat_entry(cluster, 0)?;
        }
        Ok(())
    }

    fn cluster_range(&self, cluster: u32) -> std::ops::Range<usize> {
        self.sectors(
            self.layout.cluster_sector(cluster),
            self.layout.sectors_per_cluster,
        )
    }

    /// The bytes of a directory.
    fn read_dir(&self, dir: Dir) -> Result<Vec<u8>, FloppyError> {
        match dir {
            Dir::Root => Ok(self.image
                [self.sectors(self.layout.root_start, self.layout.root_sectors)]
            .to_vec()),
            Dir::Chain(first) => {
                let mut out = Vec::new();
                for cluster in self.chain(first)? {
                    out.extend_from_slice(&self.image[self.cluster_range(cluster)]);
                }
                Ok(out)
            }
        }
    }

    /// Write one 32-byte entry at `slot`.
    fn write_entry(&mut self, slot: &Slot, entry: &[u8; ENTRY_SIZE]) -> Result<(), FloppyError> {
        let image_at = match slot.dir {
            Dir::Root => self.layout.root_start as usize * SECTOR_SIZE + slot.at,
            Dir::Chain(first) => {
                let per_cluster = self.layout.sectors_per_cluster as usize * SECTOR_SIZE;
                let cluster = *self
                    .chain(first)?
                    .get(slot.at / per_cluster)
                    .ok_or_else(|| FloppyError::new("directory entry beyond its chain"))?;
                self.cluster_range(cluster).start + slot.at % per_cluster
            }
        };
        self.image[image_at..image_at + ENTRY_SIZE].copy_from_slice(entry);
        Ok(())
    }

    /// The raw entry at `slot`.
    fn read_entry(&self, slot: &Slot) -> Result<[u8; ENTRY_SIZE], FloppyError> {
        let region = self.read_dir(slot.dir)?;
        region
            .get(slot.at..slot.at + ENTRY_SIZE)
            .and_then(|e| e.try_into().ok())
            .ok_or_else(|| FloppyError::new("directory entry beyond its region"))
    }

    /// The slot of the entry named `name` in `dir`, matched without
    /// regard to case, with the entry itself.
    fn find(&self, dir: Dir, name: &str) -> Result<Option<(Slot, [u8; ENTRY_SIZE])>, FloppyError> {
        let wanted = short_name(name)?;
        let region = self.read_dir(dir)?;
        for (index, raw) in region.chunks_exact(ENTRY_SIZE).enumerate() {
            match raw[0] {
                0x00 => break,
                FREE_ENTRY => continue,
                _ => {}
            }
            if raw[11] & ATTR_VOLUME != 0 {
                continue;
            }
            if raw[..11] == wanted {
                let slot = Slot {
                    dir,
                    at: index * ENTRY_SIZE,
                };
                return Ok(Some((slot, raw.try_into().unwrap())));
            }
        }
        Ok(None)
    }

    /// The directory at `path`.
    fn resolve_dir(&self, path: &str) -> Result<Dir, FloppyError> {
        let mut dir = Dir::Root;
        for component in path.split(['/', '\\']).filter(|c| !c.is_empty()) {
            let (_, entry) = self
                .find(dir, component)?
                .ok_or_else(|| FloppyError::new(format!("path not found: {}", path)))?;
            if entry[11] & ATTR_DIRECTORY == 0 {
                return Err(FloppyError::new(format!("not a directory: {}", path)));
            }
            dir = Dir::Chain(le16(&entry, 26) as u32);
        }
        Ok(dir)
    }

    /// Split `path` into its directory and its last component.
    fn parent_and_name<'p>(&self, path: &'p str) -> Result<(Dir, &'p str), FloppyError> {
        let trimmed = path.trim_matches(['/', '\\']);
        let (parent, name) = match trimmed.rsplit_once(['/', '\\']) {
            Some((parent, name)) => (parent, name),
            None => ("", trimmed),
        };
        if name.is_empty() {
            return Err(FloppyError::new(format!("no file name in {:?}", path)));
        }
        Ok((self.resolve_dir(parent)?, name))
    }

    /// A free slot in `dir`, extending a subdirectory by a cluster when
    /// it is full.
    fn free_slot(&mut self, dir: Dir) -> Result<Slot, FloppyError> {
        let region = self.read_dir(dir)?;
        if let Some(index) = region
            .chunks_exact(ENTRY_SIZE)
            .position(|raw| raw[0] == 0x00 || raw[0] == FREE_ENTRY)
        {
            return Ok(Slot {
                dir,
                at: index * ENTRY_SIZE,
            });
        }
        let Dir::Chain(first) = dir else {
            return Err(FloppyError::new("root directory is full"));
        };
        let chain = self.chain(first)?;
        let last = *chain
            .last()
            .ok_or_else(|| FloppyError::new("directory has no clusters"))?;
        let new = self.allocate(1)?[0];
        self.set_fat_entry(last, new)?;
        let range = self.cluster_range(new);
        self.image[range].fill(0);
        Ok(Slot {
            dir,
            at: region.len(),
        })
    }

    fn entry_bytes(name: [u8; 11], attr: u8, cluster: u32, size: u32) -> [u8; ENTRY_SIZE] {
        let mut e = [0u8; ENTRY_SIZE];
        e[..11].copy_from_slice(&name);
        e[11] = attr;
        e[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
        e[28..32].copy_from_slice(&size.to_le_bytes());
        e
    }

    /// Write `data` to the file at `path`, replacing an existing file.
    /// Returns the file's first cluster (0 for an empty file).
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<u32, FloppyError> {
        let (dir, name) = self.parent_and_name(path)?;
        let size = u32::try_from(data.len())
            .map_err(|_| FloppyError::new("file is larger than FAT12 can record"))?;
        // An existing file keeps its clusters until the new ones are
        // allocated, so a full disk leaves it intact rather than freed
        // and dangling. ponytail: replacing a file needs room for both
        // copies at once; reuse the old chain in place if that matters.
        let (slot, old) = match self.find(dir, name)? {
            Some((slot, entry)) => {
                if entry[11] & ATTR_DIRECTORY != 0 {
                    return Err(FloppyError::new(format!("is a directory: {}", path)));
                }
                (slot, self.chain(le16(&entry, 26) as u32)?)
            }
            None => (self.free_slot(dir)?, Vec::new()),
        };
        let per_cluster = self.layout.sectors_per_cluster as usize * SECTOR_SIZE;
        let clusters = self.allocate(data.len().div_ceil(per_cluster))?;
        for cluster in old {
            self.set_fat_entry(cluster, 0)?;
        }
        for (cluster, piece) in clusters.iter().zip(data.chunks(per_cluster)) {
            let range = self.cluster_range(*cluster);
            let dst = &mut self.image[range];
            dst[..piece.len()].copy_from_slice(piece);
            dst[piece.len()..].fill(0);
        }
        let first = clusters.first().copied().unwrap_or(0);
        let entry = Self::entry_bytes(short_name(name)?, ATTR_ARCHIVE, first, size);
        self.write_entry(&slot, &entry)?;
        Ok(first)
    }

    /// Delete the file at `path`.
    pub fn delete_file(&mut self, path: &str) -> Result<(), FloppyError> {
        let (dir, name) = self.parent_and_name(path)?;
        let (slot, mut entry) = self
            .find(dir, name)?
            .ok_or_else(|| FloppyError::new(format!("path not found: {}", path)))?;
        if entry[11] & ATTR_DIRECTORY != 0 {
            return Err(FloppyError::new(format!("is a directory: {}", path)));
        }
        let first = le16(&entry, 26) as u32;
        if first != 0 {
            self.free_chain(first)?;
        }
        entry[0] = FREE_ENTRY;
        self.write_entry(&slot, &entry)
    }

    /// Create the directory at `path`, whose parent must exist. Returns
    /// its first cluster.
    pub fn create_dir(&mut self, path: &str) -> Result<u32, FloppyError> {
        let (dir, name) = self.parent_and_name(path)?;
        if self.find(dir, name)?.is_some() {
            return Err(FloppyError::new(format!("already exists: {}", path)));
        }
        let slot = self.free_slot(dir)?;
        let cluster = self.allocate(1)?[0];
        let range = self.cluster_range(cluster);
        self.image[range.clone()].fill(0);
        let parent = match dir {
            Dir::Root => 0,
            Dir::Chain(first) => first,
        };
        let dot = Self::entry_bytes(*b".          ", ATTR_DIRECTORY, cluster, 0);
        let dotdot = Self::entry_bytes(*b"..         ", ATTR_DIRECTORY, parent, 0);
        self.image[range.start..range.start + ENTRY_SIZE].copy_from_slice(&dot);
        self.image[range.start + ENTRY_SIZE..range.start + 2 * ENTRY_SIZE].copy_from_slice(&dotdot);
        let entry = Self::entry_bytes(short_name(name)?, ATTR_DIRECTORY, cluster, 0);
        self.write_entry(&slot, &entry)?;
        Ok(cluster)
    }

    /// Delete the empty directory at `path`.
    pub fn delete_dir(&mut self, path: &str) -> Result<(), FloppyError> {
        let (dir, name) = self.parent_and_name(path)?;
        let (slot, mut entry) = self
            .find(dir, name)?
            .ok_or_else(|| FloppyError::new(format!("path not found: {}", path)))?;
        if entry[11] & ATTR_DIRECTORY == 0 {
            return Err(FloppyError::new(format!("not a directory: {}", path)));
        }
        let first = le16(&entry, 26) as u32;
        if !entries_in(&self.read_dir(Dir::Chain(first))?).is_empty() {
            return Err(FloppyError::new(format!("directory not empty: {}", path)));
        }
        self.free_chain(first)?;
        entry[0] = FREE_ENTRY;
        self.write_entry(&slot, &entry)
    }

    /// Rename or move the entry at `from` to `to`.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), FloppyError> {
        let (from_dir, from_name) = self.parent_and_name(from)?;
        let (to_dir, to_name) = self.parent_and_name(to)?;
        let (old_slot, mut entry) = self
            .find(from_dir, from_name)?
            .ok_or_else(|| FloppyError::new(format!("path not found: {}", from)))?;
        if self.find(to_dir, to_name)?.is_some() {
            return Err(FloppyError::new(format!("already exists: {}", to)));
        }
        let name = short_name(to_name)?;
        if from_dir == to_dir {
            entry[..11].copy_from_slice(&name);
            return self.write_entry(&old_slot, &entry);
        }
        let mut moved = entry;
        moved[..11].copy_from_slice(&name);
        let new_slot = self.free_slot(to_dir)?;
        self.write_entry(&new_slot, &moved)?;
        entry[0] = FREE_ENTRY;
        self.write_entry(&old_slot, &entry)?;
        if moved[11] & ATTR_DIRECTORY != 0 {
            // The moved directory's `..` must name its new parent.
            let parent = match to_dir {
                Dir::Root => 0,
                Dir::Chain(first) => first,
            };
            let dotdot = Slot {
                dir: Dir::Chain(le16(&moved, 26) as u32),
                at: ENTRY_SIZE,
            };
            let mut raw = self.read_entry(&dotdot)?;
            if raw.starts_with(b"..") {
                raw[26..28].copy_from_slice(&(parent as u16).to_le_bytes());
                self.write_entry(&dotdot, &raw)?;
            }
        }
        Ok(())
    }

    /// The entries of the directory at `path`.
    pub fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, FloppyError> {
        Ok(entries_in(&self.read_dir(self.resolve_dir(path)?)?))
    }
}

/// An empty 80-track, 9-sector FAT12 disk with `sides` sides (1 or 2),
/// labelled `label` when it is not empty, as TOS formats it. The boot
/// sector carries no boot code and no valid boot checksum.
pub fn format_empty_st_disk(label: &str, sides: u32) -> Result<Vec<u8>, FloppyError> {
    if !(1..=2).contains(&sides) {
        return Err(FloppyError::new("a floppy has one or two sides"));
    }
    let total: u16 = (80 * 9 * sides) as u16;
    let (sectors_per_fat, media): (u16, u8) = if sides == 2 { (3, 0xF9) } else { (2, 0xF8) };
    let mut image = vec![0u8; total as usize * SECTOR_SIZE];
    let boot = &mut image[..SECTOR_SIZE];
    boot[0..2].copy_from_slice(&[0x60, 0x1C]); // bra.s past the parameter block
    boot[2..8].copy_from_slice(b"Loader");
    boot[11..13].copy_from_slice(&(SECTOR_SIZE as u16).to_le_bytes());
    boot[13] = 2; // sectors per cluster
    boot[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved
    boot[16] = 2; // FATs
    boot[17..19].copy_from_slice(&112u16.to_le_bytes()); // root entries
    boot[19..21].copy_from_slice(&total.to_le_bytes());
    boot[21] = media;
    boot[22..24].copy_from_slice(&sectors_per_fat.to_le_bytes());
    boot[24..26].copy_from_slice(&9u16.to_le_bytes());
    boot[26..28].copy_from_slice(&(sides as u16).to_le_bytes());
    // TOS boots a sector whose big-endian word sum is $1234; make sure an
    // empty disk is not taken for a boot disk.
    let sum: u16 = boot
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0u16, |acc, w| acc.wrapping_add(u16::from_be_bytes(*w)));
    if sum == 0x1234 {
        boot[510] ^= 0x01;
    }
    for copy in 0..2u32 {
        let at = (1 + copy * sectors_per_fat as u32) as usize * SECTOR_SIZE;
        image[at..at + 3].copy_from_slice(&[media, 0xFF, 0xFF]);
    }
    if !label.is_empty() {
        let mut field = [b' '; 11];
        for (dst, src) in field.iter_mut().zip(label.bytes()) {
            if !src.is_ascii_alphanumeric() && !b"_-".contains(&src) {
                return Err(FloppyError::new(format!(
                    "invalid volume label: {:?}",
                    label
                )));
            }
            *dst = src.to_ascii_uppercase();
        }
        if label.len() > 11 {
            return Err(FloppyError::new(
                "volume label is longer than 11 characters",
            ));
        }
        let root = (1 + 2 * sectors_per_fat as usize) * SECTOR_SIZE;
        let entry = Fat12Writer::entry_bytes(field, ATTR_VOLUME, 0, 0);
        image[root..root + ENTRY_SIZE].copy_from_slice(&entry);
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fat12::Fat12;
    use crate::st::StBackend;

    fn read_back(image: &[u8], path: &str) -> Result<Vec<u8>, FloppyError> {
        let mut st = StBackend::from_image(image.to_vec()).unwrap();
        Fat12::mount(&mut st)?.read_file_at_path(path)
    }

    fn walked(image: &[u8]) -> Vec<String> {
        let mut st = StBackend::from_image(image.to_vec()).unwrap();
        Fat12::mount(&mut st)
            .unwrap()
            .walk("", 64)
            .unwrap()
            .into_iter()
            .map(|(p, _)| p)
            .collect()
    }

    #[test]
    fn formats_writes_and_reads_back() {
        let mut image = format_empty_st_disk("TEST", 2).unwrap();
        assert_eq!(image.len(), 1440 * SECTOR_SIZE);
        let mut fs = Fat12Writer::mount(&mut image).unwrap();
        let free = fs.free_clusters();
        assert_eq!(free, (1440 - 14) / 2);

        let big: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        fs.write_file("big.bin", &big).unwrap();
        fs.write_file("EMPTY", b"").unwrap();
        fs.create_dir("auto").unwrap();
        fs.write_file("AUTO/boot.prg", b"boot").unwrap();
        assert_eq!(fs.free_clusters(), free - 5 - 1 - 1);
        // Replacing a file frees its old clusters first.
        fs.write_file("BIG.BIN", b"small").unwrap();
        assert_eq!(fs.free_clusters(), free - 1 - 1 - 1);
        assert!(fs.write_file("auto", b"x").is_err());
        assert!(fs.write_file("toolongname.txt", b"x").is_err());
        assert!(fs.write_file("a:b", b"x").is_err());
        assert!(fs.create_dir("auto").is_err());
        assert!(fs.delete_dir("auto").is_err());
        assert!(fs.write_file("nodir/x", b"x").is_err());

        assert_eq!(read_back(&image, "big.bin").unwrap(), b"small");
        assert_eq!(read_back(&image, "empty").unwrap(), b"");
        assert_eq!(read_back(&image, "auto/boot.prg").unwrap(), b"boot");
        assert_eq!(
            walked(&image),
            ["BIG.BIN", "EMPTY", "AUTO", "AUTO/BOOT.PRG"]
        );

        let mut fs = Fat12Writer::mount(&mut image).unwrap();
        fs.rename("big.bin", "renamed.bin").unwrap();
        fs.rename("empty", "auto/moved").unwrap();
        fs.delete_file("auto/boot.prg").unwrap();
        fs.delete_file("auto/moved").unwrap();
        fs.delete_dir("auto").unwrap();
        assert!(fs.delete_file("auto/boot.prg").is_err());
        assert_eq!(fs.free_clusters(), free - 1);
        assert_eq!(walked(&image), ["RENAMED.BIN"]);
        assert_eq!(read_back(&image, "renamed.bin").unwrap(), b"small");
        // Both FAT copies agree.
        let fat = |n: usize| image[(1 + n * 3) * SECTOR_SIZE..(4 + n * 3) * SECTOR_SIZE].to_vec();
        assert_eq!(fat(0), fat(1));
    }

    #[test]
    fn a_full_subdirectory_grows_and_a_full_root_does_not() {
        let mut image = format_empty_st_disk("", 1).unwrap();
        let mut fs = Fat12Writer::mount(&mut image).unwrap();
        fs.create_dir("d").unwrap();
        // One 1024-byte cluster holds 32 entries, two of them `.`/`..`.
        for i in 0..40 {
            fs.write_file(&format!("d/f{i}"), b"").unwrap();
        }
        assert_eq!(fs.list_dir("d").unwrap().len(), 40);
        for i in 0..111 {
            fs.write_file(&format!("r{i}"), b"").unwrap();
        }
        assert!(fs.write_file("r111", b"").unwrap_err().0.contains("full"));
        assert_eq!(walked(&image).len(), 40 + 1 + 111);
    }

    #[test]
    fn disk_full_is_an_error_and_leaves_the_fat_consistent() {
        let mut image = format_empty_st_disk("", 1).unwrap();
        let mut fs = Fat12Writer::mount(&mut image).unwrap();
        let free = fs.free_clusters();
        let err = fs
            .write_file("huge", &vec![1; (free as usize + 1) * 1024])
            .unwrap_err();
        assert!(err.0.contains("disk full"), "{}", err);
        assert_eq!(fs.free_clusters(), free);
    }

    #[test]
    fn a_looping_chain_is_refused() {
        let mut image = format_empty_st_disk("", 2).unwrap();
        let mut fs = Fat12Writer::mount(&mut image).unwrap();
        fs.write_file("f", &[1; 3000]).unwrap();
        fs.set_fat_entry(3, 2).unwrap();
        assert!(fs.delete_file("f").unwrap_err().0.contains("loops"));
        assert!(fs.write_file("f", b"x").unwrap_err().0.contains("loops"));
    }

    #[test]
    fn a_short_image_is_refused() {
        let mut image = format_empty_st_disk("", 2).unwrap();
        image.truncate(1000 * SECTOR_SIZE);
        assert!(Fat12Writer::mount(&mut image).is_err());
    }
}
