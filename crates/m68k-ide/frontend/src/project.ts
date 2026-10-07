//! Project Management and Template Engine for m68k Studio.

export interface ProjectFileTemplate {
  id: string;
  name: string;
  extension: string;
  language: 'm68k' | 'python';
  system: 'amiga' | 'megadrive' | 'atarist' | 'baremetal' | 'custom';
  description: string;
  content: string;
}

export interface ProjectTemplate {
  id: string;
  name: string;
  system: 'amiga' | 'megadrive' | 'atarist' | 'baremetal' | 'custom';
  language: 'm68k' | 'python';
  defaultProfileId: string;
  description: string;
  icon: string;
  files: Record<string, string>;
  mainFile: string;
  isCustom?: boolean;
}

export interface StudioProject {
  id: string;
  name: string;
  version: string;
  profileId: string;
  mainFile: string;
  files: Record<string, string>;
  created: number;
  lastModified: number;
}

export const BUILTIN_PROJECT_TEMPLATES: ProjectTemplate[] = [
  {
    id: 'amiga500_py',
    name: 'Amiga 500 Python Copper Demo',
    system: 'amiga',
    language: 'python',
    defaultProfileId: 'amiga500',
    description: 'Bare-metal Amiga 500 Demo in Python mit direkter Copper- & Rasterzeilen-Steuerung.',
    icon: '🐍',
    mainFile: 'src/main.py',
    files: {
      'src/main.py': `# =============================================================================
# Amiga 500 Python Copper Raster Demo
# Powered by m68k Studio Python-to-m68k Transpiler
# =============================================================================
from m68k.amiga import custom, wait_vbl, wait_line

def main():
    # 1. Hardware initialisieren & DMA abschalten
    custom.dmacon = 0x7FFF     # Disable DMA
    custom.intena = 0x7FFF     # Disable Interrupts
    custom.bplcon0 = 0x0000    # 0 Bitplanes
    custom.color00 = 0x0002    # Dunkelblauer Hintergrund

    # 2. Main Loop: Rasterbar Animation
    while True:
        wait_line(128)
        custom.color00 = 0x0F80  # Orange Rasterzeile
        wait_line(136)
        custom.color00 = 0x0002  # Wieder Dunkelblau
`,
      'includes/hardware.i': `; Amiga Hardware Offsets
CUSTOM_BASE EQU $DFF000
COLOR00     EQU $180
DMACON      EQU $096
INTENA      EQU $09A
`,
      'README.md': `# Amiga 500 Python Demo\n\nKompiliert mit m68k Studio direkt zu nativem 68000 Maschinencode.\nDrücke **F7 (Build)** oder **F5 (Run)**!\n`,
    },
  },
  {
    id: 'amiga500_asm',
    name: 'Amiga 500 OCS Assembly Copper Demo',
    system: 'amiga',
    language: 'm68k',
    defaultProfileId: 'amiga500',
    description: 'Klassische Amiga OCS Demo mit Copperlist, Farbbalken und CIA Mouse-Check.',
    icon: '🕹️',
    mainFile: 'src/main.s',
    files: {
      'src/main.s': `; =============================================================================
; Amiga 500 Bare-Metal Copper Raster Demo (ADF Bootable)
; Powered by m68k Studio IDE
; =============================================================================
    SECTION Code,CODE

    INCLUDE "includes/custom.i"

Start:
    ; 1. Take over hardware
    move.l  $4.w,a6             ; ExecBase
    suba.l  a1,a1
    jsr     -$126(a6)           ; FindTask(NULL)

    lea     CUSTOM_BASE,a6
    move.w  #$7FFF,DMACON(a6)   ; Disable DMA
    move.w  #$7FFF,INTENA(a6)   ; Disable Interrupts

    ; 2. Install Copperlist
    lea     CopperList(pc),a0
    move.l  a0,COP1LCH(a6)
    move.w  #0,COPJMP1(a6)
    move.w  #$8280,DMACON(a6)   ; Enable DMA + Copper

MainLoop:
    ; Check left mouse button (CIAA PRA bit 6)
    btst    #6,$BFE001
    bne.s   MainLoop

    ; 3. Restore system and exit
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
`,
      'includes/custom.i': `; Amiga Custom Chip Register Definitions
CUSTOM_BASE     EQU $DFF000
DMACON          EQU $096
INTENA          EQU $09A
COP1LCH         EQU $080
COPJMP1         EQU $088
COLOR00         EQU $180
BPLCON0         EQU $100
`,
      'README.md': `# Amiga 500 Assembly Demo\n\nBootfähiges ADF-Demo Projekt für OCS/ECS Amiga Systeme.\n`,
    },
  },
  {
    id: 'megadrive_py',
    name: 'Sega Mega Drive Python ROM',
    system: 'megadrive',
    language: 'python',
    defaultProfileId: 'megadrive',
    description: 'Sega Mega Drive / Genesis ROM mit TMSS-Initialisierung und VDP in Python.',
    icon: '🐍',
    mainFile: 'src/rom.py',
    files: {
      'src/rom.py': `# =============================================================================
# Sega Mega Drive Python ROM
# Powered by m68k Studio Python-to-m68k Transpiler
# =============================================================================
from m68k.megadrive import vdp

def main():
    # VDP Display Modus & Hintergrundfarbe initialisieren
    vdp.control = 0x8004  # Mode Set 1
    vdp.control = 0x8114  # Mode Set 2 (Display ON)
    vdp.backdrop = 0x8700 # Palette Backdrop Color
`,
      'README.md': `# Sega Mega Drive Python ROM\n\nGeneriert ein echtes 512KB/4MB Mega Drive .bin ROM Image.\n`,
    },
  },
  {
    id: 'megadrive_asm',
    name: 'Sega Mega Drive Assembly ROM',
    system: 'megadrive',
    language: 'm68k',
    defaultProfileId: 'megadrive',
    description: 'Vollwertiger Sega Genesis Header, VDP Setup und Interrupt-Vektoren.',
    icon: '🎮',
    mainFile: 'src/rom.s',
    files: {
      'src/rom.s': `; =============================================================================
; Sega Mega Drive / Genesis ROM Starter
; =============================================================================
    ORG $000000

    ; Standard 68000 Vector Table
    dc.l    $00FF0000           ; Initial Stack Pointer (SP)
    dc.l    EntryPoint          ; Initial Program Counter (PC)
    dcb.l   62, DefaultHandler  ; Exception Vectors

    ; ROM Header ($000100)
    dc.b    "SEGA GENESIS    "  ; Console Name
    dc.b    "(C)2026 M68K    "  ; Copyright
    dc.b    "M68K STUDIO TEST ROM                                    "
    dc.b    "M68K STUDIO TEST ROM                                    "
    dc.b    "GM 00000000-00"    ; Version
    dc.w    $0000               ; Checksum
    dc.b    "J6              "  ; I/O Support
    dc.l    $00000000           ; ROM Start Address
    dc.l    $0007FFFF           ; ROM End Address
    dc.l    $00FF0000           ; RAM Start Address
    dc.l    $00FFFFFF           ; RAM End Address

EntryPoint:
    ; TMSS Security check (Model 1 & 2)
    move.b  $A10001,d0
    andi.b  #$0F,d0
    beq.s   InitVDP
    move.l  #"SEGA",$A14000     ; Write 'SEGA' to TMSS register

InitVDP:
    lea     $C00004,a0          ; VDP Control Port
    move.w  #$8004,(a0)         ; Mode set 1
    move.w  #$8114,(a0)         ; Mode set 2 (Display ON)
    move.w  #$8700,(a0)         ; Backdrop color 0

Main:
    bra.s   Main

DefaultHandler:
    rte
`,
      'README.md': `# Sega Mega Drive ASM ROM\n\nStartklar für BlastEm, Mednafen oder echte Mega Drive Hardware!\n`,
    },
  },
  {
    id: 'baremetal_asm',
    name: 'Bare-Metal 68000 System Binary',
    system: 'baremetal',
    language: 'm68k',
    defaultProfileId: 'baremetal',
    description: 'Universelles 68000 Embedded / Minimal-System mit Vektortabelle und RAM-Puffer.',
    icon: '⚡',
    mainFile: 'src/main.s',
    files: {
      'src/main.s': `; =============================================================================
; Generic Bare-Metal 68000 Application
; =============================================================================
    ORG $000000

    ; Reset Vector Table
    dc.l    $00010000           ; Initial SP
    dc.l    ResetHandler        ; Initial PC
    dcb.l   62, UnhandledVector

ResetHandler:
    ; Initialize registers
    moveq   #0,d0
    moveq   #0,d1
    lea     Buffer(pc),a0

ComputeLoop:
    addq.l  #1,d0
    move.l  d0,(a0)+
    cmpi.l  #100,d0
    blt.s   ComputeLoop

Halt:
    stop    #$2700
    bra.s   Halt

UnhandledVector:
    rte

    SECTION BSS
Buffer:
    ds.l    100
`,
      'README.md': `# Bare Metal 68000 Binary\n\nUniverseller Maschinencode für Single-Board-Computer und Emulatoren.\n`,
    },
  },
];

export const BUILTIN_FILE_TEMPLATES: ProjectFileTemplate[] = [
  {
    id: 'file_amiga_copper',
    name: 'Amiga Copperlist Routine (.s)',
    extension: '.s',
    language: 'm68k',
    system: 'amiga',
    description: 'Eine modulare Copperlist mit Farbgradienten und Wait-Befehlen.',
    content: `; Amiga Copperlist Block
CopperList:
    dc.w    $0100,$0000         ; BPLCON0: 0 bitplanes
    dc.w    $0180,$0002         ; COLOR00: Dunkelblau
    dc.w    $8001,$FFFE         ; Wait Line 128
    dc.w    $0180,$0F80         ; COLOR00: Orange
    dc.w    $8801,$FFFE         ; Wait Line 136
    dc.w    $0180,$0002         ; COLOR00: Dunkelblau
    dc.w    $FFFF,$FFFE         ; End of Copperlist
`,
  },
  {
    id: 'file_amiga_blitter',
    name: 'Amiga Blitter Copy Routine (.s)',
    extension: '.s',
    language: 'm68k',
    system: 'amiga',
    description: 'Asynchrone Blitter Kopier-Routine mit Blitter-Wait Loop.',
    content: `; Amiga Blitter Copy Helper
; Input: A0 = Source, A1 = Destination, D0 = Width in Words, D1 = Height in Lines
BlitCopy:
    lea     $DFF000,a6
.wait_blit:
    btst    #6,$002(a6)         ; DMACONR BBUSY bit
    bne.s   .wait_blit

    move.w  #$09F0,$040(a6)     ; BLTCON0: Copy A -> D
    move.w  #$0000,$042(a6)     ; BLTCON1: Normal direction
    move.w  #$0000,$064(a6)     ; BLTAMOD: 0 modulo
    move.w  #$0000,$066(a6)     ; BLTDMOD: 0 modulo
    move.l  a0,$050(a6)         ; BLTAPT: Source pointer
    move.l  a1,$054(a6)         ; BLTDPT: Dest pointer
    
    lsl.w   #6,d1
    or.w    d0,d1
    move.w  d1,$058(a6)         ; BLTSIZE: Trigger Blit!
    rts
`,
  },
  {
    id: 'file_python_raster',
    name: 'Python Raster Animation Module (.py)',
    extension: '.py',
    language: 'python',
    system: 'amiga',
    description: 'Python Modul mit Rasterbar-Schleife und Timing.',
    content: `# Amiga Python Raster Module
from m68k.amiga import custom, wait_line

def draw_raster_bars():
    # 8-zeiliger Farbverlauf
    wait_line(120)
    custom.color00 = 0x0113
    wait_line(122)
    custom.color00 = 0x0337
    wait_line(124)
    custom.color00 = 0x055B
    wait_line(126)
    custom.color00 = 0x088F
    wait_line(128)
    custom.color00 = 0x055B
    wait_line(130)
    custom.color00 = 0x0337
    wait_line(132)
    custom.color00 = 0x0113
`,
  },
  {
    id: 'file_megadrive_vdp',
    name: 'Mega Drive VDP Tile Upload (.s)',
    extension: '.s',
    language: 'm68k',
    system: 'megadrive',
    description: 'VDP VRAM Autoincrement & Tile Pattern Upload.',
    content: `; Mega Drive VDP Tile Pattern Upload
; Input: A0 = Tile data pointer, D0 = VRAM Tile Index, D1 = Num Tiles (32 bytes each)
LoadTilesToVRAM:
    lea     $C00004,a1          ; VDP Control Port
    lea     $C00000,a2          ; VDP Data Port
    ; Set VRAM Write Command
    lsl.l   #5,d0               ; Index * 32 bytes
    andi.l  #$FFFF,d0
    lsl.l   #2,d0
    lsr.w   #2,d0
    swap    d0
    ori.l   #$40000000,d0
    move.l  d0,(a1)
.loop:
    move.l  (a0)+,(a2)
    subq.w  #1,d1
    bne.s   .loop
    rts
`,
  },
];

const STORAGE_KEY_CUSTOM_TEMPLATES = 'm68k_studio_custom_templates';

export function loadCustomTemplates(): ProjectTemplate[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY_CUSTOM_TEMPLATES);
    if (!raw) return [];
    return JSON.parse(raw);
  } catch {
    return [];
  }
}

export function saveCustomTemplate(template: ProjectTemplate) {
  const list = loadCustomTemplates().filter((t) => t.id !== template.id);
  list.push({ ...template, isCustom: true });
  localStorage.setItem(STORAGE_KEY_CUSTOM_TEMPLATES, JSON.stringify(list));
}

export function getAllProjectTemplates(): ProjectTemplate[] {
  return [...BUILTIN_PROJECT_TEMPLATES, ...loadCustomTemplates()];
}

export function createProjectFromTemplate(template: ProjectTemplate, projectName: string): StudioProject {
  const files: Record<string, string> = { ...template.files };
  
  // Create project.json inside files
  const proj: StudioProject = {
    id: `proj_${Date.now()}`,
    name: projectName.trim() || template.name,
    version: '1.0.0',
    profileId: template.defaultProfileId,
    mainFile: template.mainFile,
    files,
    created: Date.now(),
    lastModified: Date.now(),
  };

  proj.files['project.json'] = JSON.stringify(
    {
      name: proj.name,
      version: proj.version,
      profile: proj.profileId,
      main_file: proj.mainFile,
      created_at: new Date(proj.created).toISOString(),
    },
    null,
    2
  );

  return proj;
}
