| A second translation unit with the same groups: its copies go.
	.section	.text.twice,"axG",@progbits,twice,comdat
	.weak	twice
twice:
	moveq	#2,%d0
	nop
	rts

	.section	.bss.counter,"awG",@nobits,counter,comdat
	.weak	counter
	.align	2
counter:
	.zero	4

	.text
	.globl	other
other:
	lea	wide2,%a0
	jbsr	twice
	rts

	.section	.rodata.str4.4,"aMS",@progbits,4
	.align	2
wide2:
	.string	"\0\0\0b\0\0\0c\0\0\0"

| Read-only data that ends at an odd address.
	.section	.rodata
	.byte	9

	.section	.init_array.00101,"aw"
	.align	2
	.long	other
	.section	.init_array,"aw"
	.align	2
	.long	other
	.section	.fini_array.00101,"aw"
	.align	2
	.long	other
