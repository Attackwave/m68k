| 68020 code as GCC emits it with -m68020: 32-bit branches, full-format
| indices, 64-bit multiply and divide, bitfields.
	.text
	.globl	start
start:
	jbsr	ext
	jra	ext
	jeq	ext
	dbra	%d0,ext
	bra.l	ext
	jra	.Lfar
	jbsr	.Lfar
	jne	.Lfar
	dbra	%d1,.Lfar
	jra	.Lnear
	nop
.Lnear:
	lea	table(%d0.l*4),%a0
	move.l	table+8(%d1.l),%d0
	lea	(%d0.l*4),%a0
	lea	4(%d0.l*2),%a0
	lea	40000(%d0.l*8),%a0
	lea	(%a1.l*4),%a0
	lea	1518500249(%a2,%d6.l),%a2
	lea	100(%a2,%d6.l),%a2
	lea	1000(%a2,%d6.l),%a2
	lea	100000(%a2,%a3.w*2),%a2
	move.l	100000(%a0),%d0
	move.l	1000(%a0),%d0
	move.l	0(%a0,%d0.l*2),%d0
	muls.l	%d1,%d2:%d3
	mulu.l	(%a0),%d4:%d5
	muls.l	%d1,%d2
	mulu.l	table,%d2
	divsl.l	%d1,%d2:%d3
	divul.l	%d1,%d2:%d3
	divs.l	%d1,%d2:%d3
	divs.l	%d1,%d3
	bfextu	%d0{#3:#5},%d1
	bfexts	%d0{%d2:#8},%d1
	bftst	table+4{#1:#2}
	bfins	%d1,4(%a0){%d2:%d3}
	bfffo	(%a0){#0:#32},%d0
	extb.l	%d0
	link.l	%fp,#-40000
	unlk	%fp
	.zero	32760
.Lfar:
	rts
	.data
	.align	2
table:
	.long	start, ext, 0, 0
