; Atari TOS system calls for m68k-asm: GEMDOS (trap #1), BIOS (trap #13)
; and XBIOS (trap #14) function numbers, and macros that issue them.
;
; Arguments are pushed in reverse order, the function number last; the
; caller removes everything after the trap. The result is in D0.
;
;     include 'atarist/tos.i'
;
;             PEA     text(PC)
;             GEMDOS  Cconws
;             ADDQ.L  #6,SP

        IFND    ATARIST_TOS_I
ATARIST_TOS_I   EQU     1

; --- GEMDOS, trap #1 -------------------------------------------------------

Pterm0          EQU     $00
Cconin          EQU     $01
Cconout         EQU     $02
Cauxin          EQU     $03
Cauxout         EQU     $04
Cprnout         EQU     $05
Crawio          EQU     $06
Crawcin         EQU     $07
Cnecin          EQU     $08
Cconws          EQU     $09
Cconrs          EQU     $0A
Cconis          EQU     $0B
Dsetdrv         EQU     $0E
Cconos          EQU     $10
Cprnos          EQU     $11
Cauxis          EQU     $12
Cauxos          EQU     $13
Dgetdrv         EQU     $19
Fsetdta         EQU     $1A
Super           EQU     $20
Tgetdate        EQU     $2A
Tsetdate        EQU     $2B
Tgettime        EQU     $2C
Tsettime        EQU     $2D
Fgetdta         EQU     $2F
Sversion        EQU     $30
Ptermres        EQU     $31
Dfree           EQU     $36
Dcreate         EQU     $39
Ddelete         EQU     $3A
Dsetpath        EQU     $3B
Fcreate         EQU     $3C
Fopen           EQU     $3D
Fclose          EQU     $3E
Fread           EQU     $3F
Fwrite          EQU     $40
Fdelete         EQU     $41
Fseek           EQU     $42
Fattrib         EQU     $43
Fdup            EQU     $45
Fforce          EQU     $46
Dgetpath        EQU     $47
Malloc          EQU     $48
Mfree           EQU     $49
Mshrink         EQU     $4A
Pexec           EQU     $4B
Pterm           EQU     $4C
Fsfirst         EQU     $4E
Fsnext          EQU     $4F
Frename         EQU     $56
Fdatime         EQU     $57

; --- BIOS, trap #13 --------------------------------------------------------

Getmpb          EQU     $00
Bconstat        EQU     $01
Bconin          EQU     $02
Bconout         EQU     $03
Rwabs           EQU     $04
Setexc          EQU     $05
Tickcal         EQU     $06
Getbpb          EQU     $07
Bcostat         EQU     $08
Mediach         EQU     $09
Drvmap          EQU     $0A
Kbshift         EQU     $0B

; --- XBIOS, trap #14 -------------------------------------------------------

Initmous        EQU     $00
Physbase        EQU     $02
Logbase         EQU     $03
Getrez          EQU     $04
Setscreen       EQU     $05
Setpalette      EQU     $06
Setcolor        EQU     $07
Floprd          EQU     $08
Flopwr          EQU     $09
Flopfmt         EQU     $0A
Midiws          EQU     $0C
Mfpint          EQU     $0D
Iorec           EQU     $0E
Rsconf          EQU     $0F
Keytbl          EQU     $10
Random          EQU     $11
Protobt         EQU     $12
Flopver         EQU     $13
Scrdmp          EQU     $14
Cursconf        EQU     $15
Settime         EQU     $16
Gettime         EQU     $17
Bioskeys        EQU     $18
Ikbdws          EQU     $19
Jdisint         EQU     $1A
Jenabint        EQU     $1B
Giaccess        EQU     $1C
Offgibit        EQU     $1D
Ongibit         EQU     $1E
Xbtimer         EQU     $1F
Dosound         EQU     $20
Setprt          EQU     $21
Kbdvbase        EQU     $22
Kbrate          EQU     $23
Prtblk          EQU     $24
Vsync           EQU     $25
Supexec         EQU     $26
Puntaes         EQU     $27
Floprate        EQU     $29
DMAread         EQU     $2A
DMAwrite        EQU     $2B
Bconmap         EQU     $2C
NVMaccess       EQU     $2E
Blitmode        EQU     $40

; --- Device numbers for Bconstat/Bconin/Bconout/Bcostat -------------------

DEV_PRT         EQU     0
DEV_AUX         EQU     1
DEV_CON         EQU     2
DEV_MIDI        EQU     3
DEV_IKBD        EQU     4
DEV_RAW         EQU     5

; --- Macros ----------------------------------------------------------------

GEMDOS          MACRO   ; function
                MOVE.W  #\1,-(SP)
                TRAP    #1
                ENDM

BIOS            MACRO   ; function
                MOVE.W  #\1,-(SP)
                TRAP    #13
                ENDM

XBIOS           MACRO   ; function
                MOVE.W  #\1,-(SP)
                TRAP    #14
                ENDM

        ENDC    ; ATARIST_TOS_I
