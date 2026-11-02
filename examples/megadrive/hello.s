; Minimal Mega Drive cartridge: a vector table, a header, TMSS handshake,
; VDP set-up and a frame counter. Assemble with
;
;     m68k-asm -I include -f binary --pad pow2 --checksum megadrive \
;              -o hello.bin examples/megadrive/hello.s

        include 'megadrive/hardware.i'

        ORG     ROM_START

; --- Vector table ----------------------------------------------------------

        DC.L    WORK_RAM_END+1          ; initial stack pointer
        DC.L    reset                   ; initial program counter
        DC.L    error,error,error,error ; bus error, address error, illegal, zero divide
        DC.L    error,error,error,error ; CHK, TRAPV, privilege, trace
        DC.L    error,error,error,error ; line A, line F, 14-15 reserved
        DC.L    error,error,error,error ; 16-23 reserved
        DC.L    error,error,error,error
        DC.L    error,error,error,error ; 22-23 reserved, 24 spurious, 25 level 1
        DC.L    hblank                  ; level 2 (external)
        DC.L    error
        DC.L    hblank                  ; level 4: horizontal interrupt
        DC.L    error
        DC.L    vblank                  ; level 6: vertical interrupt
        DC.L    error                   ; level 7
        REPT    32
        DC.L    error                   ; TRAP #0-15 and reserved
        ENDM

; --- Header ----------------------------------------------------------------

        DC.B    'SEGA MEGA DRIVE '
        DC.B    '(C)M68K 2026.NOV'
        DC.B    'HELLO                                           '
        DC.B    'HELLO                                           '
        DC.B    'GM 00000000-00'
        DC.W    0                       ; checksum, written by --checksum megadrive
        DC.B    'J               '
        DC.L    ROM_START
        DC.L    rom_end-1
        DC.L    WORK_RAM
        DC.L    WORK_RAM_END
        DC.B    '            '          ; no SRAM
        DC.B    '            '          ; no modem
        DC.B    '                                        '
        DC.B    'JUE             '

; --- Program ---------------------------------------------------------------

frames  EQU     WORK_RAM                ; one long word of RAM

reset:
        MOVE    #$2700,SR               ; interrupts off
        MOVE.B  IO_VERSION,D0
        ANDI.B  #$0F,D0
        BEQ.S   .no_tmss
        MOVE.L  #'SEGA',TMSS
.no_tmss:
        LEA     vdp_regs(PC),A0
        MOVEQ   #(vdp_regs_end-vdp_regs)/2-1,D0
.init_vdp:
        MOVE.W  (A0)+,VDP_CTRL
        DBF     D0,.init_vdp

        VDP_SETADDR VDP_CRAM_WRITE,0    ; background colour
        MOVE.W  #$0E00,VDP_DATA         ; blue

        CLR.L   frames
        MOVE    #$2300,SR               ; vertical interrupt on
.idle:
        STOP    #$2300
        BRA.S   .idle

vblank:
        ADDQ.L  #1,frames
        RTE

hblank:
        RTE

error:
        STOP    #$2700
        BRA.S   error

vdp_regs:
        DC.W    $8000|(VDPREG_MODE1<<8)|$04
        DC.W    $8000|(VDPREG_MODE2<<8)|$64     ; display on, V interrupt, DMA
        DC.W    $8000|(VDPREG_PLANEA<<8)|($C000>>10)
        DC.W    $8000|(VDPREG_WINDOW<<8)|($D000>>10)
        DC.W    $8000|(VDPREG_PLANEB<<8)|($E000>>13)
        DC.W    $8000|(VDPREG_SPRITE<<8)|($F000>>9)
        DC.W    $8000|(VDPREG_BGCOLOR<<8)|$00
        DC.W    $8000|(VDPREG_HINT<<8)|$FF
        DC.W    $8000|(VDPREG_MODE3<<8)|$00
        DC.W    $8000|(VDPREG_MODE4<<8)|$81     ; H40
        DC.W    $8000|(VDPREG_HSCROLL<<8)|($FC00>>10)
        DC.W    $8000|(VDPREG_AUTOINC<<8)|$02
        DC.W    $8000|(VDPREG_PLANESIZE<<8)|$01 ; 64x32
vdp_regs_end:

rom_end:
