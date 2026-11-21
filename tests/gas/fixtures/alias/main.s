| Symbol aliases as libc code has them (weak_alias, strong_alias), weak
| definitions that another file overrides, constants set with .set, .equ
| and `=`, and GNU as's `mov` for `move`.
	.text
	.globl	start
start:
	jbsr	helper
	jbsr	pow10
	jbsr	twice
	mov.l	%d0,%d1
	mov.l	%d0,%a1
	mov.l	#1,%d0
	mov.w	#limit,%d2
	move.l	#scaled,%d3
	lea	table+offset,%a0
	lea	same,%a1
	jbsr	same
	rts
| GCC's -fipa-icf: a static function made an alias of an identical one;
| it relaxes like the label it stands for.
	.set	same,helper2
helper2:
	moveq	#3,%d0
	rts
	.weak	helper
helper:
	moveq	#1,%d0
	rts
	.globl	exp10
exp10:
	moveq	#10,%d0
	rts
	.weak	pow10
	.set	pow10,exp10
	.set	limit,100
	.equ	offset,4
	scaled = limit*4+offset
	.data
	.align	2
table:
	.long	pow10, helper, twice, table+offset
	.long	limit, scaled
