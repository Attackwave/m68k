//! Floppy disk management commands for ADF images.

use serde::{Deserialize, Serialize};

use m68k_floppy::adf_writer::{format_empty_ffs_disk, format_empty_ofs_disk};
use m68k_floppy::amigados::{AmigaFs, DirEntry};
use m68k_floppy::amigados_write::AmigaFsWriter;
use m68k_floppy::floppy_base::{FloppyError, FloppyImageReader};

struct InMemoryAdf<'a>(&'a [u8]);

impl FloppyImageReader for InMemoryAdf<'_> {
    fn read_sector(&mut self, track: u32, side: u32, sector: u32) -> Result<Vec<u8>, FloppyError> {
        let block = (track * 2 + side) * 11 + sector;
        let offset = (block as usize) * 512;
        if offset + 512 <= self.0.len() {
            Ok(self.0[offset..offset + 512].to_vec())
        } else {
            Err(FloppyError::new("Sector out of bounds"))
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateAdfRequest {
    pub disk_name: String,
    pub is_ffs: bool,
    pub boot_code: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdfEntryItem {
    pub name: String,
    pub is_dir: bool,
    pub size: u32,
    pub block: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdfInfoResponse {
    pub volume_name: String,
    pub is_ffs: bool,
    pub free_blocks: u32,
    pub total_blocks: u32,
    pub entries: Vec<AdfEntryItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WriteFileToAdfRequest {
    pub adf_bytes: Vec<u8>,
    pub file_name: String,
    pub file_data: Vec<u8>,
}

pub fn create_new_adf(req: CreateAdfRequest) -> Result<Vec<u8>, String> {
    let mut image = if req.is_ffs {
        format_empty_ffs_disk(&req.disk_name).map_err(|e| e.to_string())?
    } else {
        format_empty_ofs_disk(&req.disk_name).map_err(|e| e.to_string())?
    };

    if let Some(boot) = req.boot_code
        && boot.len() <= 1024
    {
        let mut bootblock = [0u8; 1024];
        bootblock[..boot.len()].copy_from_slice(&boot);
        // Ensure DOS header
        if &bootblock[0..3] != b"DOS" {
            bootblock[0] = b'D';
            bootblock[1] = b'O';
            bootblock[2] = b'S';
            bootblock[3] = if req.is_ffs { 1 } else { 0 };
        }
        let chk = m68k_floppy::adf_writer::compute_bootblock_checksum(&bootblock);
        bootblock[4..8].copy_from_slice(&chk.to_be_bytes());
        image[0..1024].copy_from_slice(&bootblock);
    }

    Ok(image)
}

pub fn write_file_to_adf(req: WriteFileToAdfRequest) -> Result<Vec<u8>, String> {
    let mut image = req.adf_bytes;
    {
        let mut writer = AmigaFsWriter::mount(&mut image).map_err(|e| e.to_string())?;
        writer
            .write_file(&req.file_name, &req.file_data)
            .map_err(|e| e.to_string())?;
    }
    Ok(image)
}

pub fn inspect_adf(adf_bytes: Vec<u8>) -> Result<AdfInfoResponse, String> {
    let mut image = adf_bytes;
    let writer = AmigaFsWriter::mount(&mut image).map_err(|e| e.to_string())?;

    let is_ffs = writer.is_ffs();
    let free_blocks = writer.free_blocks();
    let total_blocks = (image.len() / 512) as u32;

    // Use in-memory reader to list root directory entries
    let mut reader = InMemoryAdf(&image);
    let mut fs = AmigaFs::mount(&mut reader, 80).map_err(|e| e.to_string())?;
    let dir_list: Vec<DirEntry> = fs
        .list_dir(fs.root_block_num())
        .map_err(|e| e.to_string())?;

    let entries = dir_list
        .into_iter()
        .map(|e| AdfEntryItem {
            name: e.name,
            is_dir: e.is_dir,
            size: e.size,
            block: e.block,
        })
        .collect();

    Ok(AdfInfoResponse {
        volume_name: "AmigaDisk".to_string(),
        is_ffs,
        free_blocks,
        total_blocks,
        entries,
    })
}
