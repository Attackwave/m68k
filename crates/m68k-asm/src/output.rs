//! Output format generators: Binary, S-Record, Intel Hex, ELF32.
//!
//! Takes assembled instructions and produces various output formats
//! suitable for loading into emulators, EPROM programmers, etc.

use crate::assembler::AssembledInstruction;
use crate::directives::{Section, SectionKind, SectionManager};

// ---------------------------------------------------------------------------
// Binary output
// ---------------------------------------------------------------------------

/// Generate raw binary bytes from assembled instructions.
///
/// Returns a tuple of `(bytes, base_address)` where `base_address` is the
/// starting address of the first instruction.
pub fn generate_binary(instructions: &[AssembledInstruction]) -> Option<(Vec<u8>, u32)> {
    if instructions.is_empty() {
        return None;
    }

    let base_addr = instructions[0].pc;
    let last_addr = instructions
        .iter()
        .map(|i| i.pc + i.size_bytes() as u32)
        .max()
        .unwrap_or(base_addr);

    let total_size = (last_addr - base_addr) as usize;
    let mut bytes = vec![0u8; total_size];

    for instr in instructions {
        let offset = (instr.pc - base_addr) as usize;
        let mut pos = offset;
        for word in &instr.words {
            bytes[pos] = (word >> 8) as u8;
            bytes[pos + 1] = (word & 0xFF) as u8;
            pos += 2;
        }
    }

    Some((bytes, base_addr))
}

/// Generate raw binary bytes, filling gaps with a specified padding byte.
pub fn generate_binary_with_padding(
    instructions: &[AssembledInstruction],
    pad_byte: u8,
) -> Option<(Vec<u8>, u32)> {
    if instructions.is_empty() {
        return None;
    }

    let base_addr = instructions[0].pc;
    let last_addr = instructions
        .iter()
        .map(|i| i.pc + i.size_bytes() as u32)
        .max()
        .unwrap_or(base_addr);

    let total_size = (last_addr - base_addr) as usize;
    let mut bytes = vec![pad_byte; total_size];

    for instr in instructions {
        let offset = (instr.pc - base_addr) as usize;
        let mut pos = offset;
        for word in &instr.words {
            bytes[pos] = (word >> 8) as u8;
            bytes[pos + 1] = (word & 0xFF) as u8;
            pos += 2;
        }
    }

    Some((bytes, base_addr))
}

// ---------------------------------------------------------------------------
// S-Record (Motorola S-Record / SREC)
// ---------------------------------------------------------------------------

/// S-Record output generator.
pub struct SRecordWriter {
    records: Vec<String>,
    max_addr: u32,
}

impl Default for SRecordWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SRecordWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.records.join("\n"))
    }
}

impl SRecordWriter {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            max_addr: 0,
        }
    }

    /// Add an S0 header record with module name.
    pub fn header(&mut self, name: &str) {
        let name_bytes = name.as_bytes();
        let addr: u16 = 0x0000;
        let mut data = Vec::new();
        data.push((addr >> 8) as u8);
        data.push((addr & 0xFF) as u8);
        data.extend_from_slice(name_bytes);

        let byte_count = (data.len() + 1) as u8; // +1 for checksum
        let checksum = compute_checksum_srec(byte_count, 0, &data);

        let mut record = format!("S0{:02X}{:04X}", byte_count, addr);
        for b in &data {
            record.push_str(&format!("{:02X}", b));
        }
        record.push_str(&format!("{:02X}", checksum));

        self.records.push(record);
    }

    /// Add data records from assembled instructions.
    /// Uses S1 (16-bit addresses) if all addresses fit in 16 bits,
    /// otherwise S2 (24-bit) or S3 (32-bit).
    pub fn add_instructions(&mut self, instructions: &[AssembledInstruction]) {
        if instructions.is_empty() {
            return;
        }

        // Determine max address to choose record type
        let max_addr = instructions
            .iter()
            .map(|i| i.pc + i.size_bytes() as u32 - 1)
            .max()
            .unwrap_or(0);

        self.max_addr = max_addr;

        let record_type = if max_addr <= 0xFFFF {
            SRecType::S1
        } else if max_addr <= 0xFFFFFF {
            SRecType::S2
        } else {
            SRecType::S3
        };

        // Build linear byte array
        let base_addr = instructions[0].pc;
        let end_addr = max_addr + 1;
        let mut mem = vec![0u8; (end_addr - base_addr) as usize];

        for instr in instructions {
            let offset = (instr.pc - base_addr) as usize;
            let mut pos = offset;
            for word in &instr.words {
                mem[pos] = (word >> 8) as u8;
                mem[pos + 1] = (word & 0xFF) as u8;
                pos += 2;
            }
        }

        // Split into records of max 32 bytes of data per record
        const MAX_DATA_PER_RECORD: usize = 32;
        let mut offset: usize = 0;
        while offset < mem.len() {
            let chunk_size = MAX_DATA_PER_RECORD.min(mem.len() - offset);
            let chunk = &mem[offset..offset + chunk_size];
            let addr = base_addr + offset as u32;

            let record = encode_srec(record_type, addr, chunk);
            self.records.push(record);
            offset += chunk_size;
        }
    }

    /// Add S9 termination record with start address.
    pub fn termination(&mut self, start_addr: u32) {
        let record = if self.max_addr <= 0xFFFF {
            let addr = (start_addr & 0xFFFF) as u16;
            let byte_count = 3u8;
            let checksum =
                (0x100 - ((byte_count as u16 + (addr >> 8) + (addr & 0xFF)) & 0xFF)) as u8;
            format!("S9{:02X}{:04X}{:02X}", byte_count, addr, checksum)
        } else if self.max_addr <= 0xFFFFFF {
            let b0 = ((start_addr >> 16) & 0xFF) as u8;
            let b1 = ((start_addr >> 8) & 0xFF) as u8;
            let b2 = (start_addr & 0xFF) as u8;
            let byte_count = 4u8;
            let checksum =
                (0x100 - ((byte_count as u16 + b0 as u16 + b1 as u16 + b2 as u16) & 0xFF)) as u8;
            format!(
                "S9{:02X}{:02X}{:02X}{:02X}{:02X}",
                byte_count, b0, b1, b2, checksum
            )
        } else {
            let b0 = ((start_addr >> 24) & 0xFF) as u8;
            let b1 = ((start_addr >> 16) & 0xFF) as u8;
            let b2 = ((start_addr >> 8) & 0xFF) as u8;
            let b3 = (start_addr & 0xFF) as u8;
            let byte_count = 5u8;
            let checksum = (0x100
                - ((byte_count as u16 + b0 as u16 + b1 as u16 + b2 as u16 + b3 as u16) & 0xFF))
                as u8;
            format!(
                "S9{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                byte_count, b0, b1, b2, b3, checksum
            )
        };
        self.records.push(record);
    }

    /// Get the records as a vector of strings.
    pub fn records(&self) -> &[String] {
        &self.records
    }
}

#[derive(Debug, Clone, Copy)]
enum SRecType {
    S1, // 16-bit address
    S2, // 24-bit address
    S3, // 32-bit address
}

fn encode_srec(rec_type: SRecType, address: u32, data: &[u8]) -> String {
    let addr_bytes: [u8; 4] = match rec_type {
        SRecType::S1 => [(address >> 8) as u8, (address & 0xFF) as u8, 0, 0],
        SRecType::S2 => [
            ((address >> 16) & 0xFF) as u8,
            ((address >> 8) & 0xFF) as u8,
            (address & 0xFF) as u8,
            0,
        ],
        SRecType::S3 => [
            ((address >> 24) & 0xFF) as u8,
            ((address >> 16) & 0xFF) as u8,
            ((address >> 8) & 0xFF) as u8,
            (address & 0xFF) as u8,
        ],
    };

    let addr_len = match rec_type {
        SRecType::S1 => 2,
        SRecType::S2 => 3,
        SRecType::S3 => 4,
    };
    let byte_count = (addr_len + data.len() + 1) as u8;

    let mut record = match rec_type {
        SRecType::S1 => format!("S1{:02X}{:04X}", byte_count, address),
        SRecType::S2 => format!(
            "S2{:02X}{:02X}{:02X}{:02X}",
            byte_count, addr_bytes[0], addr_bytes[1], addr_bytes[2]
        ),
        SRecType::S3 => format!(
            "S3{:02X}{:02X}{:02X}{:02X}{:02X}",
            byte_count, addr_bytes[0], addr_bytes[1], addr_bytes[2], addr_bytes[3]
        ),
    };

    for b in data {
        record.push_str(&format!("{:02X}", b));
    }

    let checksum = compute_srec_checksum_from_str(&record[2..]);
    record.push_str(&format!("{:02X}", checksum));

    record
}

fn compute_checksum_srec(byte_count: u8, _record_type: u8, data: &[u8]) -> u8 {
    let mut sum = byte_count as u16;
    for b in data {
        sum += *b as u16;
    }
    (!sum & 0xFF) as u8
}

fn compute_srec_checksum_from_str(hex_str: &str) -> u8 {
    let mut sum: u16 = 0;
    let bytes = hex_str.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let hex_byte = std::str::from_utf8(&bytes[i..i + 2]).unwrap_or("00");
        sum += u8::from_str_radix(hex_byte, 16).unwrap_or(0) as u16;
        i += 2;
    }
    (!sum & 0xFF) as u8
}

/// Generate S-Record output from assembled instructions.
pub fn generate_srecord(instructions: &[AssembledInstruction], name: &str) -> String {
    let mut writer = SRecordWriter::new();
    writer.header(name);
    writer.add_instructions(instructions);
    if let Some(instr) = instructions.first() {
        writer.termination(instr.pc);
    } else {
        writer.termination(0);
    }
    writer.to_string()
}

// ---------------------------------------------------------------------------
// Intel Hex
// ---------------------------------------------------------------------------

/// Intel Hex output generator.
pub struct IntelHexWriter {
    records: Vec<String>,
    current_extended_addr: u32,
}

impl Default for IntelHexWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for IntelHexWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.records.join("\n"))
    }
}

impl IntelHexWriter {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            current_extended_addr: 0xFFFF_FFFF, // Force first extended addr record
        }
    }

    /// Add data records from assembled instructions.
    pub fn add_instructions(&mut self, instructions: &[AssembledInstruction]) {
        if instructions.is_empty() {
            return;
        }

        // Build linear byte array
        let base_addr = instructions[0].pc;
        let last_addr = instructions
            .iter()
            .map(|i| i.pc + i.size_bytes() as u32)
            .max()
            .unwrap_or(base_addr);

        let total_size = (last_addr - base_addr) as usize;
        let mut mem = vec![0u8; total_size];

        for instr in instructions {
            let offset = (instr.pc - base_addr) as usize;
            let mut pos = offset;
            for word in &instr.words {
                mem[pos] = (word >> 8) as u8;
                mem[pos + 1] = (word & 0xFF) as u8;
                pos += 2;
            }
        }

        // Split into 16-byte chunks (standard Intel Hex record size)
        const MAX_DATA_PER_RECORD: usize = 16;
        let mut offset: usize = 0;
        while offset < mem.len() {
            let chunk_size = MAX_DATA_PER_RECORD.min(mem.len() - offset);
            let addr = base_addr + offset as u32;

            // Check if we need an extended address record
            let extended_addr = (addr >> 16) & 0xFFFF;
            if extended_addr != self.current_extended_addr {
                self.current_extended_addr = extended_addr;
                let record = format!(
                    ":02000004{:04X}{:02X}",
                    extended_addr,
                    compute_hex_checksum(0x02, 0x0000, 0x04, &[])
                );
                self.records.push(record);
            }

            let chunk = &mem[offset..offset + chunk_size];
            let record_addr = (addr & 0xFFFF) as u16;
            let record = encode_hex_data(record_addr, chunk);
            self.records.push(record);

            offset += chunk_size;
        }
    }

    /// Add end-of-file record.
    pub fn end_of_file(&mut self) {
        self.records.push(":00000001FF".to_string());
    }

    /// Get the records as a vector of strings.
    pub fn records(&self) -> &[String] {
        &self.records
    }
}

fn encode_hex_data(address: u16, data: &[u8]) -> String {
    let byte_count = data.len() as u8;
    let record_type: u8 = 0x00; // Data record
    let mut record = format!(":{:02X}{:04X}{:02X}", byte_count, address, record_type);

    for b in data {
        record.push_str(&format!("{:02X}", b));
    }

    let checksum = compute_hex_checksum(byte_count, address, record_type, data);
    record.push_str(&format!("{:02X}", checksum));

    record
}

fn compute_hex_checksum(byte_count: u8, address: u16, record_type: u8, data: &[u8]) -> u8 {
    let mut sum: u32 = byte_count as u32
        + ((address >> 8) as u32)
        + ((address & 0xFF) as u32)
        + record_type as u32;

    for b in data {
        sum += *b as u32;
    }

    ((!sum + 1) & 0xFF) as u8
}

/// Generate Intel Hex output from assembled instructions.
pub fn generate_intel_hex(instructions: &[AssembledInstruction]) -> String {
    let mut writer = IntelHexWriter::new();
    writer.add_instructions(instructions);
    writer.end_of_file();
    writer.to_string()
}

// ---------------------------------------------------------------------------
// Output format enum
// ---------------------------------------------------------------------------

/// Supported output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Raw binary.
    Binary,
    /// Motorola S-Record.
    SRecord,
    /// Intel Hex.
    IntelHex,
    /// ELF32 relocatable object (EM_68K). Binary, not representable by
    /// `generate_output`'s text-based interface — use `generate_elf` directly.
    Elf,
    /// IEEE-695 object module. Binary, not representable by
    /// `generate_output`'s text-based interface — use
    /// `crate::ieee695::generate_ieee695` directly.
    Ieee695,
}

/// Generate output in the specified format.
///
/// Not valid for `OutputFormat::Elf` or `OutputFormat::Ieee695`, which are
/// binary; call `generate_elf` / `crate::ieee695::generate_ieee695` directly
/// for those formats.
pub fn generate_output(
    instructions: &[AssembledInstruction],
    format: OutputFormat,
    name: &str,
) -> String {
    match format {
        OutputFormat::Binary => {
            if let Some((bytes, _base)) = generate_binary(instructions) {
                bytes
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                String::new()
            }
        }
        OutputFormat::SRecord => generate_srecord(instructions, name),
        OutputFormat::IntelHex => generate_intel_hex(instructions),
        OutputFormat::Elf => {
            panic!("OutputFormat::Elf is binary; call generate_elf directly")
        }
        OutputFormat::Ieee695 => {
            panic!("OutputFormat::Ieee695 is binary; call ieee695::generate_ieee695 directly")
        }
    }
}

// ---------------------------------------------------------------------------
// ELF32 object file (big-endian, EM_68K)
// ---------------------------------------------------------------------------
//
// Emits a relocatable ELF32 object (ET_REL). `generate_elf_sections` splits
// output into one ELF section per non-empty assembler SECTION (TEXT/DATA/BSS/
// named), each keeping its own `sh_addr`/flags; `generate_elf` is a
// single-`.text`-section convenience wrapper kept for the common case where
// no SECTION directives are used.
//
// Limitations:
// - The assembler resolves all symbol references to absolute values during
//   assembly and does not track relocation entries, so this writer emits no
//   `.rela.*` sections — all instruction bytes are already fully resolved.
//   The object is only useful as a final, already-linked image wrapped in
//   ELF framing, not as linker input for undefined external symbols.

const ET_REL: u16 = 1;
const EM_68K: u16 = 4;
const SHT_NULL: u32 = 0;
const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_NOBITS: u32 = 8;
const SHF_WRITE: u32 = 1;
const SHF_ALLOC: u32 = 2;
const SHF_EXECINSTR: u32 = 4;
const STB_GLOBAL: u8 = 1 << 4;
const STT_NOTYPE: u8 = 0;
const SHN_ABS: u16 = 0xFFF1;

struct StringTable {
    bytes: Vec<u8>,
}

impl StringTable {
    fn new() -> Self {
        // Index 0 is always the empty string.
        Self { bytes: vec![0] }
    }

    fn add(&mut self, s: &str) -> u32 {
        let offset = self.bytes.len() as u32;
        self.bytes.extend_from_slice(s.as_bytes());
        self.bytes.push(0);
        offset
    }
}

struct Elf32SectionHeader {
    name_offset: u32,
    sh_type: u32,
    flags: u32,
    addr: u32,
    offset: u32,
    size: u32,
    link: u32,
    info: u32,
    align: u32,
    entsize: u32,
}

impl Elf32SectionHeader {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.name_offset.to_be_bytes());
        out.extend_from_slice(&self.sh_type.to_be_bytes());
        out.extend_from_slice(&self.flags.to_be_bytes());
        out.extend_from_slice(&self.addr.to_be_bytes());
        out.extend_from_slice(&self.offset.to_be_bytes());
        out.extend_from_slice(&self.size.to_be_bytes());
        out.extend_from_slice(&self.link.to_be_bytes());
        out.extend_from_slice(&self.info.to_be_bytes());
        out.extend_from_slice(&self.align.to_be_bytes());
        out.extend_from_slice(&self.entsize.to_be_bytes());
    }
}

/// Generate an ELF32 big-endian EM_68K relocatable object file with a single
/// `.text` section spanning `instructions`' address range, plus a symbol
/// table built from the assembler's `SymbolTable`.
///
/// Returns an empty `Vec` if `instructions` is empty. Convenience wrapper
/// around [`generate_elf_sections`] for callers without a [`SectionManager`]
/// (or that never used SECTION directives — the common case).
pub fn generate_elf(
    instructions: &[AssembledInstruction],
    symbols: &crate::assembler::SymbolTable,
) -> Vec<u8> {
    let Some((text_bytes, text_addr)) = generate_binary(instructions) else {
        return Vec::new();
    };
    generate_elf_from_parts(
        &[(
            ".text".to_string(),
            "text".to_string(),
            SHF_ALLOC | SHF_EXECINSTR,
            text_addr,
            text_bytes,
        )],
        symbols,
    )
}

/// Generate an ELF32 big-endian EM_68K relocatable object file with one ELF
/// section per non-empty assembler SECTION (TEXT/DATA/BSS/named), each
/// keeping its own address and flags. BSS sections are emitted as
/// `SHT_NOBITS` (no file-image bytes, matching their uninitialized nature).
///
/// See module docs for the no-relocation-entries limitation shared with
/// [`generate_elf`].
pub fn generate_elf_sections(
    sections: &SectionManager,
    symbols: &crate::assembler::SymbolTable,
) -> Vec<u8> {
    let mut ordered: Vec<(&SectionKind, &Section)> = sections
        .iter_sections()
        .filter(|(_, s)| !s.instructions.is_empty())
        .collect();
    ordered.sort_by(|a, b| {
        a.1.base_addr()
            .cmp(&b.1.base_addr())
            .then(a.0.name().cmp(b.0.name()))
    });

    let parts: Vec<(String, String, u32, u32, Vec<u8>)> = ordered
        .iter()
        .map(|(kind, section)| {
            let flags = match kind {
                SectionKind::Text => SHF_ALLOC | SHF_EXECINSTR,
                SectionKind::Bss | SectionKind::Data | SectionKind::Named(_) => {
                    SHF_ALLOC | SHF_WRITE
                }
            };
            // ELF section-name convention: leading dot (`.text`/`.data`/`.bss`),
            // distinct from `SectionKind::name()`'s bare form used for SECTION
            // directive parsing and for `SymbolEntry::section` matching.
            (
                format!(".{}", kind.name()),
                kind.name().to_string(),
                flags,
                section.base_addr(),
                section.to_bytes(),
            )
        })
        .collect();

    generate_elf_from_parts(&parts, symbols)
}

/// Look up which section (by address range) a symbol value falls into, for
/// `st_shndx`. Returns `None` if the value doesn't fall in any known range
/// (e.g. an `EQU` constant unrelated to code/data placement) — such symbols
/// are emitted as `SHN_ABS`.
///
/// This is a fallback for symbols with no recorded `SymbolEntry::section`
/// (e.g. `EQU`/`SET` constants, which the assembler doesn't associate with a
/// section). For symbols defined by a label, `generate_elf_from_parts`
/// resolves the section by name instead, which is exact and doesn't suffer
/// from the ambiguity this heuristic has when sections share overlapping
/// address ranges (most commonly when no `ORG` is used and every section
/// defaults to the same base address).
fn section_index_for_value(bounds: &[(u32, u32)], value: u32) -> Option<u16> {
    bounds
        .iter()
        .position(|(start, end)| value >= *start && value < *end)
        .map(|i| (i + 1) as u16) // +1: section 0 is the mandatory SHT_NULL entry
}

fn generate_elf_from_parts(
    parts: &[(String, String, u32, u32, Vec<u8>)],
    symbols: &crate::assembler::SymbolTable,
) -> Vec<u8> {
    if parts.is_empty() {
        return Vec::new();
    }

    let mut shstrtab = StringTable::new();
    let mut strtab = StringTable::new();

    // Section 0 is the mandatory SHT_NULL entry; one entry per part follows,
    // then .symtab, .strtab, .shstrtab.
    let mut section_headers: Vec<Elf32SectionHeader> = Vec::new();
    let mut section_data: Vec<Vec<u8>> = Vec::new();
    let mut is_nobits: Vec<bool> = Vec::new();

    section_headers.push(Elf32SectionHeader {
        name_offset: 0,
        sh_type: SHT_NULL,
        flags: 0,
        addr: 0,
        offset: 0,
        size: 0,
        link: 0,
        info: 0,
        align: 0,
        entsize: 0,
    });
    section_data.push(Vec::new());
    is_nobits.push(false);

    let mut bounds: Vec<(u32, u32)> = Vec::new();
    let mut index_by_section_name: std::collections::HashMap<&str, u16> =
        std::collections::HashMap::new();
    for (i, (name, bare_name, flags, addr, data)) in parts.iter().enumerate() {
        let name_offset = shstrtab.add(name);
        let nobits = *name == ".bss";
        section_headers.push(Elf32SectionHeader {
            name_offset,
            sh_type: if nobits { SHT_NOBITS } else { SHT_PROGBITS },
            flags: *flags,
            addr: *addr,
            offset: 0, // patched below
            size: data.len() as u32,
            link: 0,
            info: 0,
            align: 2,
            entsize: 0,
        });
        section_data.push(if nobits { Vec::new() } else { data.clone() });
        is_nobits.push(nobits);
        bounds.push((*addr, addr + data.len() as u32));
        index_by_section_name.insert(bare_name.as_str(), (i + 1) as u16); // +1: SHT_NULL is index 0
    }

    // Symbol table: one null entry, then one entry per defined symbol.
    let mut symtab_bytes = Vec::new();
    // Null symbol.
    symtab_bytes.extend_from_slice(&0u32.to_be_bytes()); // st_name
    symtab_bytes.extend_from_slice(&0u32.to_be_bytes()); // st_value
    symtab_bytes.extend_from_slice(&0u32.to_be_bytes()); // st_size
    symtab_bytes.push(0); // st_info
    symtab_bytes.push(0); // st_other
    symtab_bytes.extend_from_slice(&0u16.to_be_bytes()); // st_shndx

    let mut defined_symbols: Vec<_> = symbols.iter().filter(|(_, e)| e.defined).collect();
    defined_symbols.sort_by(|a, b| a.0.cmp(b.0));

    for (name, entry) in defined_symbols {
        let name_offset = strtab.add(name);
        let shndx = entry
            .section
            .as_deref()
            .and_then(|s| index_by_section_name.get(s).copied())
            .or_else(|| section_index_for_value(&bounds, entry.value))
            .unwrap_or(SHN_ABS);
        symtab_bytes.extend_from_slice(&name_offset.to_be_bytes());
        symtab_bytes.extend_from_slice(&entry.value.to_be_bytes());
        symtab_bytes.extend_from_slice(&0u32.to_be_bytes()); // st_size
        symtab_bytes.push(STB_GLOBAL | STT_NOTYPE);
        symtab_bytes.push(0);
        symtab_bytes.extend_from_slice(&shndx.to_be_bytes());
    }

    let symtab_name_offset = shstrtab.add(".symtab");
    let strtab_section_index = section_headers.len() as u32 + 1;
    section_headers.push(Elf32SectionHeader {
        name_offset: symtab_name_offset,
        sh_type: SHT_SYMTAB,
        flags: 0,
        addr: 0,
        offset: 0,
        size: symtab_bytes.len() as u32,
        link: strtab_section_index,
        info: 1, // one local (null) symbol
        align: 4,
        entsize: 16,
    });
    section_data.push(symtab_bytes);
    is_nobits.push(false);

    let strtab_name_offset = shstrtab.add(".strtab");
    section_headers.push(Elf32SectionHeader {
        name_offset: strtab_name_offset,
        sh_type: SHT_STRTAB,
        flags: 0,
        addr: 0,
        offset: 0,
        size: strtab.bytes.len() as u32,
        link: 0,
        info: 0,
        align: 1,
        entsize: 0,
    });
    section_data.push(strtab.bytes.clone());
    is_nobits.push(false);

    let shstrtab_name_offset = shstrtab.add(".shstrtab");
    let shstrtab_index = section_headers.len();
    section_headers.push(Elf32SectionHeader {
        name_offset: shstrtab_name_offset,
        sh_type: SHT_STRTAB,
        flags: 0,
        addr: 0,
        offset: 0,
        size: 0, // patched below (depends on itself being added last)
        link: 0,
        info: 0,
        align: 1,
        entsize: 0,
    });
    section_data.push(Vec::new()); // patched below
    is_nobits.push(false);

    // shstrtab includes its own name, so finalize its bytes now.
    let shstrtab_bytes = shstrtab.bytes.clone();
    section_headers[shstrtab_index].size = shstrtab_bytes.len() as u32;
    section_data[shstrtab_index] = shstrtab_bytes;

    // Layout: ELF header, then section contents (in order, skipping
    // SHT_NOBITS sections which occupy no file space), then section header
    // table. Compute file offsets for each section's data.
    const EHDR_SIZE: u32 = 52;
    const SHDR_SIZE: u32 = 40;

    let mut offset = EHDR_SIZE;
    for ((hdr, data), nobits) in section_headers
        .iter_mut()
        .zip(section_data.iter())
        .zip(is_nobits.iter())
    {
        if hdr.sh_type == SHT_NULL {
            continue;
        }
        hdr.offset = offset;
        if !nobits {
            offset += data.len() as u32;
        }
    }
    let shoff = offset;
    let shnum = section_headers.len() as u16;

    let mut out = Vec::new();
    // e_ident
    out.extend_from_slice(&[0x7F, b'E', b'L', b'F']);
    out.push(1); // EI_CLASS = ELFCLASS32
    out.push(2); // EI_DATA = ELFDATA2MSB (big-endian)
    out.push(1); // EI_VERSION
    out.push(0); // EI_OSABI
    out.extend_from_slice(&[0u8; 8]); // EI_PAD
    out.extend_from_slice(&ET_REL.to_be_bytes()); // e_type
    out.extend_from_slice(&EM_68K.to_be_bytes()); // e_machine
    out.extend_from_slice(&1u32.to_be_bytes()); // e_version
    out.extend_from_slice(&0u32.to_be_bytes()); // e_entry
    out.extend_from_slice(&0u32.to_be_bytes()); // e_phoff
    out.extend_from_slice(&shoff.to_be_bytes()); // e_shoff
    out.extend_from_slice(&0u32.to_be_bytes()); // e_flags
    out.extend_from_slice(&(EHDR_SIZE as u16).to_be_bytes()); // e_ehsize
    out.extend_from_slice(&0u16.to_be_bytes()); // e_phentsize
    out.extend_from_slice(&0u16.to_be_bytes()); // e_phnum
    out.extend_from_slice(&(SHDR_SIZE as u16).to_be_bytes()); // e_shentsize
    out.extend_from_slice(&shnum.to_be_bytes()); // e_shnum
    out.extend_from_slice(&(shstrtab_index as u16).to_be_bytes()); // e_shstrndx

    debug_assert_eq!(out.len() as u32, EHDR_SIZE);

    for (data, nobits) in section_data.iter().zip(is_nobits.iter()) {
        if !nobits {
            out.extend_from_slice(data);
        }
    }
    for hdr in &section_headers {
        hdr.write(&mut out);
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_instr(pc: u32, words: Vec<u16>) -> AssembledInstruction {
        AssembledInstruction {
            pc,
            words,
            line_no: None,
            source: None,
        }
    }

    #[test]
    fn test_binary_output() {
        let instructions = vec![
            make_instr(0x1000, vec![0x4E71]), // NOP at $1000
            make_instr(0x1002, vec![0x4E75]), // RTS at $1002
        ];

        let (bytes, base) = generate_binary(&instructions).unwrap();
        assert_eq!(base, 0x1000);
        assert_eq!(bytes, vec![0x4E, 0x71, 0x4E, 0x75]);
    }

    #[test]
    fn test_binary_output_with_gap() {
        let instructions = vec![
            make_instr(0x1000, vec![0x4E71]), // NOP at $1000
            make_instr(0x1010, vec![0x4E75]), // RTS at $1010
        ];

        let (bytes, base) = generate_binary(&instructions).unwrap();
        assert_eq!(base, 0x1000);
        assert_eq!(bytes.len(), 0x12); // $1010 + 2 - $1000 = 0x12
        assert_eq!(bytes[0], 0x4E);
        assert_eq!(bytes[1], 0x71);
        assert_eq!(bytes[0x10], 0x4E);
        assert_eq!(bytes[0x11], 0x75);
        // Gap bytes should be 0
        for b in &bytes[2..0x10] {
            assert_eq!(*b, 0);
        }
    }

    #[test]
    fn test_binary_with_padding() {
        let instructions = vec![make_instr(0x1000, vec![0x4E71])];

        let (bytes, _base) = generate_binary_with_padding(&instructions, 0xFF).unwrap();
        assert_eq!(bytes, vec![0x4E, 0x71]);
    }

    #[test]
    fn test_srecord_basic() {
        let instructions = vec![make_instr(0x1000, vec![0x4E71, 0x4E75])];

        let output = generate_srecord(&instructions, "test");
        assert!(output.contains("S0")); // Header
        assert!(output.contains("S1")); // Data
        assert!(output.contains("S9")); // Termination
    }

    #[test]
    fn test_srecord_checksum() {
        // S1040000FB -> byte_count=4, addr=0x0000, data=[0xFB], checksum should be 0xFF - (0x04+0x00+0x00+0xFB) + 1 = 0x00
        // Actually: checksum = ~(sum) & 0xFF where sum = 0x04 + 0x00 + 0x00 + 0xFB = 0xFF
        // checksum = ~0xFF & 0xFF = 0x00
        let checksum = compute_srec_checksum_from_str("040000FB");
        assert_eq!(checksum, 0x00);
    }

    #[test]
    fn test_intel_hex_basic() {
        let instructions = vec![make_instr(0x1000, vec![0x4E71, 0x4E75])];

        let output = generate_intel_hex(&instructions);
        assert!(output.contains(":04")); // 4 bytes of data
        assert!(output.contains("0001FF")); // End of file record
    }

    #[test]
    fn test_intel_hex_checksum() {
        let checksum = compute_hex_checksum(0x04, 0x1000, 0x00, &[0x4E, 0x71, 0x4E, 0x75]);
        // sum = 0x04 + 0x10 + 0x00 + 0x4E + 0x71 + 0x4E + 0x75 = 0x196
        // checksum = (0x100 - (0x196 & 0xFF)) & 0xFF = 0x6A
        assert_eq!(checksum, 0x6A);
    }

    #[test]
    fn test_intel_hex_extended_address() {
        // Instructions at addresses > 0xFFFF should trigger extended address records
        let instructions = vec![make_instr(0x10000, vec![0x4E71])];

        let output = generate_intel_hex(&instructions);
        assert!(output.contains("04")); // Extended address record type
    }

    #[test]
    fn test_empty_instructions() {
        let instructions: Vec<AssembledInstruction> = vec![];
        assert!(generate_binary(&instructions).is_none());
        assert!(
            generate_srecord(&instructions, "test").is_empty()
                || generate_srecord(&instructions, "test").contains("S0")
        );
    }

    #[test]
    fn test_srecord_24bit_address() {
        let instructions = vec![make_instr(0x123456, vec![0x4E71])];

        let output = generate_srecord(&instructions, "test");
        assert!(output.contains("S2")); // 24-bit data record
    }

    #[test]
    fn test_output_format_enum() {
        let instructions = vec![make_instr(0x1000, vec![0x4E71])];

        let binary = generate_output(&instructions, OutputFormat::Binary, "test");
        assert!(binary.contains("4E"));

        let srec = generate_output(&instructions, OutputFormat::SRecord, "test");
        assert!(srec.contains("S0"));

        let ihex = generate_output(&instructions, OutputFormat::IntelHex, "test");
        assert!(ihex.starts_with(':'));
    }

    #[test]
    fn test_elf_header_fields() {
        let mut asm = crate::assembler::Assembler::new(0x1000);
        asm.assemble("    ORG $1000\nSTART:\n    MOVEQ #1,D0\n    RTS\n")
            .unwrap();

        let elf = generate_elf(&asm.code, &asm.symbols);

        assert_eq!(&elf[0..4], &[0x7F, b'E', b'L', b'F']);
        assert_eq!(elf[4], 1); // ELFCLASS32
        assert_eq!(elf[5], 2); // ELFDATA2MSB
        let e_type = u16::from_be_bytes([elf[16], elf[17]]);
        assert_eq!(e_type, ET_REL);
        let e_machine = u16::from_be_bytes([elf[18], elf[19]]);
        assert_eq!(e_machine, EM_68K);
    }

    #[test]
    fn test_elf_roundtrips_via_readelf() {
        use std::io::Write;
        use std::process::Command;

        let mut asm = crate::assembler::Assembler::new(0x1000);
        asm.assemble("    ORG $1000\nSTART:\n    MOVEQ #1,D0\n    RTS\nCOUNT EQU 5\n")
            .unwrap();

        let elf = generate_elf(&asm.code, &asm.symbols);

        let mut path = std::env::temp_dir();
        path.push(format!("m68k_elf_test_{}.o", std::process::id()));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&elf).unwrap();
        }

        let output = Command::new("readelf").arg("-a").arg(&path).output();
        let _ = std::fs::remove_file(&path);

        match output {
            Ok(out) => {
                assert!(out.status.success(), "readelf failed: {:?}", out);
                let stdout = String::from_utf8_lossy(&out.stdout);
                assert!(stdout.contains("Motorola m68k") || stdout.contains("68000"));
                assert!(stdout.contains("START"));
                assert!(stdout.contains("COUNT"));
            }
            Err(_) => {
                // readelf not installed in this environment; skip external validation.
            }
        }
    }

    #[test]
    fn test_elf_sections_splits_text_and_data() {
        let mut asm = crate::assembler::Assembler::new(0x1000);
        asm.assemble(
            "    SECTION text\nSTART:\n    NOP\n    SECTION data\nCOUNT:\n    DC.W $1234\n",
        )
        .unwrap();

        let elf = generate_elf_sections(&asm.sections, &asm.symbols);
        assert_eq!(&elf[0..4], &[0x7F, b'E', b'L', b'F']);

        // Both section names must appear in the shstrtab-backed section
        // header names; cheapest check without a full ELF parser is to look
        // for the raw bytes in the file.
        let contains = |needle: &[u8]| elf.windows(needle.len()).any(|w| w == needle);
        assert!(contains(b".text"));
        assert!(contains(b".data"));
    }

    #[test]
    fn test_elf_sections_roundtrips_via_readelf() {
        use std::io::Write;
        use std::process::Command;

        let mut asm = crate::assembler::Assembler::new(0x1000);
        asm.assemble(
            "    SECTION text\nSTART:\n    MOVEQ #1,D0\n    RTS\n    SECTION data\nCOUNT:\n    DC.W $1234\n",
        )
        .unwrap();

        let elf = generate_elf_sections(&asm.sections, &asm.symbols);

        let mut path = std::env::temp_dir();
        path.push(format!("m68k_elf_sections_test_{}.o", std::process::id()));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&elf).unwrap();
        }

        let output = Command::new("readelf").arg("-a").arg(&path).output();
        let _ = std::fs::remove_file(&path);

        match output {
            Ok(out) => {
                assert!(out.status.success(), "readelf failed: {:?}", out);
                let stdout = String::from_utf8_lossy(&out.stdout);
                assert!(stdout.contains(".text"));
                assert!(stdout.contains(".data"));
                assert!(stdout.contains("START"));
                assert!(stdout.contains("COUNT"));
            }
            Err(_) => {
                // readelf not installed in this environment; skip external validation.
            }
        }
    }

    #[test]
    fn test_elf_sections_symbol_shndx_without_org() {
        // Regression check: two sections with no explicit ORG both default
        // to the same base address (SectionManager::default_origin), which
        // used to make the address-range heuristic in
        // section_index_for_value ambiguous (both symbols would match the
        // first section's range). SymbolEntry::section now records the
        // defining section directly, so this must resolve correctly even
        // when the address ranges overlap.
        use std::io::Write;
        use std::process::Command;

        let mut asm = crate::assembler::Assembler::new(0);
        asm.assemble(
            "    SECTION text\nSTART:\n    NOP\n    SECTION data\nCOUNT:\n    DC.W $1234\n",
        )
        .unwrap();

        let elf = generate_elf_sections(&asm.sections, &asm.symbols);

        let mut path = std::env::temp_dir();
        path.push(format!("m68k_elf_shndx_test_{}.o", std::process::id()));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&elf).unwrap();
        }

        let syms_output = Command::new("readelf").arg("-sW").arg(&path).output();
        let secs_output = Command::new("readelf").arg("-SW").arg(&path).output();
        let _ = std::fs::remove_file(&path);

        if let (Ok(syms), Ok(secs)) = (syms_output, secs_output) {
            assert!(syms.status.success(), "readelf -sW failed: {:?}", syms);
            assert!(secs.status.success(), "readelf -SW failed: {:?}", secs);
            let syms_out = String::from_utf8_lossy(&syms.stdout);
            let secs_out = String::from_utf8_lossy(&secs.stdout);

            // `readelf -SW` lines look like "  [ 1] .data  PROGBITS ...".
            let name_for_ndx = |ndx: &str| -> String {
                let marker = format!("[{:>2}]", ndx.trim());
                secs_out
                    .lines()
                    .find(|l| l.contains(&marker))
                    .and_then(|l| l.split_whitespace().nth(2))
                    .unwrap_or("")
                    .to_string()
            };
            // `readelf -sW` symbol lines end with "... <Ndx> <Name>".
            let section_for = |symbol: &str| -> String {
                let fields: Vec<&str> = syms_out
                    .lines()
                    .find(|l| l.trim_end().ends_with(symbol))
                    .map(|l| l.split_whitespace().collect())
                    .unwrap_or_default();
                fields
                    .len()
                    .checked_sub(2)
                    .and_then(|i| fields.get(i))
                    .map(|ndx| name_for_ndx(ndx))
                    .unwrap_or_default()
            };
            assert_eq!(section_for("START"), ".text");
            assert_eq!(section_for("COUNT"), ".data");
        }
    }
}
