; SNK Neo Geo (MVS and AES) hardware map and BIOS interface for m68k-asm.
;
; Addresses as seen from the 68000. The Z80 sound program is not covered.
;
;     include 'neogeo/hardware.i'

        IFND    NEOGEO_HARDWARE_I
NEOGEO_HARDWARE_I       EQU     1

; --- Memory map ------------------------------------------------------------

PROM_START      EQU     $000000         ; cartridge program ROM, first 1 MB
PROM_END        EQU     $0FFFFF
PROM_BANK       EQU     $200000         ; banked program ROM, 1 MB window
PROM_BANK_END   EQU     $2FFFFF
WORK_RAM        EQU     $100000         ; 64 KB
WORK_RAM_END    EQU     $10FFFF
BIOS_RAM        EQU     $10F300         ; top of work RAM, owned by the BIOS
PALETTE_RAM     EQU     $400000         ; 8 KB, two banks of 256 x 16 colours
PALETTE_RAM_END EQU     $401FFF
MEMCARD         EQU     $800000         ; memory card, odd bytes
SYSTEM_ROM      EQU     $C00000         ; BIOS, 128 KB
BACKUP_RAM      EQU     $D00000         ; MVS battery-backed RAM, odd bytes

; --- Cartridge header (offsets from the start of the program ROM) -------

HEADER_MAGIC    EQU     $100            ; 'NEO-GEO'
HEADER_NGH      EQU     $108            ; word, game number
HEADER_PROM_SIZE EQU    $10A            ; long, program size in bytes
HEADER_BACKUP   EQU     $10E            ; long: backup RAM pointer (word) and size
HEADER_EYECATCH EQU     $112            ; byte: 0 = BIOS plays it, 1 = game, 2 = none
HEADER_EYECATCH_TILES EQU $113          ; byte: upper byte of the eyecatcher tile bank
HEADER_NAME_JP  EQU     $114            ; long, pointer to the Japanese title
HEADER_NAME_US  EQU     $118            ; long, pointer to the overseas title
HEADER_NAME_EU  EQU     $11C            ; long, pointer to the European title
HEADER_USER     EQU     $122            ; JMP to the USER routine
HEADER_PLAYER_START EQU $128            ; JMP to PLAYER_START
HEADER_DEMO_END EQU     $12E            ; JMP to DEMO_END
HEADER_COIN_SOUND EQU   $134            ; JMP to COIN_SOUND
HEADER_SECURITY EQU     $182            ; long, pointer to the security code
HEADER_END      EQU     $200

; --- System registers (REG_* are the names SNK's documentation uses) -----

REG_P1CNT       EQU     $300000         ; read: player 1 joystick and buttons
REG_DIPSW       EQU     $300001         ; read: hardware DIP switches; write: watchdog kick
REG_SYSTYPE     EQU     $300081         ; read: bit 6 = 4-slot or 6-slot board
REG_SOUND       EQU     $320000         ; write: command to the Z80; read: its reply
REG_STATUS_A    EQU     $320001         ; read: coins, service, calendar
REG_P2CNT       EQU     $340000         ; read: player 2
REG_STATUS_B    EQU     $380000         ; read: start, select, memory card, MVS/AES
REG_POUTPUT     EQU     $380001         ; write: joystick port outputs
REG_CRDBANK     EQU     $380011         ; write: memory card bank
REG_SLOT        EQU     $380021         ; write: cartridge slot select
REG_LEDLATCHES  EQU     $380031
REG_LEDDATA     EQU     $380041
REG_RTCCTRL     EQU     $380051         ; write: calendar chip control
REG_RESETCC1    EQU     $380061         ; write: reset coin counter 1
REG_RESETCC2    EQU     $380063
REG_RESETCL1    EQU     $380065         ; write: reset coin lockout 1
REG_RESETCL2    EQU     $380067
REG_SETCC1      EQU     $3800E1         ; write: set coin counter 1
REG_SETCC2      EQU     $3800E3
REG_SETCL1      EQU     $3800E5
REG_SETCL2      EQU     $3800E7

REG_NOSHADOW    EQU     $3A0001         ; write: normal video output
REG_SHADOW      EQU     $3A0011         ; write: darkened video output
REG_SWPBIOS     EQU     $3A0003         ; write: vectors from the BIOS
REG_SWPROM      EQU     $3A0013         ; write: vectors from the cartridge
REG_CRDUNLOCK1  EQU     $3A0005
REG_CRDLOCK1    EQU     $3A0015
REG_CRDLOCK2    EQU     $3A0007
REG_CRDUNLOCK2  EQU     $3A0017
REG_CRDREGSEL   EQU     $3A0009
REG_CRDNORMAL   EQU     $3A0019
REG_BRDFIX      EQU     $3A000B         ; write: fix layer from the BIOS ROM
REG_CRTFIX      EQU     $3A001B         ; write: fix layer from the cartridge
REG_SRAMLOCK    EQU     $3A000D
REG_SRAMUNLOCK  EQU     $3A001D
REG_PALBANK1    EQU     $3A000F         ; write: palette bank 1
REG_PALBANK0    EQU     $3A001F         ; write: palette bank 0

; --- Video (LSPC) ----------------------------------------------------------

REG_VRAMADDR    EQU     $3C0000         ; word: VRAM address
REG_VRAMRW      EQU     $3C0002         ; word: VRAM data
REG_VRAMMOD     EQU     $3C0004         ; word: address increment after a write
REG_LSPCMODE    EQU     $3C0006         ; write: auto animation, timer; read: raster line
REG_TIMERHIGH   EQU     $3C0008
REG_TIMERLOW    EQU     $3C000A
REG_IRQACK      EQU     $3C000C         ; write: acknowledge interrupts
REG_TIMERSTOP   EQU     $3C000E

VRAM_SCB1       EQU     $0000           ; sprite tile maps, 64 words per sprite
VRAM_FIX        EQU     $7000           ; fix layer, 40 x 32 tiles
VRAM_SCB2       EQU     $8000           ; sprite shrink
VRAM_SCB3       EQU     $8200           ; sprite y, height, sticky bit
VRAM_SCB4       EQU     $8400           ; sprite x

; --- BIOS calls (JSR to these addresses in the system ROM) -----------------

SYSTEM_INT1     EQU     $C00438         ; vertical blank, from the game's handler
SYSTEM_INT2     EQU     $C0043E         ; timer
SYSTEM_RETURN   EQU     $C00444         ; return from USER to the BIOS
SYSTEM_IO       EQU     $C0044A         ; read inputs into BIOS_P1*, BIOS_P2*, BIOS_STAT*
CREDIT_CHECK    EQU     $C00450
CREDIT_DOWN     EQU     $C00456
READ_CALENDAR   EQU     $C0045C
SETUP_CALENDAR  EQU     $C00462
CARD            EQU     $C00468
CARD_ERROR      EQU     $C0046E
HOW_TO_PLAY     EQU     $C00474
CHECKSUM        EQU     $C0047A
FIX_CLEAR       EQU     $C004C2         ; clear the fix layer
LSP_1ST         EQU     $C004C8         ; clear the sprites
MESS_OUT        EQU     $C004CE         ; run the BIOS message queue
CONTROLLER_SETUP EQU    $C004D4

; --- BIOS RAM ---------------------------------------------------------------

BIOS_SYSTEM_MODE EQU    $10FD80         ; $80 = game, 0 = BIOS
BIOS_MVS_FLAG   EQU     $10FD82         ; 1 = MVS, 0 = AES
BIOS_COUNTRY_CODE EQU   $10FD83         ; 0 Japan, 1 USA, 2 Europe
BIOS_GAME_DIP   EQU     $10FD84         ; 16 bytes of soft DIP settings
BIOS_P1STATUS   EQU     $10FD94
BIOS_P1PREVIOUS EQU     $10FD95
BIOS_P1CURRENT  EQU     $10FD96
BIOS_P1CHANGE   EQU     $10FD97
BIOS_P1REPEAT   EQU     $10FD98
BIOS_P1TIMER    EQU     $10FD99
BIOS_P2STATUS   EQU     $10FD9A
BIOS_P2PREVIOUS EQU     $10FD9B
BIOS_P2CURRENT  EQU     $10FD9C
BIOS_P2CHANGE   EQU     $10FD9D
BIOS_P2REPEAT   EQU     $10FD9E
BIOS_P2TIMER    EQU     $10FD9F
BIOS_STATCURNT  EQU     $10FDAC         ; start and select buttons
BIOS_STATCHANGE EQU     $10FDAD
BIOS_USER_REQUEST EQU   $10FDAE         ; what USER is asked to do, see USER_*
BIOS_USER_MODE  EQU     $10FDAF         ; what the game is doing, 0 init, 1 title, 2 game
BIOS_CREDIT_DEC1 EQU    $10FDB0
BIOS_CREDIT_DEC2 EQU    $10FDB1
BIOS_START_FLAG EQU     $10FDB4
BIOS_PLAYER_MOD1 EQU    $10FDB6
BIOS_PLAYER_MOD2 EQU    $10FDB7
BIOS_MESS_POINT EQU     $10FDBE
BIOS_MESS_BUSY  EQU     $10FDC2
BIOS_CARD_COMMAND EQU   $10FDC4
BIOS_CARD_MODE  EQU     $10FDC5
BIOS_CARD_ANSWER EQU    $10FDC6
BIOS_CARD_START EQU     $10FDC8
BIOS_CARD_SIZE  EQU     $10FDCC
BIOS_CARD_FCB   EQU     $10FDCE
BIOS_CARD_SUB   EQU     $10FDD0
BIOS_YEAR       EQU     $10FDD2
BIOS_MONTH      EQU     $10FDD3
BIOS_DAY        EQU     $10FDD4
BIOS_WEEKDAY    EQU     $10FDD5
BIOS_HOUR       EQU     $10FDD6
BIOS_MINUTE     EQU     $10FDD7
BIOS_SECOND     EQU     $10FDD8
BIOS_SELECT_TIMER EQU   $10FDDA
BIOS_DEVMODE    EQU     $10FE80

; Values of BIOS_USER_REQUEST.
USER_INIT       EQU     0               ; initialise the game's RAM
USER_EYECATCHER EQU     1
USER_DEMO       EQU     2               ; title or attract mode
USER_TITLE      EQU     3

; Controller bits in BIOS_P1CURRENT and friends.
JOY_UP          EQU     0
JOY_DOWN        EQU     1
JOY_LEFT        EQU     2
JOY_RIGHT       EQU     3
JOY_A           EQU     4
JOY_B           EQU     5
JOY_C           EQU     6
JOY_D           EQU     7

; --- Macros ----------------------------------------------------------------

; Kick the watchdog; a frame without it resets the system.
WATCHDOG        MACRO
                MOVE.B  D0,REG_DIPSW
                ENDM

; Write `value` to VRAM at `addr` with the increment left as it is.
VRAM_WRITE      MACRO   ; addr, value
                MOVE.W  #\1,REG_VRAMADDR
                MOVE.W  #\2,REG_VRAMRW
                ENDM

        ENDC    ; NEOGEO_HARDWARE_I
