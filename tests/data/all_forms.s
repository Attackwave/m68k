org $8000

; === Data movement ===
move.b d0, d1
move.w d0, d1
move.l d0, d1
move.b d0, (a0)
move.w d0, (a0)
move.l d0, (a0)
move.b (a0)+, d0
move.w -(a0), d1
move.l $1000.w, d0
move.b $00001234, d0

movea.w $1000, a0
movea.l $00001234, a1

move.b #42, d0
move.w #$1234, d1
move.l #$12345678, d2

move.b (a0), (a1)
move.w (a0)+, -(a1)

; === Arithmetic ===
add.b d0, d1
add.w d0, d1
add.l d0, d1
add.b #1, d0
add.w #2, (a0)
add.l #3, (a0)+

sub.b d0, d1
sub.w d0, d1
sub.l d0, d1
sub.b #1, d0
sub.w #2, (a0)

addq.b #1, d0
addq.w #4, (a0)
addq.l #8, d1
subq.b #1, d0
subq.w #4, (a0)

adda.w #100, a0
adda.l #-1, a1
suba.w #100, a0
suba.l #-1, a1

addx.b d0, d1
addx.w -(a0), -(a1)
subx.b d0, d1
subx.w -(a0), -(a1)

; === Logic ===
and.b d0, d1
and.w d0, (a0)
and.l #$FF00, d2
or.b d0, d1
or.w d0, (a0)
eor.b d0, d1
eor.w d0, (a0)

andi.b #7, d0
andi.w #$FF, (a0)
ori.b #1, d0
ori.l #$FFFFFFFF, d1
eori.b #1, d0

; === Comparison ===
cmp.b d0, d1
cmp.w d0, d1
cmp.l d0, d1
cmp.b #0, d0
cmp.w (a0), d1
cmp.l (a0)+, d0

cmpa.w #100, a0
cmpa.l #-1, a1

cmpi.b #42, (a0)
cmpi.w #1000, d0

; === Multiplication / Division ===
muls.w d0, d1
mulu.w d0, d1
divs.w d0, d1
divu.w d0, d1

; === Single-EA ===
clr.b d0
clr.w (a0)
clr.l (a0)+
tst.b d0
tst.w (a0)
tst.l $1000.w
neg.b d0
neg.w (a0)
negx.b d0
negx.w (a0)
not.b d0
not.w (a0)
nbcd d0
nbcd (a0)
tas d0
tas (a0)

jmp $1000.w
jmp (a0)
jsr $1000.w
jsr (a0)
pea (a0)

; === LEA ===
lea (a0), a1
lea 8(a0, d0.w), a1
lea $1000.w, a0
lea (a0, d0.l*2), a1

; === EXG ===
exg d0, d1
exg a0, a1
exg d0, a0

; === EXT / EXTB ===
ext.w d0
ext.l d1
extb.l d0

; === SWAP ===
swap d0

; === CHK ===
chk d0, d1

; === LINK / UNLK ===
link a0, #-4
link a1, #100
unlk a0
unlk a1

; === MOVEQ ===
moveq #0, d0
moveq #1, d1
moveq #-1, d2
moveq #42, d3

; === TRAP ===
trap #0
trap #14

; === Bit manipulation ===
btst d0, d1
btst #3, d1
btst d0, (a0)
btst #7, (a0)
bchg d0, d1
bchg #3, (a0)
bclr d0, d1
bclr #3, (a0)
bset d0, d1
bset #3, (a0)

; === Shifts / Rotates ===
asl.b d0
asl.w d1
asl.l d2
asl.w (a0)
asr.b d0
asr.w (a0)
lsl.w d0
lsl.w (a0)
lsr.w d0
lsr.w (a0)
rol.w d0
rol.w (a0)
ror.w d0
ror.w (a0)
roxl.w d0
roxl.w (a0)
roxr.w d0
roxr.w (a0)

; === ABCD / SBCD ===
abcd d0, d1
abcd -(a0), -(a1)
sbcd d0, d1
sbcd -(a0), -(a1)

; === MOVEP ===
movep.w d0, 0(a1)
movep.l d1, 1(a2)

; === CMPM ===
cmpm.b (a0)+, (a1)+
cmpm.w (a0)+, (a1)+
cmpm.l (a0)+, (a1)+

; === Branches ===
bra.w label
bra.w label
bra label
bra.l label
bsr.w label
bsr label
beq label
bne label
bhi label
bls label
bcc label
bcs label
bge label
blt label
bgt label
ble label

; === DBcc ===
dbra d0, label
dbf d0, label
dbeq d0, label
dbne d0, label

; === Scc ===
st d0
sf (a0)
seq d1
sne (a0)

; === Move SR/CCR/USP ===
move.w ccr, d0
move.w sr, d0
move.w d0, ccr
move.w d0, sr
move.l usp, a0
move.l a0, usp

; === Stop / RTE / RTS / RTR / RESET / NOP / TRAPV / ILLEGAL ===
stop #$2700
rte
rts
rtr
reset
nop
trapv
illegal

; === BKPT ===
bkpt #0

; === MOVEM ===
movem.w d0-d2/a0-a1, (a0)+
movem.l d0-d7/a0-a7, -(sp)
movem.w (a0)+, d0-d2
movem.l -(sp), d0-d7/a0-a7

; === Data directives ===
dc.b $01, $02, $03
dc.w $1234, $5678
dc.l $12345678, $DEADBEEF
dc.b "hello"
even
align 4

; === Labels ===
label:
    nop

; === Addressing modes (68020+) ===
move.b (a0, d0.w), d1
move.w (a0, d0.l), d1
move.l ([a0], d0), d2
move.b ([a0, d0.w], d1.l*4), d3

even
