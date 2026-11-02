; Atari ST / STE / Mega STE hardware map for m68k-asm. Falcon-only
; devices are not covered.
;
;     include 'atarist/hardware.i'

        IFND    ATARIST_HARDWARE_I
ATARIST_HARDWARE_I      EQU     1

; --- Memory map ------------------------------------------------------------

CART_START      EQU     $FA0000         ; cartridge port, 128 KB
CART_END        EQU     $FBFFFF
CART_MAGIC      EQU     $ABCDEF42       ; first long of an application cartridge
TOS_START       EQU     $FC0000         ; 192 KB TOS (TOS 1.0x); $E00000 from TOS 1.06
TOS_START_HIGH  EQU     $E00000

; --- Memory controller and video shifter (byte registers unless noted) -----

MMU_CONFIG      EQU     $FF8001         ; bank sizes
VID_BASE_HIGH   EQU     $FF8201         ; screen base address bits 23-16
VID_BASE_MID    EQU     $FF8203         ; bits 15-8
VID_COUNT_HIGH  EQU     $FF8205         ; read: current address being displayed
VID_COUNT_MID   EQU     $FF8207
VID_COUNT_LOW   EQU     $FF8209
VID_SYNC        EQU     $FF820A         ; bit 1: 0 = 60 Hz, 1 = 50 Hz
VID_BASE_LOW    EQU     $FF820D         ; STE: bits 7-1
VID_LINE_WIDTH  EQU     $FF820F         ; STE: extra words per line
VID_PALETTE     EQU     $FF8240         ; 16 words, $0RGB (STE: 4 bits each)
VID_SHIFT_MODE  EQU     $FF8260         ; 0 = 320x200x16, 1 = 640x200x4, 2 = 640x400x2
VID_HSCROLL_PRE EQU     $FF8264         ; STE: fine scroll, no prefetch
VID_HSCROLL     EQU     $FF8265         ; STE: fine scroll 0-15

; --- Floppy and hard disk DMA ---------------------------------------------

DMA_DATA        EQU     $FF8604         ; word: WD1772 / ACSI data
DMA_MODE        EQU     $FF8606         ; word: write mode / read status
DMA_ADDR_HIGH   EQU     $FF8609
DMA_ADDR_MID    EQU     $FF860B
DMA_ADDR_LOW    EQU     $FF860D

; --- YM2149 sound ----------------------------------------------------------

PSG_SELECT      EQU     $FF8800         ; write: register number; read: register value
PSG_WRITE       EQU     $FF8802         ; write: register value
PSG_PERIOD_A_LO EQU     0
PSG_PERIOD_A_HI EQU     1
PSG_PERIOD_B_LO EQU     2
PSG_PERIOD_B_HI EQU     3
PSG_PERIOD_C_LO EQU     4
PSG_PERIOD_C_HI EQU     5
PSG_NOISE       EQU     6
PSG_MIXER       EQU     7
PSG_VOLUME_A    EQU     8
PSG_VOLUME_B    EQU     9
PSG_VOLUME_C    EQU     10
PSG_ENV_LO      EQU     11
PSG_ENV_HI      EQU     12
PSG_ENV_SHAPE   EQU     13
PSG_PORT_A      EQU     14              ; floppy side and drive select, RS232, printer strobe
PSG_PORT_B      EQU     15              ; printer data

; --- STE DMA sound ---------------------------------------------------------

SND_CTRL        EQU     $FF8901         ; bit 0 play, bit 1 loop
SND_START_HIGH  EQU     $FF8903
SND_START_MID   EQU     $FF8905
SND_START_LOW   EQU     $FF8907
SND_COUNT_HIGH  EQU     $FF8909
SND_COUNT_MID   EQU     $FF890B
SND_COUNT_LOW   EQU     $FF890D
SND_END_HIGH    EQU     $FF890F
SND_END_MID     EQU     $FF8911
SND_END_LOW     EQU     $FF8913
SND_MODE        EQU     $FF8921         ; bit 7 mono, bits 1-0 rate: 0 6258, 1 12517, 2 25033, 3 50066 Hz
SND_MICROWIRE_DATA EQU  $FF8922
SND_MICROWIRE_MASK EQU  $FF8924

; --- Blitter (STE, Mega ST) ------------------------------------------------

BLT_HALFTONE    EQU     $FF8A00         ; 16 words
BLT_SRC_XINC    EQU     $FF8A20
BLT_SRC_YINC    EQU     $FF8A22
BLT_SRC_ADDR    EQU     $FF8A24         ; long
BLT_ENDMASK1    EQU     $FF8A28
BLT_ENDMASK2    EQU     $FF8A2A
BLT_ENDMASK3    EQU     $FF8A2C
BLT_DST_XINC    EQU     $FF8A2E
BLT_DST_YINC    EQU     $FF8A30
BLT_DST_ADDR    EQU     $FF8A32         ; long
BLT_XCOUNT      EQU     $FF8A36
BLT_YCOUNT      EQU     $FF8A38
BLT_HOP         EQU     $FF8A3A         ; byte: halftone operation
BLT_OP          EQU     $FF8A3B         ; byte: logical operation
BLT_CONTROL     EQU     $FF8A3C         ; byte: bit 7 busy, bit 6 hog, bit 5 smudge, bits 3-0 line
BLT_SKEW        EQU     $FF8A3D         ; byte: bit 7 FXSR, bit 6 NFSR, bits 3-0 skew

; --- MFP 68901 (byte registers at odd addresses) --------------------------

MFP_GPIP        EQU     $FFFA01         ; bit 7 monochrome monitor, 5 FDC/HDC, 4 keyboard/MIDI, 0 Centronics busy
MFP_AER         EQU     $FFFA03
MFP_DDR         EQU     $FFFA05
MFP_IERA        EQU     $FFFA07         ; interrupt enable A: 7 mono detect, 5 timer A, 4 RS232 rx, 3 rx error, 2 tx, 1 tx error, 0 timer B
MFP_IERB        EQU     $FFFA09         ; interrupt enable B: 7 FDC/HDC, 6 keyboard/MIDI, 5 timer C, 4 timer D, 0 Centronics
MFP_IPRA        EQU     $FFFA0B
MFP_IPRB        EQU     $FFFA0D
MFP_ISRA        EQU     $FFFA0F
MFP_ISRB        EQU     $FFFA11
MFP_IMRA        EQU     $FFFA13
MFP_IMRB        EQU     $FFFA15
MFP_VR          EQU     $FFFA17         ; vector base, bit 3 software end of interrupt
MFP_TACR        EQU     $FFFA19
MFP_TBCR        EQU     $FFFA1B
MFP_TCDCR       EQU     $FFFA1D
MFP_TADR        EQU     $FFFA1F
MFP_TBDR        EQU     $FFFA21
MFP_TCDR        EQU     $FFFA23
MFP_TDDR        EQU     $FFFA25
MFP_SCR         EQU     $FFFA27
MFP_UCR         EQU     $FFFA29
MFP_RSR         EQU     $FFFA2B
MFP_TSR         EQU     $FFFA2D
MFP_UDR         EQU     $FFFA2F

; --- ACIAs -----------------------------------------------------------------

ACIA_KBD_CTRL   EQU     $FFFC00         ; keyboard (IKBD) control / status
ACIA_KBD_DATA   EQU     $FFFC02
ACIA_MIDI_CTRL  EQU     $FFFC04
ACIA_MIDI_DATA  EQU     $FFFC06

; --- Exception vectors and system variables (supervisor mode) -------------

VEC_HBL         EQU     $68             ; level 2 autovector: horizontal blank
VEC_VBL         EQU     $70             ; level 4 autovector: vertical blank
VEC_MFP         EQU     $100            ; MFP vectors $100-$13C (vector base $40)
VEC_TIMER_B     EQU     $120
VEC_ACIA        EQU     $118
VEC_TIMER_C     EQU     $114
VEC_TIMER_A     EQU     $134

memvalid        EQU     $420
memcntlr        EQU     $424
resvalid        EQU     $426
resvector       EQU     $42A
phystop         EQU     $42E
_membot         EQU     $432
_memtop         EQU     $436
memval2         EQU     $43A
flock           EQU     $43E
seekrate        EQU     $440
_timer_ms       EQU     $442
_fverify        EQU     $444
_bootdev        EQU     $446
palmode         EQU     $448
defshiftmd      EQU     $44A
sshiftmd        EQU     $44C
_v_bas_ad       EQU     $44E            ; screen base address
vblsem          EQU     $452
nvbls           EQU     $454
_vblqueue       EQU     $456
colorptr        EQU     $45A
screenpt        EQU     $45E
_vbclock        EQU     $462
_frclock        EQU     $466
hdv_init        EQU     $46A
swv_vec         EQU     $46E
hdv_bpb         EQU     $472
hdv_rw          EQU     $476
hdv_boot        EQU     $47A
hdv_mediach     EQU     $47E
_cmdload        EQU     $482
conterm         EQU     $484
savptr          EQU     $4A2
_nflops         EQU     $4A6
con_state       EQU     $4A8
save_row        EQU     $4AC
sav_context     EQU     $4AE
_bufl           EQU     $4B2
_hz_200         EQU     $4BA            ; 200 Hz system timer
the_env         EQU     $4BE
_drvbits        EQU     $4C2
_dskbufp        EQU     $4C6
_autopath       EQU     $4CA
_vbl_list       EQU     $4CE
_dumpflg        EQU     $4EE
_prtabt         EQU     $4F0
_sysbase        EQU     $4F2            ; OS header
_shell_p        EQU     $4F6
end_os          EQU     $4FA
exec_os         EQU     $4FE
scr_dump        EQU     $502
prv_lsto        EQU     $506
prv_lst         EQU     $50A
prv_auxo        EQU     $50E
prv_aux         EQU     $512
pun_ptr         EQU     $516
memval3         EQU     $51A
xconstat        EQU     $51E
xconin          EQU     $53E
xcostat         EQU     $55E
xconout         EQU     $57E
_longframe      EQU     $59E            ; non-zero: 68010+ exception frames
_p_cookies      EQU     $5A0            ; cookie jar
ramtop          EQU     $5A4
ramvalid        EQU     $5A8
bell_hook       EQU     $5AC
kcl_hook        EQU     $5B0

        ENDC    ; ATARIST_HARDWARE_I
