| Branches and PC-relative references at and beyond the 16-bit reach,
| where GNU as on a 68000 falls back to absolute jumps and addresses.
	.text
	.globl	start
start:
	jra	.Lfar
	jbsr	.Lfar
	jeq	.Lfar
	dbra	%d0,.Lfar
	lea	.Lfar,%a0
	move.l	.Lfar,%d0
	jra	.Ledge
	lea	.Ledge,%a0
	jne	.Ledge2
	jbsr	.Lnear
.Lnear:
	nop
	.zero	32750
.Ledge:
	nop
.Ledge2:
	nop
	.zero	40
.Lfar:
	rts
	jra	start
	jne	start
	jbsr	start
	lea	start,%a0
	jlt	.Lfar
	dbeq	%d1,.Lfar
