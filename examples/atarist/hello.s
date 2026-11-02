; Atari ST program: print a line on the console, wait for a key, exit.
; Written position-independent, as a TOS program is loaded at whatever
; address GEMDOS finds free.
;
;     m68k-asm -I include -f binary -o hello.bin examples/atarist/hello.s

        include 'atarist/tos.i'
        include 'atarist/hardware.i'

start:
        PEA     text(PC)
        GEMDOS  Cconws
        ADDQ.L  #6,SP

        GEMDOS  Cnecin                  ; wait for a key, no echo
        ADDQ.L  #2,SP

        MOVE.W  #Vsync,-(SP)            ; one frame, so the screen settles
        TRAP    #14
        ADDQ.L  #2,SP

        CLR.W   -(SP)                   ; Pterm0
        TRAP    #1

text:   DC.B    'Hello from m68k-asm',13,10,0
        EVEN
