| Forms GCC rarely writes but GNU as reads: index registers without a
| size (long), displacements that cancel only partly, numeric PC
| displacements behind a leading extension word, joined strings and
| logical right shifts.
	.text
	.globl	start
start:
	move.l %d0,(%a0,%d1)
	move.l %d0,%a0@(2,%d1)
	jmp (2,%pc,%d0)
	move.w (%a0,%a1*2),%d0
	move.l %d0,(%d1)
	move.l %d0,4(%d1)
	.long 0
a:	nop
b:
	.long 0
c:	nop
	move.l (a-b+c-b)(%a0),%d0
	mulu.l 8(%pc),%d1
	divs.l -20(%pc),%d1
	fmove.l 8(%pc),%fp0
	fadd.x 8(%pc),%fp0
	bfextu 8(%pc){#1:#2},%d0
	chk2.l 8(%pc),%d0
	cmp2.l 8(%pc),%d0
	mulu.l (8,%pc,%d1.l),%d1
	mulu.l (-120,%pc,%d1.l),%d1
	mulu.l (127,%pc,%d1.l),%d1
	jmp (2,%pc,%d0.w)
	lea -6(%pc),%a0
	move.l 100(%pc),%d0
	movem.l #0xffff,-(%sp)
	fmovem.x #0xff,-(%sp)
	rts
	.data
	.ascii "ab" "cd"
	.ascii "x\"y"
	.long -1>>40, 1<<63>>63
