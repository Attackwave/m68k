; Amiga program: print a line through dos.library and return to the CLI.
;
;     m68k-asm -f hunk-exe -o hello examples/amiga/hello.s
;
; The library vector offsets are written out because the Amiga includes
; are not part of this repository. Exec's base is always at address 4.

_LVOOpenLibrary  equ -552
_LVOCloseLibrary equ -414
_LVOOutput       equ -60
_LVOWrite        equ -48

start:
        movea.l 4.w,a6                  ; exec.library
        lea     dosname(pc),a1
        moveq   #0,d0                   ; any version
        jsr     _LVOOpenLibrary(a6)
        tst.l   d0
        beq.s   fail
        movea.l d0,a6                   ; dos.library

        jsr     _LVOOutput(a6)
        move.l  d0,d1                   ; file handle
        move.l  #text,d2                ; absolute: relocated by LoadSeg()
        moveq   #textend-text,d3
        jsr     _LVOWrite(a6)

        movea.l a6,a1
        movea.l 4.w,a6
        jsr     _LVOCloseLibrary(a6)
        moveq   #0,d0                   ; return code
        rts
fail:
        moveq   #20,d0                  ; RETURN_FAIL
        rts

dosname: dc.b    'dos.library',0
        even
text:   dc.b    'Hello from m68k-asm',10
textend:
