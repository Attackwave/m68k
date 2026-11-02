; Sega Mega Drive / Genesis hardware map for m68k-asm.
;
; Addresses as seen from the 68000. The Z80 side (sound) is not covered;
; the Z80 program is assembled elsewhere and brought in with INCBIN.
;
;     include 'megadrive/hardware.i'

        IFND    MEGADRIVE_HARDWARE_I
MEGADRIVE_HARDWARE_I    EQU     1

; --- Memory map ------------------------------------------------------------

ROM_START       EQU     $000000         ; cartridge, up to 4 MB
ROM_END         EQU     $3FFFFF
Z80_RAM         EQU     $A00000         ; 8 KB, visible while the bus is held
Z80_RAM_END     EQU     $A01FFF
WORK_RAM        EQU     $FF0000         ; 64 KB
WORK_RAM_END    EQU     $FFFFFF

; --- Cartridge header (offsets from the start of the ROM) ----------------
; The console reads CHECKSUM at start-up: the word sum of the ROM from
; HEADER_END to its end (`m68k-asm --checksum megadrive` writes it).

HEADER          EQU     $100            ; 'SEGA MEGA DRIVE ' or 'SEGA GENESIS    '
HEADER_COPYRIGHT EQU    $110            ; '(C)XXXX YYYY.MMM'
HEADER_NAME_JP  EQU     $120            ; 48 characters, space padded
HEADER_NAME_INT EQU     $150            ; 48 characters, space padded
HEADER_SERIAL   EQU     $180            ; 'GM 00000000-00'
HEADER_CHECKSUM EQU     $18E
HEADER_DEVICES  EQU     $190            ; 16 characters, 'J' = 3-button pad
HEADER_ROM_START EQU    $1A0
HEADER_ROM_END  EQU     $1A4
HEADER_RAM_START EQU    $1A8
HEADER_RAM_END  EQU     $1AC
HEADER_SRAM     EQU     $1B0            ; 'RA', type, start, end; spaces if none
HEADER_MODEM    EQU     $1BC
HEADER_REGION   EQU     $1F0            ; 'JUE' or any subset
HEADER_END      EQU     $200

; --- VDP (315-5313) --------------------------------------------------------

VDP_DATA        EQU     $C00000         ; word; also mirrored at $C00002
VDP_CTRL        EQU     $C00004         ; word; also mirrored at $C00006
VDP_HVCOUNTER   EQU     $C00008         ; word, read only
PSG             EQU     $C00011         ; byte, SN76489 write port
VDP_DEBUG       EQU     $C0001C

; VDP register numbers. Write `$8000 | (reg << 8) | value` to VDP_CTRL.
VDPREG_MODE1    EQU     0               ; H interrupt enable, HV counter latch
VDPREG_MODE2    EQU     1               ; display enable, V interrupt, DMA, V30
VDPREG_PLANEA   EQU     2               ; plane A name table (addr >> 10)
VDPREG_WINDOW   EQU     3               ; window name table (addr >> 10)
VDPREG_PLANEB   EQU     4               ; plane B name table (addr >> 13)
VDPREG_SPRITE   EQU     5               ; sprite attribute table (addr >> 9)
VDPREG_BGCOLOR  EQU     7               ; palette line and index
VDPREG_HINT     EQU     10              ; H interrupt line counter
VDPREG_MODE3    EQU     11              ; external interrupt, scroll modes
VDPREG_MODE4    EQU     12              ; H40, shadow/highlight, interlace
VDPREG_HSCROLL  EQU     13              ; H scroll table (addr >> 10)
VDPREG_AUTOINC  EQU     15              ; data port auto increment
VDPREG_PLANESIZE EQU    16
VDPREG_WINX     EQU     17
VDPREG_WINY     EQU     18
VDPREG_DMALEN_L EQU     19
VDPREG_DMALEN_H EQU     20
VDPREG_DMASRC_L EQU     21
VDPREG_DMASRC_M EQU     22
VDPREG_DMASRC_H EQU     23              ; bits 6-7: DMA mode

; VDP_CTRL access codes for the long-word address form (see VDP_ADDR).
VDP_VRAM_READ   EQU     $00000000
VDP_VRAM_WRITE  EQU     $40000000
VDP_CRAM_READ   EQU     $00000020
VDP_CRAM_WRITE  EQU     $C0000000
VDP_VSRAM_READ  EQU     $00000010
VDP_VSRAM_WRITE EQU     $40000010
VDP_DMA         EQU     $00000080       ; OR into a write code to start DMA

; Status word read from VDP_CTRL.
VDPSTAT_PAL     EQU     0
VDPSTAT_DMA     EQU     1
VDPSTAT_HBLANK  EQU     2
VDPSTAT_VBLANK  EQU     3
VDPSTAT_ODDFRAME EQU    4
VDPSTAT_COLLISION EQU   5
VDPSTAT_OVERFLOW EQU    6
VDPSTAT_VINT    EQU     7
VDPSTAT_FULL    EQU     8
VDPSTAT_EMPTY   EQU     9

; --- Z80 bus and YM2612 ----------------------------------------------------

Z80_BUSREQ      EQU     $A11100         ; word: $100 requests the bus, 0 releases it
Z80_RESET       EQU     $A11200         ; word: 0 holds the Z80 in reset, $100 runs it
YM2612_A0       EQU     $A04000         ; register select, part 1
YM2612_D0       EQU     $A04001         ; data, part 1
YM2612_A1       EQU     $A04002         ; register select, part 2
YM2612_D1       EQU     $A04003         ; data, part 2

; --- I/O ports (bytes at odd addresses) ------------------------------------

IO_VERSION      EQU     $A10001         ; region, PAL/NTSC, TMSS version
IO_DATA1        EQU     $A10003
IO_DATA2        EQU     $A10005
IO_DATA3        EQU     $A10007         ; expansion port
IO_CTRL1        EQU     $A10009
IO_CTRL2        EQU     $A1000B
IO_CTRL3        EQU     $A1000D
IO_TXDATA1      EQU     $A1000F
IO_RXDATA1      EQU     $A10011
IO_SCTRL1       EQU     $A10013
IO_TXDATA2      EQU     $A10015
IO_RXDATA2      EQU     $A10017
IO_SCTRL2       EQU     $A10019
IO_TXDATA3      EQU     $A1001B
IO_RXDATA3      EQU     $A1001D
IO_SCTRL3       EQU     $A1001F

; --- System ----------------------------------------------------------------

MEMORY_MODE     EQU     $A11000
TMSS            EQU     $A14000         ; write 'SEGA' here when IO_VERSION & $F != 0
TMSS_CART       EQU     $A14101         ; 0 = boot ROM, 1 = cartridge
SRAM_CTRL       EQU     $A130F1         ; Sega mapper: bit 0 enables SRAM, bit 1 write protects
BANK_REG        EQU     $A130F3         ; Sega mapper: bank registers at $A130F3,F5,...,FF

; --- Macros ----------------------------------------------------------------

; The VDP_CTRL long word that selects `addr` for access `code`
; (VDP_VRAM_WRITE and friends): bits 13-0 of the address go to the high
; word, bits 15-14 to the low word.
VDP_ADDR        MACRO   ; code, addr
                DC.L    (\1)|(((\2)&$3FFF)<<16)|(((\2)&$C000)>>14)
                ENDM

; Write one VDP register.
VDP_SETREG      MACRO   ; reg, value
                MOVE.W  #$8000|((\1)<<8)|((\2)&$FF),VDP_CTRL
                ENDM

; Point the VDP data port at `addr` for access `code`.
VDP_SETADDR     MACRO   ; code, addr
                MOVE.L  #(\1)|(((\2)&$3FFF)<<16)|(((\2)&$C000)>>14),VDP_CTRL
                ENDM

        ENDC    ; MEGADRIVE_HARDWARE_I
