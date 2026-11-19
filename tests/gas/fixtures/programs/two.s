#NO_APP
	.file	"two.c"
	.text
	.align	2
	.globl	two
	.type	two, @function
two:
	lea (-40,%sp),%sp
	move.l %a6,-(%sp)
	move.l 48(%sp),-(%sp)
	jsr greeting
	move.l %a0,%a6
	move.l 8(%sp),-(%sp)
	pea 3.w
	moveq #3,%d0
	and.l 60(%sp),%d0
	move.l %d0,-(%sp)
	jsr one
	move.b (%a6),%d1
	ext.w %d1
	lea (16,%sp),%sp
	move.l %d0,%a1
	lea 6(%a1,%d1.w),%a0
	move.l %a0,%d0
	move.l (%sp)+,%a6
	lea (40,%sp),%sp
	rts
	.size	two, .-two
	.globl	shared_counter
	.data
	.align	2
	.type	shared_counter, @object
	.size	shared_counter, 4
shared_counter:
	.long	5
	.ident	"GCC: (Ubuntu 15.2.0-16ubuntu1) 15.2.0"
	.section	.note.GNU-stack,"",@progbits
