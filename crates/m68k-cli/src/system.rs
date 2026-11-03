//! What `m68k-disasm --system` knows about a target machine.
//!
//! The hardware names come from the include files in `include/`, read by
//! the assembler itself so the two tools cannot disagree about an address:
//! every equate whose value lies in one of the system's address ranges
//! becomes a label, and `move.w d0,$c00004` reads `move.w d0,VDP_CTRL`.
//! A system with a fixed cartridge layout also contributes the vector
//! table (pointers, and entry points for every vector inside the image)
//! and the header (data). Atari TOS adds the names of its `trap` calls.

use std::collections::HashMap;

use m68k_asm::assembler::Assembler;
use m68k_disasm::disassembler::WriteNote;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum System {
    /// Sega Mega Drive / Genesis cartridge: 64 vectors, header at $100,
    /// VDP, I/O and Z80 names
    Megadrive,
    /// SNK Neo Geo P-ROM: 64 vectors, header at $100 with its entry
    /// jumps, system and LSPC registers, BIOS calls and BIOS RAM
    Neogeo,
    /// Atari ST/STE: hardware registers, system variables, exception
    /// vectors, and names for GEMDOS, BIOS and XBIOS trap calls
    Atarist,
}

/// A trap call's name by `(trap number, function number)`.
pub type TrapName = ((u8, u16), String);

/// What the disassembler is told about an image for a system.
#[derive(Default)]
pub struct SystemFacts {
    pub labels: Vec<(u32, String)>,
    pub data_ranges: Vec<(u32, u32)>,
    pub pointer_sites: Vec<u32>,
    pub entry_points: Vec<u32>,
    pub trap_names: Vec<TrapName>,
    /// What an immediate written to a hardware address means.
    pub write_notes: Vec<(u32, WriteNote)>,
}

const MEGADRIVE_HARDWARE: &str = include_str!("../../../include/megadrive/hardware.i");
const NEOGEO_HARDWARE: &str = include_str!("../../../include/neogeo/hardware.i");
const ATARIST_HARDWARE: &str = include_str!("../../../include/atarist/hardware.i");
const ATARIST_TOS: &str = include_str!("../../../include/atarist/tos.i");

/// The 64 exception vectors a 68000 reads from the start of a cartridge.
const VECTOR_TABLE: (u32, u32) = (0, 0x100);
/// The cartridge header behind them on the Mega Drive and the Neo Geo.
const HEADER: (u32, u32) = (0x100, 0x200);
/// The four `JMP` entries in a Neo Geo header: USER, PLAYER_START,
/// DEMO_END, COIN_SOUND.
const NEOGEO_HEADER_JUMPS: [u32; 4] = [0x122, 0x128, 0x12E, 0x134];

/// Every defined symbol of an assembler source: `(name, value, line)`, in
/// source order. The source is assembled at origin 0, so labels are
/// offsets and equates are their values.
pub fn equates(source: &str) -> Result<Vec<(String, u32, usize)>, String> {
    let mut asm = Assembler::new(0);
    asm.assemble(source).map_err(|e| e.message)?;
    let mut found: Vec<(String, u32, usize)> = asm
        .symbols
        .iter()
        .filter(|(_, entry)| entry.defined)
        .map(|(name, entry)| (name.clone(), entry.value, entry.line_no.unwrap_or(0)))
        .collect();
    found.sort_by_key(|(_, _, line)| *line);
    Ok(found)
}

fn within(ranges: &[(u32, u32)], value: u32) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| (start..end).contains(&value))
}

/// GEMDOS, BIOS and XBIOS function names by `(trap, number)`, from the
/// equates of `tos.i`. The file groups its equates under `; ---`
/// headings; an equate belongs to the heading above it, and only a
/// heading that names a trap (`; --- GEMDOS, trap #1 ---`) holds
/// function numbers.
fn tos_trap_names() -> Result<Vec<TrapName>, String> {
    let mut headings: Vec<(usize, Option<u8>)> = Vec::new();
    for (index, line) in ATARIST_TOS.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("; ---") {
            let trap = rest.split_once("trap #").and_then(|(_, after)| {
                let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            });
            headings.push((index + 1, trap));
        }
    }
    let mut names = Vec::new();
    for (name, value, line) in equates(ATARIST_TOS)? {
        let heading = headings.iter().rev().find(|(at, _)| *at < line);
        if let Some(&(_, Some(trap))) = heading
            && let Ok(number) = u16::try_from(value)
        {
            names.push(((trap, number), name));
        }
    }
    Ok(names)
}

/// The VDP's access code from a control-port longword: CD1-0 sit in bits
/// 31-30, CD5-2 in bits 7-4.
fn vdp_access_code(value: u32) -> u32 {
    ((value >> 30) & 3) | (((value >> 4) & 0xF) << 2)
}

/// What a value written to the Mega Drive VDP control port means: a word
/// `$8rvv` sets register `r` to `vv` (a longword can set two), any other
/// longword selects a VRAM, CRAM or VSRAM address for the data port. The
/// register and access names come from `hardware.i`.
fn vdp_control_note() -> Result<WriteNote, String> {
    let symbols = equates(MEGADRIVE_HARDWARE)?;
    let registers: HashMap<u32, String> = symbols
        .iter()
        .filter_map(|(name, value, _)| Some((*value, name.strip_prefix("VDPREG_")?.to_string())))
        .collect();
    // VDP_VRAM_WRITE and friends are the full control longwords; what
    // identifies the access is their code.
    let accesses: HashMap<u32, String> = symbols
        .iter()
        .filter_map(|(name, value, _)| {
            let (memory, direction) = name.strip_prefix("VDP_")?.rsplit_once('_')?;
            matches!(direction, "READ" | "WRITE").then(|| {
                (
                    vdp_access_code(*value),
                    format!("{} {}", memory, direction.to_ascii_lowercase()),
                )
            })
        })
        .collect();
    let register_write = move |word: u32| -> Option<String> {
        if word & 0xE000 != 0x8000 {
            return None;
        }
        let (reg, value) = ((word >> 8) & 0x1F, word & 0xFF);
        Some(match registers.get(&reg) {
            Some(name) => format!("VDP reg {} {} = ${:02x}", reg, name, value),
            None => format!("VDP reg {} = ${:02x}", reg, value),
        })
    };
    Ok(Box::new(move |value, size| {
        let value = u32::try_from(value).ok()?;
        match size {
            2 => register_write(value),
            4 => match (register_write(value >> 16), register_write(value & 0xFFFF)) {
                (Some(high), Some(low)) => Some(format!("{}; {}", high, low)),
                _ => {
                    let code = vdp_access_code(value);
                    let address = ((value >> 16) & 0x3FFF) | ((value & 3) << 14);
                    // CD5 (bit 7 of the low word) starts a DMA transfer.
                    let dma = if code & 0x20 != 0 { " (DMA)" } else { "" };
                    Some(format!(
                        "VDP {} ${:04x}{}",
                        accesses.get(&(code & 0xF))?,
                        address,
                        dma
                    ))
                }
            },
            _ => None,
        }
    }))
}

impl System {
    fn sources(self) -> &'static [&'static str] {
        match self {
            System::Megadrive => &[MEGADRIVE_HARDWARE],
            System::Neogeo => &[NEOGEO_HARDWARE],
            System::Atarist => &[ATARIST_HARDWARE],
        }
    }

    /// Where the system's equates denote addresses rather than register
    /// numbers, bit masks or sizes.
    fn address_ranges(self) -> &'static [(u32, u32)] {
        match self {
            // Header, Z80 and I/O area, VDP, work RAM.
            System::Megadrive => &[
                HEADER,
                (0xA0_0000, 0xA2_0000),
                (0xC0_0000, 0xC0_0020),
                (0xFF_0000, 0x100_0000),
            ],
            // Header, work RAM with the BIOS variables, system registers,
            // palette RAM, memory card, system ROM calls, backup RAM.
            System::Neogeo => &[
                HEADER,
                (0x10_0000, 0x11_0000),
                (0x30_0000, 0x40_2000),
                (0x80_0000, 0x80_0001),
                (0xC0_0000, 0xC2_0000),
                (0xD0_0000, 0xD1_0000),
            ],
            // Exception vectors and system variables, cartridge and TOS
            // ROM, hardware registers.
            System::Atarist => &[(0x8, 0x600), (0xE0_0000, 0x100_0000)],
        }
    }

    /// The range of in-image addresses that still take labels from the
    /// equates: a cartridge header's fields. Any other equate inside the
    /// image is a coincidence of values, not a name for that place.
    fn header(self) -> Option<(u32, u32)> {
        match self {
            System::Megadrive | System::Neogeo => Some(HEADER),
            System::Atarist => None,
        }
    }

    /// Everything to tell the disassembler about `image` loaded at
    /// `origin`.
    pub fn facts(self, origin: u32, image: &[u8]) -> Result<SystemFacts, String> {
        let in_image = |addr: u32| addr >= origin && (addr - origin) < image.len() as u32;
        let mut facts = SystemFacts::default();
        let header = self.header();
        for source in self.sources() {
            for (name, value, _) in equates(source)? {
                let names_a_place = within(self.address_ranges(), value)
                    && (!in_image(value) || header.is_some_and(|h| within(&[h], value)));
                if names_a_place {
                    facts.labels.push((value, name));
                }
            }
        }

        // A cartridge image starting at 0 begins with the vector table and
        // the header; anything shorter or loaded elsewhere is a fragment.
        let cartridge = header.is_some() && origin == 0 && image.len() >= HEADER.1 as usize;
        if cartridge {
            facts.pointer_sites = (VECTOR_TABLE.0..VECTOR_TABLE.1).step_by(4).collect();
            // Vector 0 is the initial stack pointer; every other vector
            // that points into the image past the header is a handler. An
            // unused vector holds 0 or points at a BIOS outside the image.
            let table = &image[VECTOR_TABLE.0 as usize..VECTOR_TABLE.1 as usize];
            facts.entry_points = table
                .as_chunks::<4>()
                .0
                .iter()
                .skip(1)
                .map(|v| u32::from_be_bytes(*v))
                .filter(|&target| {
                    target >= HEADER.1 && target.is_multiple_of(2) && in_image(target)
                })
                .collect();
            match self {
                System::Neogeo => {
                    let jumps = NEOGEO_HEADER_JUMPS;
                    facts.data_ranges = vec![(HEADER.0, jumps[0]), (jumps[3] + 6, HEADER.1)];
                    facts.entry_points.extend(jumps);
                }
                _ => facts.data_ranges = vec![HEADER],
            }
        }

        match self {
            System::Atarist => facts.trap_names = tos_trap_names()?,
            System::Megadrive => {
                // The control port and its mirror.
                let ctrl = 0xC0_0004;
                facts.write_notes =
                    vec![(ctrl, vdp_control_note()?), (ctrl + 2, vdp_control_note()?)];
            }
            System::Neogeo => {}
        }
        Ok(facts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Labels as a map, the shape the tests look things up in.
    fn label_map(facts: &SystemFacts) -> HashMap<&str, u32> {
        facts
            .labels
            .iter()
            .map(|(addr, name)| (name.as_str(), *addr))
            .collect()
    }

    #[test]
    fn megadrive_names_hardware_and_header_but_not_register_numbers() {
        let mut rom = vec![0u8; 0x400];
        rom[4..8].copy_from_slice(&0x200u32.to_be_bytes()); // reset
        rom[0x78..0x7C].copy_from_slice(&0x300u32.to_be_bytes()); // vblank
        rom[0x7C..0x80].copy_from_slice(&0x301u32.to_be_bytes()); // odd: not code
        let facts = System::Megadrive.facts(0, &rom).unwrap();
        let labels = label_map(&facts);
        assert_eq!(labels["VDP_CTRL"], 0xC0_0004);
        assert_eq!(labels["HEADER_CHECKSUM"], 0x18E);
        // ROM_START is 0: inside the image and not a header field.
        assert!(!labels.contains_key("ROM_START"));
        // A VDP register number is not an address.
        assert!(!labels.contains_key("VDP_REG_MODE2"));
        assert_eq!(facts.pointer_sites.len(), 64);
        assert_eq!(facts.entry_points, vec![0x200, 0x300]);
        assert_eq!(facts.data_ranges, vec![(0x100, 0x200)]);
        assert!(facts.trap_names.is_empty());
    }

    #[test]
    fn megadrive_explains_what_is_written_to_the_vdp_control_port() {
        let facts = System::Megadrive.facts(0x1000, &[0x4E, 0x75]).unwrap();
        let note = |addr: u32| {
            &facts
                .write_notes
                .iter()
                .find(|(a, _)| *a == addr)
                .unwrap()
                .1
        };
        let ctrl = note(0xC0_0004);
        assert_eq!(ctrl(0x8F02, 2).as_deref(), Some("VDP reg 15 AUTOINC = $02"));
        assert_eq!(ctrl(0x8600, 2).as_deref(), Some("VDP reg 6 = $00"));
        assert_eq!(ctrl(0x1234, 2), None);
        assert_eq!(
            ctrl(0x8104_8F02, 4).as_deref(),
            Some("VDP reg 1 MODE2 = $04; VDP reg 15 AUTOINC = $02")
        );
        assert_eq!(
            ctrl(0xC000_0000, 4).as_deref(),
            Some("VDP CRAM write $0000")
        );
        // VRAM write to $c000 with DMA: A15-14 in bits 1-0, DMA in bit 7.
        assert_eq!(
            ctrl(0x4000_0083, 4).as_deref(),
            Some("VDP VRAM write $c000 (DMA)")
        );
        assert_eq!(ctrl(0x1234_5678, 4), None);
        assert_eq!(ctrl(0x8F, 1), None);
        assert!(note(0xC0_0006)(0x8F02, 2).is_some());
    }

    #[test]
    fn a_fragment_gets_names_only() {
        let facts = System::Megadrive.facts(0x1000, &[0x4E, 0x75]).unwrap();
        assert!(facts.pointer_sites.is_empty());
        assert!(facts.entry_points.is_empty());
        assert!(facts.data_ranges.is_empty());
        assert!(!facts.labels.is_empty());
    }

    #[test]
    fn neogeo_header_jumps_are_entry_points_between_data() {
        let rom = vec![0u8; 0x400];
        let facts = System::Neogeo.facts(0, &rom).unwrap();
        let labels = label_map(&facts);
        assert_eq!(labels["REG_VRAMADDR"], 0x3C_0000);
        assert_eq!(labels["BIOS_P1CURRENT"], 0x10_FD96);
        assert_eq!(facts.entry_points, vec![0x122, 0x128, 0x12E, 0x134]);
        assert_eq!(facts.data_ranges, vec![(0x100, 0x122), (0x13A, 0x200)]);
    }

    #[test]
    fn atarist_names_registers_vectors_and_trap_calls() {
        let facts = System::Atarist.facts(0x20000, &[0x4E, 0x75]).unwrap();
        let labels = label_map(&facts);
        assert_eq!(labels["VEC_VBL"], 0x70);
        assert_eq!(labels["MFP_GPIP"], 0xFF_FA01);
        assert_eq!(labels["_hz_200"], 0x4BA);
        let traps: HashMap<(u8, u16), &str> = facts
            .trap_names
            .iter()
            .map(|(key, name)| (*key, name.as_str()))
            .collect();
        assert_eq!(traps[&(1, 9)], "Cconws");
        assert_eq!(traps[&(1, 0)], "Pterm0");
        assert_eq!(traps[&(13, 3)], "Bconout");
        assert_eq!(traps[&(14, 37)], "Vsync");
    }
}
