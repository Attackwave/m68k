//! Project management, configuration and template scaffolding.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    pub target_cpu: String,
    pub main_file: String,
    pub output_format: String, // "binary", "adf", "hunk", "srecord"
    pub origin_address: String,
    pub emulator_profile: String, // "fsuae", "winuae", "blastem", "custom"
    pub include_dirs: Vec<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: "NewM68kProject".to_string(),
            target_cpu: "68000".to_string(),
            main_file: "src/main.s".to_string(),
            output_format: "adf".to_string(),
            origin_address: "$000000".to_string(),
            emulator_profile: "fsuae".to_string(),
            include_dirs: vec!["includes".to_string()],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScaffoldProjectRequest {
    pub target_directory: String,
    pub template: String, // "amiga500", "amiga_hunk", "megadrive", "baremetal"
    pub project_name: String,
}

pub fn scaffold_project(req: ScaffoldProjectRequest) -> Result<ProjectConfig, String> {
    let base_dir = PathBuf::from(&req.target_directory);
    fs::create_dir_all(&base_dir).map_err(|e| e.to_string())?;

    let src_dir = base_dir.join("src");
    let inc_dir = base_dir.join("includes");
    fs::create_dir_all(&src_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&inc_dir).map_err(|e| e.to_string())?;

    let mut config = ProjectConfig {
        name: req.project_name.clone(),
        ..Default::default()
    };

    match req.template.as_str() {
        "amiga500" => {
            config.target_cpu = "68000".to_string();
            config.output_format = "adf".to_string();

            // Write includes/custom.i
            let custom_i = r#"; Amiga Custom Chip Hardware Offsets
CUSTOM_BASE     EQU $DFF000
DMACON          EQU $096
DMACONR         EQU $002
INTENA          EQU $09A
INTENAR         EQU $01C
INTREQ          EQU $09C
INTREQR         EQU $01E
COP1LCH         EQU $080
COP1LCL         EQU $082
COPJMP1         EQU $088
COLOR00         EQU $180
COLOR01         EQU $182
BPLCON0         EQU $100
VPOSR           EQU $004
VHPOSR          EQU $006
"#;
            fs::write(inc_dir.join("custom.i"), custom_i).map_err(|e| e.to_string())?;

            // Write src/main.s
            let main_s = r#"; Amiga 500 Bare-Metal Copper Demo
    SECTION Code,CODE

    INCLUDE "custom.i"

Start:
    ; Take over hardware
    move.l  $4.w,a6             ; ExecBase
    suba.l  a1,a1
    jsr     -$126(a6)           ; FindTask(NULL)
    move.l  d0,a4

    lea     CUSTOM_BASE,a6
    move.w  #$7FFF,DMACON(a6)   ; Disable DMA
    move.w  #$7FFF,INTENA(a6)   ; Disable Interrupts

    ; Install Copperlist
    lea     CopperList(pc),a0
    move.l  a0,COP1LCH(a6)
    move.w  #0,COPJMP1(a6)
    move.w  #$8280,DMACON(a6)   ; Enable DMA + Copper

MainLoop:
    ; Check left mouse button (CIAA PRA bit 6)
    btst    #6,$BFE001
    bne.s   MainLoop

    ; Restore system and exit
    move.w  #$8020,DMACON(a6)
    rts

    SECTION Data,DATA_C

CopperList:
    dc.w    BPLCON0,$0000       ; 0 bitplanes
    dc.w    COLOR00,$0002       ; Dark blue background
    dc.w    $8001,$FFFE         ; Wait line 128
    dc.w    COLOR00,$0F80       ; Orange raster bar
    dc.w    $8801,$FFFE         ; Wait line 136
    dc.w    COLOR00,$0002       ; Restore blue
    dc.w    $FFFF,$FFFE         ; End of copperlist
"#;
            fs::write(src_dir.join("main.s"), main_s).map_err(|e| e.to_string())?;
        }
        "megadrive" => {
            config.target_cpu = "68000".to_string();
            config.output_format = "binary".to_string();
            config.origin_address = "$000000".to_string();
            config.emulator_profile = "blastem".to_string();

            let main_s = r#"; Sega Mega Drive / Genesis ROM Header & Entry
    ORG     $000000

    ; Vector Table
    dc.l    $00FFE000           ; Initial Stack Pointer
    dc.l    EntryPoint          ; Reset Handler
    dcb.l   62, DefaultHandler  ; Unused vectors

    ; Header
    dc.b    "SEGA MEGA DRIVE "  ; Console Name
    dc.b    "(C)ATTACKWAVE   "  ; Copyright
    dc.b    "M68K STUDIO DEMO                                " ; Title
    dc.b    "M68K STUDIO DEMO                                "
    dc.b    "GM 00000000-00"    ; Serial / Version
    dc.w    $0000               ; Checksum
    dc.b    "J6              "  ; I/O Support
    dc.l    $00000000           ; ROM Start
    dc.l    $0007FFFF           ; ROM End (512 KB)
    dc.l    $00FF0000           ; RAM Start
    dc.l    $00FFFFFF           ; RAM End
    dc.b    "            "      ; SRAM Support
    dc.b    "                                        " ; Notes
    dc.b    "JUE             "  ; Country Support

EntryPoint:
    ; Unlock VDP TMSS if present
    move.b  $00A10001,d0
    andi.b  #$0F,d0
    beq.s   .no_tmss
    move.l  #"SEGA",$00A14000
.no_tmss:

MainLoop:
    nop
    bra.s   MainLoop

DefaultHandler:
    rte
"#;
            fs::write(src_dir.join("main.s"), main_s).map_err(|e| e.to_string())?;
        }
        _ => {
            // Default bare metal
            config.output_format = "binary".to_string();
            let main_s = r#"; Motorola 68000 Bare Metal Template
    ORG     $001000

Start:
    moveq   #10,d0
    moveq   #0,d1

.loop:
    add.l   d0,d1
    subq.l  #1,d0
    bne.s   .loop

    rts
"#;
            fs::write(src_dir.join("main.s"), main_s).map_err(|e| e.to_string())?;
        }
    }

    // Write project.json
    let config_json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(base_dir.join("project.json"), config_json).map_err(|e| e.to_string())?;

    Ok(config)
}

pub fn load_project_config(project_path: String) -> Result<ProjectConfig, String> {
    let p = Path::new(&project_path);
    let config_file = if p.is_dir() {
        p.join("project.json")
    } else {
        p.to_path_buf()
    };

    let content = fs::read_to_string(config_file).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}
