//! Documentation and metadata for Motorola 68000 assembler directives.

/// Metadata for an assembler directive.
#[derive(Debug, Clone)]
pub struct DirectiveDoc {
    pub name: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub syntax: &'static [&'static str],
    pub example: &'static str,
}

/// Lookup documentation for an assembler directive by name.
pub fn lookup_directive(name: &str) -> Option<DirectiveDoc> {
    let lower = name.to_ascii_lowercase();
    DIRECTIVES
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(&lower))
        .cloned()
}

/// All directive documentation entries.
pub static DIRECTIVES: &[DirectiveDoc] = &[
    DirectiveDoc {
        name: "org",
        title: "Origin Address",
        summary: "Sets the program location counter (PC) to an absolute memory address.",
        syntax: &["ORG <address>"],
        example: "    ORG     $1000\nSTART:\n    move.l  #0,d0",
    },
    DirectiveDoc {
        name: "section",
        title: "Named Section",
        summary: "Declares or switches to a named section (e.g. text, data, bss) with optional memory attributes (code, data, bss, chip, fast).",
        syntax: &["SECTION <name>,<type>[,<memory_type>]"],
        example: "    SECTION MainCode,code\n    SECTION Graphics,data,chip",
    },
    DirectiveDoc {
        name: "dc",
        title: "Define Constant",
        summary: "Allocates initialized data in memory. Supports byte (.B), word (.W), longword (.L), single (.S), double (.D), extended (.X), and packed decimal (.P).",
        syntax: &[
            "DC.B <val1>[,<val2>,...]",
            "DC.W <val1>[,<val2>,...]",
            "DC.L <val1>[,<val2>,...]",
            "DC.B \"string\",0",
        ],
        example: "Message:  DC.B    \"Hello World!\",10,0\nValues:   DC.W    $1234, $5678\nPointer:  DC.L    Message",
    },
    DirectiveDoc {
        name: "dcb",
        title: "Define Constant Block",
        summary: "Fills a block of memory with duplicate data items of the given size.",
        syntax: &["DCB.<size> <count>,<fill_value>"],
        example: "Buffer:   DCB.B   256,0       ; 256 bytes of zeros\nTable:    DCB.W   16,$FFFF    ; 16 words initialized to -1",
    },
    DirectiveDoc {
        name: "ds",
        title: "Define Storage (Reserve Memory)",
        summary: "Reserves uninitialized memory space of the specified size without emitting data bytes (commonly in BSS).",
        syntax: &["DS.<size> <count>"],
        example: "Stack:    DS.L    1024        ; 1024 longwords (4KB) for stack\nVar:      DS.W    1           ; 1 word reserved",
    },
    DirectiveDoc {
        name: "even",
        title: "Align to Even Word Boundary",
        summary: "Forces the current program counter to the next even address (word boundary), inserting a padding byte if necessary.",
        syntax: &["EVEN"],
        example: "String:   DC.B    \"Test\",0\n          EVEN                ; Ensure following code/word is aligned\nNextWord: DC.W    $1234",
    },
    DirectiveDoc {
        name: "align",
        title: "Align Location Counter",
        summary: "Aligns the location counter to a power-of-two boundary: `ALIGN n` aligns to 2^n bytes (e.g. `ALIGN 2` = 4-byte boundary, `ALIGN 4` = 16-byte boundary).",
        syntax: &["ALIGN <bit_count>"],
        example: "    ALIGN 2             ; Align to 4-byte (longword) boundary\n    ALIGN 4             ; Align to 16-byte cache line",
    },
    DirectiveDoc {
        name: "cnop",
        title: "Conditional NOP Alignment",
        summary: "Aligns PC to a byte boundary with an offset: `CNOP <offset>,<alignment>` inserts NOPs (`$4E71`) or zero bytes until (PC - offset) is divisible by alignment.",
        syntax: &["CNOP <offset>,<alignment>"],
        example: "    CNOP 0,4            ; Align to longword boundary with NOP padding",
    },
    DirectiveDoc {
        name: "equ",
        title: "Equate Constant Symbol",
        summary: "Defines a permanent symbolic constant evaluated at assembly time. Alternatively written as `NAME = <expr>`.",
        syntax: &["<name> EQU <expr>", "<name> = <expr>"],
        example: "ExecBase  EQU     $00000004\nSCREEN_W  =       320\nSCREEN_H  =       256",
    },
    DirectiveDoc {
        name: "set",
        title: "Set Re-definable Symbol",
        summary: "Defines a symbolic constant that can be redefined later with another `SET` directive.",
        syntax: &["<name> SET <expr>"],
        example: "INDEX     SET     0\nINDEX     SET     INDEX+1",
    },
    DirectiveDoc {
        name: "include",
        title: "Include Source File",
        summary: "Splices the contents of another source file at the current position in the file.",
        syntax: &["INCLUDE \"<filename>\""],
        example: "    INCLUDE \"custom.i\"\n    INCLUDE \"hardware/custom.i\"",
    },
    DirectiveDoc {
        name: "incbin",
        title: "Include Binary File",
        summary: "Embeds a raw binary file (e.g. graphic data, sound sample) directly into the output.",
        syntax: &["INCBIN \"<filename>\""],
        example: "RawData:  INCBIN  \"assets/image.raw\"",
    },
    DirectiveDoc {
        name: "macro",
        title: "Macro Definition",
        summary: "Defines a reusable block of code. Parameters can be referenced inside the macro as `\\1`, `\\2`, etc., or `\\@` for unique labels.",
        syntax: &["<name> MACRO", "       ... macro body ...", "       ENDM"],
        example: "PUSH    MACRO\n        move.l  \\1,-(sp)\n        ENDM\n\n        PUSH    d0\n        PUSH    a0",
    },
    DirectiveDoc {
        name: "endm",
        title: "End Macro Definition",
        summary: "Marks the end of a macro block.",
        syntax: &["ENDM"],
        example: "MY_MACRO MACRO\n         nop\n         ENDM",
    },
    DirectiveDoc {
        name: "rept",
        title: "Repeat Block",
        summary: "Repeats a block of code a specified number of times at assembly time.",
        syntax: &["REPT <count>", "     ... repeated body ...", "ENDR"],
        example: "    REPT    4\n    addq.l  #1,d0\n    ENDR",
    },
    DirectiveDoc {
        name: "endr",
        title: "End Repeat Block",
        summary: "Marks the end of a `REPT`, `IRP`, or `IRPC` loop block.",
        syntax: &["ENDR"],
        example: "    REPT 8\n    asl.w #1,d0\n    ENDR",
    },
    DirectiveDoc {
        name: "if",
        title: "Conditional Assembly (IF)",
        summary: "Assembles following code block only if the expression evaluates to non-zero (true).",
        syntax: &["IF <condition_expr>", "   ... body ...", "ENDC"],
        example: "    IF DEBUG=1\n    jsr     PrintDebugInfo\n    ENDC",
    },
    DirectiveDoc {
        name: "ifd",
        title: "Conditional Assembly (If Defined)",
        summary: "Assembles following code block if the given symbol has been defined. Equivalent to `IFDEF`.",
        syntax: &["IFD <symbol>", "IFND <symbol> (If Not Defined)"],
        example: "    IFND EXEC_TYPES_I\nEXEC_TYPES_I SET 1\n    ... header declarations ...\n    ENDC",
    },
    DirectiveDoc {
        name: "else",
        title: "Conditional Assembly (ELSE)",
        summary: "Alternative branch in an `IF` conditional assembly block.",
        syntax: &["ELSE"],
        example: "    IFD AMIGA\n    move.l  $4.w,a6\n    ELSE\n    move.l  #0,a6\n    ENDC",
    },
    DirectiveDoc {
        name: "endc",
        title: "End Conditional Block",
        summary: "Marks the end of an `IF`/`IFD`/`IFND` block. Motorola syntax synonym for `ENDIF`.",
        syntax: &["ENDC", "ENDIF"],
        example: "    IFD DEBUG\n    nop\n    ENDC",
    },
    DirectiveDoc {
        name: "rs",
        title: "Structure Field Offset",
        summary: "Defines structure element offsets. Automatically advances `RS_COUNTER` by the allocated size.",
        syntax: &[
            "RSRESET",
            "RSSET <start_offset>",
            "<field> RS.<size> <count>",
        ],
        example: "        RSRESET\nNode_Succ   RS.L    1   ; Offset 0\nNode_Pred   RS.L    1   ; Offset 4\nNode_Type   RS.B    1   ; Offset 8\nNode_Pri    RS.B    1   ; Offset 9\nNode_Name   RS.L    1   ; Offset 10\nNode_SIZEOF RS.W    0   ; Size = 14",
    },
    DirectiveDoc {
        name: "opt",
        title: "Assembler Options",
        summary: "Configures assembler optimization, warning, and listing options (e.g. `OPT O+`, `OPT W5-`).",
        syntax: &["OPT <flag>[+|-]"],
        example: "    OPT     O+,W5-",
    },
    DirectiveDoc {
        name: "end",
        title: "End of Assembly",
        summary: "Explicitly terminates source file processing. Anything following `END` is ignored.",
        syntax: &["END [<start_label>]"],
        example: "    END START",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_directive() {
        let org = lookup_directive("org").expect("org should exist");
        assert_eq!(org.name, "org");

        let dcb = lookup_directive("dcb").expect("dcb should exist");
        assert_eq!(dcb.name, "dcb");

        let rs = lookup_directive("rs").expect("rs should exist");
        assert_eq!(rs.name, "rs");
    }
}
