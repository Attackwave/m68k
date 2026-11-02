; Minimal Neo Geo cartridge program: the vector table, the header the
; BIOS reads, a USER routine answering the BIOS requests, and a game loop
; that polls the controller. The security code the BIOS checks at the
; address in HEADER_SECURITY is not included; put the 182 bytes at
; `security` for a cartridge that runs on hardware.
;
;     m68k-asm -I include -f binary --pad $100000 -o hello.p1 examples/neogeo/hello.s

        include 'neogeo/hardware.i'

        ORG     PROM_START

; --- Vector table: the BIOS supplies its own for most exceptions ---------

        DC.L    $10F300                 ; initial stack pointer
        DC.L    SYSTEM_ROM+$402         ; initial program counter: the BIOS
        REPT    22
        DC.L    SYSTEM_ROM+$408         ; exceptions 2-23 go to the BIOS
        ENDM
        DC.L    SYSTEM_ROM+$408         ; 24 spurious
        DC.L    vblank                  ; 25 level 1: vertical blank
        DC.L    timer                   ; 26 level 2: timer
        REPT    37
        DC.L    SYSTEM_ROM+$408
        ENDM

; --- Header ----------------------------------------------------------------

        DC.B    'NEO-GEO',0
        DC.W    $0001                   ; NGH number
        DC.L    prom_end                ; program size
        DC.W    $0000                   ; backup RAM pointer
        DC.W    $0000                   ; backup RAM size
        DC.B    $02                     ; no eyecatcher
        DC.B    $00
        DC.L    title_jp
        DC.L    title_us
        DC.L    title_eu
        DC.W    $0000
        JMP     user
        JMP     player_start
        JMP     demo_end
        JMP     coin_sound
        DCB.B   HEADER_SECURITY-HEADER_DEMO_END-6*2,0
        DC.L    security
        DCB.B   HEADER_END-HEADER_SECURITY-4,0

title_jp:
        DC.B    'HELLO           ',0
title_us:
        DC.B    'HELLO           ',0
title_eu:
        DC.B    'HELLO           ',0
        EVEN

; --- USER: the BIOS calls this with a request in BIOS_USER_REQUEST -------

user:
        MOVE.B  D0,REG_DIPSW            ; watchdog
        LEA     $10F300,SP
        MOVE.B  BIOS_USER_REQUEST,D0
        ANDI.W  #3,D0
        ADD.W   D0,D0
        MOVE.W  user_table(PC,D0.W),D0
        JMP     user_table(PC,D0.W)
user_table:
        DC.W    user_init-user_table
        DC.W    user_eyecatcher-user_table
        DC.W    user_demo-user_table
        DC.W    user_title-user_table

user_init:
        MOVE.B  #1,BIOS_USER_MODE
        JMP     SYSTEM_RETURN
user_eyecatcher:
        JMP     SYSTEM_RETURN
user_demo:
user_title:
        MOVE.B  #2,BIOS_USER_MODE
        JSR     FIX_CLEAR
        JSR     LSP_1ST
        MOVE.W  #7,REG_IRQACK
        MOVE    #$2000,SR
.loop:
        WATCHDOG
        JSR     SYSTEM_IO
        BTST    #JOY_A,BIOS_P1CURRENT
        BEQ.S   .loop
        MOVE.B  #1,BIOS_USER_REQUEST
        JMP     SYSTEM_RETURN

player_start:
        RTS
demo_end:
        RTS
coin_sound:
        RTS

vblank:
        MOVE.W  #4,REG_IRQACK
        MOVE.B  D0,REG_DIPSW
        JSR     SYSTEM_INT1
        RTE
timer:
        MOVE.W  #2,REG_IRQACK
        RTE

security:
        DCB.B   182,0                   ; the BIOS security code goes here

prom_end:
