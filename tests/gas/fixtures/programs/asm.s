| Hand-written GNU as syntax: the forms GCC output and inline assembly
| use beyond what one.s and two.s contain. Linked after them.
#NO_APP
	.text
	.globl	__mulsi3
	.globl	__umodsi3
	.globl	__divsi3
	.globl	entry
entry:
	jbsr	one			| global in another file: jsr absolute
	jbsr	local_fn		| local: bsr.s
	jsr	local_fn		| local: (d16,%pc)
	jra	.Lnear
	nop
.Lnear:
	jeq	two			| global: inverted branch around jmp
	jne	.Lnear
	bra	__mulsi3		| global, explicit bra: word, resolved by the link
	dbra	%d0,.Lnear
	dbra	%d1,two			| global: dbra over bra.s over jmp
	move.l	local_data,%d0		| PC-relative source
	cmp.l	local_data+4,%d1
	move.l	%d0,local_data		| absolute destination
	lea	(local_data),%a0
	pea	6.w
	pea	-1.w
	move.l	4,%a6
	lea	__divsi3,%a6
	jsr	(%a6)
	move.l	#-129,%d0
	move.l	#-128,%d0
	move.l	#'A,%d1
	add.l	#8,%sp
	add.l	#9,%d0
	add.w	#500,%a0
	sub.l	#1,%a1
	and.b	#-64,%d0
	cmp.b	#-1,(%a0)
	or.w	#0x8000,%d2
	cmp.l	#5,%a0
	cmp.b	(%a0)+,(%a1)+
	movem.l	%d2-%d4/%a2,-(%sp)
	movem.l	(%sp)+,%d2-%d4/%a2
	movem.l	#0x2030,-(%sp)
	movem.l	(%sp)+,#0x0c04
	move.l	%a0@(4,%d0:l),%d1
	move.l	%a1@+,%a0@-
	move.w	%a2@(-6),%d3
	move.l	(8,%sp,%d1.w),%d0
	pea	0(%sp)			| a known zero displacement: (%sp)
	move.l	(0,%a0),%d0
	move.l	%a1@(0),%d0
	move.l	0(%a0,%d0.l),%d0	| an index keeps its 8-bit field
	move.l	0(%pc),%d0		| and %pc its 16-bit one
.Lz1:
.Lz2:
	move.l	.Lz2-.Lz1(%a0),%d0	| labels already seen at one place
	move.l	.Lz4-.Lz3(%a0),%d0	| not yet seen: 16-bit 0
.Lz3:
.Lz4:
	move.w	.Ltab(%pc,%d0.w),%d0
	jmp	%pc@(2,%d0:w)
	.balignw 4,0x4e71
.Ltab:
	.word	.Lnear-.Ltab, 1f-.Ltab
1:	moveq	#0,%d0
	jra	1b
2:	jne	2f
	jra	2b
2:	move.w	%sr,%d0
	move.w	%d0,%ccr
	move.l	%usp,%a0
#APP
| 1311 "inline.h" 1
	jsr 0xF8100C
| 0 "" 2
#NO_APP
	move.l	#entry,%d0 ; move.l #local_data+2,%d1
	rts
local_fn:
	clr.l	%d0
	rts
__mulsi3:
__umodsi3:
__divsi3:
	moveq	#0,%d0
	rts
	.align	4
local_data:
	.long	0x47505232, entry, local_fn - entry, .Ltab
	.word	24, 29, -1, 0xffff
	.byte	1, -1, 255, 'x
	.section	.rodata.str1.1,"aMS",@progbits,1
	.string	"world"
	.ascii	"tab\there\"\\\101\x42\0"
	.section	.rodata,"a"
	.align	2
	.long	.Lmsg
.Lmsg:	.string	"not shared"
	.data
	.long	counter_common, asm_common, shared_counter
	.comm	asm_common,6,2
	.zero	3
	.align	2
	.long	stack_end
	.bss
	.zero	6
stack_end:
	.local	scratch
	.comm	scratch,10,4
