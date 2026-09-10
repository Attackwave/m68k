//! Cycle counting, memory map, and hardware register database analysis.

use serde::{Deserialize, Serialize};

use m68k_core::tokens::split_line;
use m68k_lsp::analysis::cycles::estimate_cycles;

#[derive(Debug, Clone, Deserialize)]
pub struct CycleAnalysisRequest {
    pub source: String,
    pub cpu: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LineCycleItem {
    pub line: usize,
    pub mnemonic: String,
    pub min_cycles: u32,
    pub max_cycles: u32,
    pub comment: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CycleAnalysisResponse {
    pub lines: Vec<LineCycleItem>,
    pub total_min_cycles: u32,
    pub total_max_cycles: u32,
    pub pal_scanlines: f64,
    pub frame_percent_pal: f64, // PAL 50Hz frame = ~70,937 cycles
}

pub fn analyze_source_cycles(req: CycleAnalysisRequest) -> CycleAnalysisResponse {
    let mut lines_out = Vec::new();
    let mut total_min = 0u32;
    let mut total_max = 0u32;

    for (idx, line_str) in req.source.lines().enumerate() {
        let (_label, mnem, operands, _) = split_line(line_str);
        if mnem.is_empty() {
            continue;
        }

        let mnem_str = mnem.as_str();
        let (base_mnem, size_suffix) = if let Some(dot_pos) = mnem_str.find('.') {
            (&mnem_str[..dot_pos], Some(&mnem_str[dot_pos + 1..]))
        } else {
            (mnem_str, None)
        };

        let ops_vec: Vec<String> = operands
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if let Some(cycle_info) = estimate_cycles(base_mnem, size_suffix, &ops_vec)
            && cycle_info.cycles > 0
        {
            total_min += cycle_info.cycles;
            total_max += cycle_info.cycles;

            lines_out.push(LineCycleItem {
                line: idx,
                mnemonic: mnem.to_string(),
                min_cycles: cycle_info.cycles,
                max_cycles: cycle_info.cycles,
                comment: cycle_info.display,
            });
        }
    }

    let avg_cycles = (total_min + total_max) as f64 / 2.0;
    let pal_scanlines = avg_cycles / 227.5; // ~227.5 cycles per PAL raster line
    let frame_percent = (avg_cycles / 70937.0) * 100.0;

    CycleAnalysisResponse {
        lines: lines_out,
        total_min_cycles: total_min,
        total_max_cycles: total_max,
        pal_scanlines,
        frame_percent_pal: frame_percent,
    }
}

/// Bitfield definition for hardware registers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterBitField {
    pub bit_range: String,
    pub name: String,
    pub description: String,
}

/// Hardware register definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareRegisterInfo {
    pub system: String, // "amiga", "megadrive", "atarist"
    pub name: String,
    pub address_hex: String,
    pub address_num: u32,
    pub read_write: String, // "R", "W", "R/W"
    pub description: String,
    pub bitfields: Vec<RegisterBitField>,
}

pub fn get_hardware_register_database() -> Vec<HardwareRegisterInfo> {
    vec![
        // Amiga Custom Chip Registers ($DFF000)
        HardwareRegisterInfo {
            system: "amiga".to_string(),
            name: "DMACON / DMACONW".to_string(),
            address_hex: "$DFF096".to_string(),
            address_num: 0xDFF096,
            read_write: "W".to_string(),
            description: "DMA Control write: Enables or disables DMA channels (Bit 15 sets or clears bits 0-14).".to_string(),
            bitfields: vec![
                RegisterBitField { bit_range: "15".to_string(), name: "SET/CLR".to_string(), description: "1 = Set specified bits, 0 = Clear specified bits".to_string() },
                RegisterBitField { bit_range: "14".to_string(), name: "BBUSY".to_string(), description: "Blitter Busy-status (Read in DMACONR)".to_string() },
                RegisterBitField { bit_range: "13".to_string(), name: "BZERO".to_string(), description: "Blitter Zero-flag (Read in DMACONR)".to_string() },
                RegisterBitField { bit_range: "10".to_string(), name: "BLTPRI".to_string(), description: "Blitter Nasty (Priority over 68000 CPU)".to_string() },
                RegisterBitField { bit_range: "9".to_string(), name: "DMAEN".to_string(), description: "Master DMA Enable".to_string() },
                RegisterBitField { bit_range: "8".to_string(), name: "BPLEN".to_string(), description: "Bitplane DMA Enable".to_string() },
                RegisterBitField { bit_range: "7".to_string(), name: "COPEN".to_string(), description: "Copper DMA Enable".to_string() },
                RegisterBitField { bit_range: "6".to_string(), name: "BLTEN".to_string(), description: "Blitter DMA Enable".to_string() },
                RegisterBitField { bit_range: "5".to_string(), name: "SPREN".to_string(), description: "Sprite DMA Enable".to_string() },
                RegisterBitField { bit_range: "4".to_string(), name: "DSKEN".to_string(), description: "Disk DMA Enable".to_string() },
                RegisterBitField { bit_range: "3-0".to_string(), name: "AUD3-0EN".to_string(), description: "Audio Channels 0-3 DMA Enable".to_string() },
            ],
        },
        HardwareRegisterInfo {
            system: "amiga".to_string(),
            name: "INTENA / INTENAW".to_string(),
            address_hex: "$DFF09A".to_string(),
            address_num: 0xDFF09A,
            read_write: "W".to_string(),
            description: "Interrupt Enable write: Enables/disables hardware interrupts (Bit 15 sets or clears).".to_string(),
            bitfields: vec![
                RegisterBitField { bit_range: "15".to_string(), name: "SET/CLR".to_string(), description: "1 = Set bits, 0 = Clear bits".to_string() },
                RegisterBitField { bit_range: "14".to_string(), name: "INTEN".to_string(), description: "Master Interrupt Enable".to_string() },
                RegisterBitField { bit_range: "13".to_string(), name: "EXTER".to_string(), description: "External Interrupt (Level 6)".to_string() },
                RegisterBitField { bit_range: "12".to_string(), name: "DSKSYN".to_string(), description: "Disk Sync (Level 5)".to_string() },
                RegisterBitField { bit_range: "11".to_string(), name: "RBF".to_string(), description: "Serial Receive Buffer Full (Level 5)".to_string() },
                RegisterBitField { bit_range: "10".to_string(), name: "AUD3".to_string(), description: "Audio Channel 3 (Level 4)".to_string() },
                RegisterBitField { bit_range: "9".to_string(), name: "AUD2".to_string(), description: "Audio Channel 2 (Level 4)".to_string() },
                RegisterBitField { bit_range: "8".to_string(), name: "AUD1".to_string(), description: "Audio Channel 1 (Level 4)".to_string() },
                RegisterBitField { bit_range: "7".to_string(), name: "AUD0".to_string(), description: "Audio Channel 0 (Level 4)".to_string() },
                RegisterBitField { bit_range: "6".to_string(), name: "BLIT".to_string(), description: "Blitter Finished (Level 3)".to_string() },
                RegisterBitField { bit_range: "5".to_string(), name: "VERTB".to_string(), description: "Vertical Blank Interrupt (Level 3)".to_string() },
                RegisterBitField { bit_range: "4".to_string(), name: "COPER".to_string(), description: "Copper Interrupt (Level 3)".to_string() },
                RegisterBitField { bit_range: "3".to_string(), name: "PORTS".to_string(), description: "I/O Ports & Timers (Level 2)".to_string() },
                RegisterBitField { bit_range: "2".to_string(), name: "SOFT".to_string(), description: "Software Interrupt (Level 1)".to_string() },
                RegisterBitField { bit_range: "1".to_string(), name: "DSKBLK".to_string(), description: "Disk Block Finished (Level 1)".to_string() },
                RegisterBitField { bit_range: "0".to_string(), name: "TBE".to_string(), description: "Serial Transmit Buffer Empty (Level 1)".to_string() },
            ],
        },
        HardwareRegisterInfo {
            system: "amiga".to_string(),
            name: "BPLCON0".to_string(),
            address_hex: "$DFF100".to_string(),
            address_num: 0xDFF100,
            read_write: "W".to_string(),
            description: "Bitplane Control Register 0: Sets number of bitplanes, color modes, HAM, interlace, high-res.".to_string(),
            bitfields: vec![
                RegisterBitField { bit_range: "15".to_string(), name: "HIRES".to_string(), description: "High-Resolution 640x256 mode (1 = Hires, 0 = Lores)".to_string() },
                RegisterBitField { bit_range: "14-12".to_string(), name: "BPU2-0".to_string(), description: "Bitplane count (000=0 planes, 001=1, 010=2, 011=3, 100=4, 101=5, 110=6/EHB)".to_string() },
                RegisterBitField { bit_range: "11".to_string(), name: "HAM".to_string(), description: "Hold-And-Modify Mode (4096 simultaneous colors)".to_string() },
                RegisterBitField { bit_range: "10".to_string(), name: "DPF".to_string(), description: "Dual Playfield Mode (2 independent parallax planes)".to_string() },
                RegisterBitField { bit_range: "9".to_string(), name: "COLOR".to_string(), description: "Color composite video enable (always 1 on PAL/NTSC)".to_string() },
                RegisterBitField { bit_range: "8".to_string(), name: "GAUD".to_string(), description: "Genlock Audio enable".to_string() },
                RegisterBitField { bit_range: "7".to_string(), name: "UHRES".to_string(), description: "Ultra-Hires (AGA)".to_string() },
                RegisterBitField { bit_range: "6".to_string(), name: "SHRES".to_string(), description: "Super-Hires (ECS/AGA)".to_string() },
                RegisterBitField { bit_range: "5".to_string(), name: "BYPASS".to_string(), description: "Bypass color palette (AGA)".to_string() },
                RegisterBitField { bit_range: "4".to_string(), name: "BPU3".to_string(), description: "Bitplane count bit 3 (for 7-8 bitplanes in AGA)".to_string() },
                RegisterBitField { bit_range: "3".to_string(), name: "LPEN".to_string(), description: "Light Pen latch enable".to_string() },
                RegisterBitField { bit_range: "2".to_string(), name: "LACE".to_string(), description: "Interlace video mode (512 lines PAL / 400 lines NTSC)".to_string() },
                RegisterBitField { bit_range: "1".to_string(), name: "ERSY".to_string(), description: "External resync".to_string() },
            ],
        },
        HardwareRegisterInfo {
            system: "amiga".to_string(),
            name: "BLTCON0".to_string(),
            address_hex: "$DFF040".to_string(),
            address_num: 0xDFF040,
            read_write: "W".to_string(),
            description: "Blitter Control Register 0: Source/Dest channel enables, A-Shift, and 8-bit Logic Minterm.".to_string(),
            bitfields: vec![
                RegisterBitField { bit_range: "15-12".to_string(), name: "ASH3-0".to_string(), description: "Channel A Shift amount (0 to 15 pixels)".to_string() },
                RegisterBitField { bit_range: "11".to_string(), name: "USEA".to_string(), description: "Channel A Enable (Source / Mask)".to_string() },
                RegisterBitField { bit_range: "10".to_string(), name: "USEB".to_string(), description: "Channel B Enable (Source / Pattern)".to_string() },
                RegisterBitField { bit_range: "9".to_string(), name: "USEC".to_string(), description: "Channel C Enable (Source / Background)".to_string() },
                RegisterBitField { bit_range: "8".to_string(), name: "USED".to_string(), description: "Channel D Enable (Destination)".to_string() },
                RegisterBitField { bit_range: "7-0".to_string(), name: "LF7-0".to_string(), description: "Logic Function Minterm ($F0=Copy, $CA=Cookie-Cut, $50=Invert, $60=XOR)".to_string() },
            ],
        },
        HardwareRegisterInfo {
            system: "amiga".to_string(),
            name: "BLTCON1".to_string(),
            address_hex: "$DFF042".to_string(),
            address_num: 0xDFF042,
            read_write: "W".to_string(),
            description: "Blitter Control Register 1: B-Shift, Descending mode, Fill mode, and Line Drawing mode.".to_string(),
            bitfields: vec![
                RegisterBitField { bit_range: "15-12".to_string(), name: "BSH3-0".to_string(), description: "Channel B Shift amount (0 to 15 pixels)".to_string() },
                RegisterBitField { bit_range: "4".to_string(), name: "EFE".to_string(), description: "Exclusive Fill Enable".to_string() },
                RegisterBitField { bit_range: "3".to_string(), name: "IFE".to_string(), description: "Inclusive Fill Enable".to_string() },
                RegisterBitField { bit_range: "2".to_string(), name: "FCI".to_string(), description: "Fill Carry Input (0 or 1)".to_string() },
                RegisterBitField { bit_range: "1".to_string(), name: "DESC".to_string(), description: "Descending mode (1 = Decrement pointers, right-to-left for overlaps)".to_string() },
                RegisterBitField { bit_range: "0".to_string(), name: "LINE".to_string(), description: "Line Drawing mode enable (1 = Line mode, 0 = Area mode)".to_string() },
            ],
        },
        // Sega Mega Drive VDP Registers ($C00004)
        HardwareRegisterInfo {
            system: "megadrive".to_string(),
            name: "VDP Control Port".to_string(),
            address_hex: "$C00004".to_string(),
            address_num: 0xC00004,
            read_write: "W".to_string(),
            description: "Sega Mega Drive VDP Control & Register Port: Accesses VRAM, CRAM, VSRAM, and internal VDP registers 0-23.".to_string(),
            bitfields: vec![
                RegisterBitField { bit_range: "Reg 0 ($80xx)".to_string(), name: "Mode Set 1".to_string(), description: "H-Interrupt enable, Freeze palette, HV Counter latch".to_string() },
                RegisterBitField { bit_range: "Reg 1 ($81xx)".to_string(), name: "Mode Set 2".to_string(), description: "Display enable, V-Interrupt enable, DMA enable, PAL 28/30 cell".to_string() },
                RegisterBitField { bit_range: "Reg 2 ($82xx)".to_string(), name: "Plane A Base".to_string(), description: "Pattern Name Table address for Scroll Plane A (VRAM / $2000)".to_string() },
                RegisterBitField { bit_range: "Reg 3 ($83xx)".to_string(), name: "Window Base".to_string(), description: "Pattern Name Table address for Window Plane (VRAM / $2000)".to_string() },
                RegisterBitField { bit_range: "Reg 4 ($84xx)".to_string(), name: "Plane B Base".to_string(), description: "Pattern Name Table address for Scroll Plane B (VRAM / $2000)".to_string() },
                RegisterBitField { bit_range: "Reg 5 ($85xx)".to_string(), name: "Sprite Table".to_string(), description: "Sprite Attribute Table address in VRAM (VRAM / $200)".to_string() },
                RegisterBitField { bit_range: "Reg 7 ($87xx)".to_string(), name: "Backdrop Color".to_string(), description: "Palette line and color index for background color".to_string() },
            ],
        },
    ]
}
