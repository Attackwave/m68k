	.text
	.globl	helper
helper:
	moveq	#2,%d0
	rts
	.globl	twice
	.set	twice,helper+2
