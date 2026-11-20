#NO_APP
	.file	"one.c"
	.text
	.align	2
	.globl	one
	.type	one, @function
one:
	movem.l #15420,-(%sp)
	moveq #0,%d3
	moveq #0,%d4
	lea table,%a3
.L2:
	cmp.l 36(%sp),%d4
	jlt .L12
	move.l %d3,writer
	moveq #127,%d0
	and.l %d3,%d0
	add.l %d0,pending
	addq.l #1,counter_common
	move.b %d3,%d0
	ext.w %d0
	ext.l %d0
	add.l %d0,shared_counter
	moveq #1,%d0
	and.l %d3,%d0
	add.l %d0,%d0
	add.l %d0,%d0
	lea names,%a0
	move.l (%a0,%d0.l),%a0
	cmp.b #97,(%a0)
	seq %d0
	ext.w %d0
	move.w %d0,%a0
	move.l %d3,%d0
	sub.l %a0,%d0
	movem.l (%sp)+,#15420
	rts
.L12:
	move.l 40(%sp),%d0
	add.l %d4,%d0
	moveq #6,%d1
	cmp.l %d0,%d1
	jcs .L3
	add.l %d0,%d0
	move.w .L5(%pc,%d0.l),%d0
	jmp %pc@(2,%d0:w)
	.balignw 2,0x284c
.L5:
	.word .L11-.L5
	.word .L13-.L5
	.word .L9-.L5
	.word .L8-.L5
	.word .L7-.L5
	.word .L6-.L5
	.word .L4-.L5
.L11:
	moveq #11,%d2
.L10:
	pea 15.w
	move.l %d4,-(%sp)
	jsr __umodsi3
	addq.l #8,%sp
	moveq #0,%d5
	move.b (%a3,%d0.l),%d5
	move.l %d2,-(%sp)
	move.l 48(%sp),-(%sp)
	jsr __mulsi3
	addq.l #8,%sp
	add.l %d0,%d5
	add.l %d5,%d3
	addq.l #1,%d4
	jra .L2
.L9:
	pea 2.w
	jsr two
	move.l %d0,%d2
	addq.l #4,%sp
	jra .L10
.L8:
	moveq #47,%d2
	jra .L10
.L7:
	moveq #59,%d2
	jra .L10
.L6:
	move.l writer,%d2
	jra .L10
.L4:
	moveq #71,%d2
	jra .L10
.L3:
	moveq #-1,%d2
	jra .L10
.L13:
	moveq #23,%d2
	jra .L10
	.size	one, .-one
	.section	.rodata.str1.1,"aMS",@progbits,1
.LC0:
	.string	"world"
.LC1:
	.string	"hello, world"
	.text
	.align	2
	.globl	greeting
	.type	greeting, @function
greeting:
	tst.l 4(%sp)
	jeq .L16
	lea .LC1,%a0
.L14:
	move.l %a0,%d0
	rts
.L16:
	lea .LC0,%a0
	jra .L14
	.size	greeting, .-greeting
	.section	.rodata.str1.1
.LC2:
	.string	"alpha"
.LC3:
	.string	"beta"
.LC4:
	.string	"gamma"
	.section	.rodata
	.align	2
	.type	names, @object
	.size	names, 12
names:
	.long	.LC2
	.long	.LC3
	.long	.LC4
	.type	table, @object
	.size	table, 15
table:
	.base64	"AAECA8gHCRsNCv+ABAUG"
	.local	pending
	.comm	pending,4,2
	.local	writer
	.comm	writer,4,2
	.globl	counter_common
	.section	.bss
	.align	2
	.type	counter_common, @object
	.size	counter_common, 4
counter_common:
	.zero	4
	.globl	__mulsi3
	.globl	__umodsi3
	.ident	"GCC: (Ubuntu 15.2.0-16ubuntu1) 15.2.0"
	.section	.note.GNU-stack,"",@progbits
