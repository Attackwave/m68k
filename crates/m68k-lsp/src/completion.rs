//! Completion provider for Motorola 68000 instructions, directives, registers, labels, and Amiga LVOs.

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, InsertTextFormat, Position,
};

use m68k_core::amiga_lvo::Library;

use crate::analysis::directives_docs::DIRECTIVES;
use crate::analysis::op_docs::lookup_opcode;
use crate::document::Document;

/// Generate completion items for a position in the document.
pub fn compute_completion(doc: &Document, _pos: Position) -> CompletionResponse {
    let mut items = Vec::new();

    // 1. Instruction mnemonics with snippets
    for mnem in COMMON_INSTRUCTIONS {
        let doc_entry = lookup_opcode(mnem);
        let detail = doc_entry.map(|d| d.summary).unwrap_or("m68k instruction");

        items.push(CompletionItem {
            label: mnem.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(detail.to_string()),
            insert_text: Some(get_instruction_snippet(mnem)),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        });

        // Also add uppercase variation
        items.push(CompletionItem {
            label: mnem.to_ascii_uppercase(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(detail.to_string()),
            insert_text: Some(get_instruction_snippet(mnem).to_ascii_uppercase()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        });
    }

    // 2. Assembler directives with snippets
    for dir in DIRECTIVES {
        items.push(CompletionItem {
            label: dir.name.to_ascii_uppercase(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some(format!("Directive: {}", dir.title)),
            documentation: Some(tower_lsp::lsp_types::Documentation::String(
                dir.summary.to_string(),
            )),
            insert_text: Some(get_directive_snippet(dir.name)),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        });
    }

    // 3. Registers
    for reg in REGISTERS {
        items.push(CompletionItem {
            label: reg.to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(format!("68k Register {}", reg.to_ascii_uppercase())),
            ..Default::default()
        });
        items.push(CompletionItem {
            label: reg.to_ascii_uppercase(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(format!("68k Register {}", reg.to_ascii_uppercase())),
            ..Default::default()
        });
    }

    // 4. Document symbols (labels, equates, macros)
    for (name, sym) in &doc.symbols {
        let (kind, detail) = match sym.kind {
            crate::document::SymbolDefKind::Label => (
                CompletionItemKind::FUNCTION,
                format!("Label defined at line {}", sym.line_idx + 1),
            ),
            crate::document::SymbolDefKind::Equate => (
                CompletionItemKind::CONSTANT,
                format!("Constant: {}", sym.value_str.as_deref().unwrap_or("")),
            ),
            crate::document::SymbolDefKind::Macro => (
                CompletionItemKind::SNIPPET,
                format!("Macro defined at line {}", sym.line_idx + 1),
            ),
            crate::document::SymbolDefKind::Section => (
                CompletionItemKind::MODULE,
                format!("Section at line {}", sym.line_idx + 1),
            ),
        };

        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(kind),
            detail: Some(detail),
            ..Default::default()
        });
    }

    // 5. AmigaOS LVO symbols
    for lib in [
        Library::Exec,
        Library::Dos,
        Library::Graphics,
        Library::Intuition,
    ] {
        for (offset, name) in lib.entries() {
            let lvo_name = format!("_LVO{}", name);
            items.push(CompletionItem {
                label: lvo_name,
                kind: Some(CompletionItemKind::INTERFACE),
                detail: Some(format!(
                    "{}.library (-0x{:03X})",
                    lib.file_name().trim_end_matches(".library"),
                    offset
                )),
                insert_text: Some(format!("_LVO{}", name)),
                ..Default::default()
            });
        }
    }

    CompletionResponse::Array(items)
}

fn get_instruction_snippet(mnem: &str) -> String {
    match mnem {
        "move" => "move.${1|w,l,b|} ${2:source}, ${3:dest}".to_string(),
        "moveq" => "moveq #${1:0}, ${2:d0}".to_string(),
        "movem" => "movem.${1|l,w|} ${2:d0-d7/a0-a6}, ${3:-(sp)}".to_string(),
        "lea" => "lea ${1:source}, ${2:a0}".to_string(),
        "pea" => "pea ${1:source}".to_string(),
        "add" => "add.${1|w,l,b|} ${2:source}, ${3:d0}".to_string(),
        "addq" => "addq.${1|w,l,b|} #${1:1}, ${2:d0}".to_string(),
        "sub" => "sub.${1|w,l,b|} ${2:source}, ${3:d0}".to_string(),
        "subq" => "subq.${1|w,l,b|} #${1:1}, ${2:d0}".to_string(),
        "cmp" => "cmp.${1|w,l,b|} ${2:source}, ${3:d0}".to_string(),
        "cmpi" => "cmpi.${1|w,l,b|} #${1:value}, ${2:dest}".to_string(),
        "bra" => "bra ${1:label}".to_string(),
        "bsr" => "bsr ${1:routine}".to_string(),
        "beq" => "beq ${1:label}".to_string(),
        "bne" => "bne ${1:label}".to_string(),
        "bge" => "bge ${1:label}".to_string(),
        "blt" => "blt ${1:label}".to_string(),
        "bgt" => "bgt ${1:label}".to_string(),
        "ble" => "ble ${1:label}".to_string(),
        "bpl" => "bpl ${1:label}".to_string(),
        "bmi" => "bmi ${1:label}".to_string(),
        "dbra" => "dbra ${1:d0}, ${2:label}".to_string(),
        "dbeq" => "dbeq ${1:d0}, ${2:label}".to_string(),
        "dbne" => "dbne ${1:d0}, ${2:label}".to_string(),
        "jmp" => "jmp ${1:target}".to_string(),
        "jsr" => "jsr ${1:target}".to_string(),
        "rts" => "rts".to_string(),
        "nop" => "nop".to_string(),
        "clr" => "clr.${1|w,l,b|} ${2:dest}".to_string(),
        "tst" => "tst.${1|w,l,b|} ${2:dest}".to_string(),
        "not" => "not.${1|w,l,b|} ${2:dest}".to_string(),
        "neg" => "neg.${1|w,l,b|} ${2:dest}".to_string(),
        "asl" => "asl.${1|w,l,b|} #${1:1}, ${2:d0}".to_string(),
        "asr" => "asr.${1|w,l,b|} #${1:1}, ${2:d0}".to_string(),
        "lsl" => "lsl.${1|w,l,b|} #${1:1}, ${2:d0}".to_string(),
        "lsr" => "lsr.${1|w,l,b|} #${1:1}, ${2:d0}".to_string(),
        "btst" => "btst #${1:bit}, ${2:dest}".to_string(),
        "bset" => "bset #${1:bit}, ${2:dest}".to_string(),
        "bclr" => "bclr #${1:bit}, ${2:dest}".to_string(),
        "bchg" => "bchg #${1:bit}, ${2:dest}".to_string(),
        "link" => "link ${1:a6}, #${2:-size}".to_string(),
        "unlk" => "unlk ${1:a6}".to_string(),
        _ => mnem.to_string(),
    }
}

fn get_directive_snippet(dir: &str) -> String {
    match dir {
        "org" => "ORG     $${1:1000}".to_string(),
        "section" => "SECTION ${1:MainCode},${2|code,data,bss|}".to_string(),
        "dc" => "DC.${1|b,w,l|}   ${2:value}".to_string(),
        "dcb" => "DCB.${1|b,w,l|}  ${1:count}, ${2:0}".to_string(),
        "ds" => "DS.${1|b,w,l|}   ${1:count}".to_string(),
        "even" => "EVEN".to_string(),
        "align" => "ALIGN   ${1:2}".to_string(),
        "equ" => "${1:NAME} EQU     ${2:value}".to_string(),
        "include" => "INCLUDE \"${1:file.i}\"".to_string(),
        "incbin" => "INCBIN  \"${1:assets/data.bin}\"".to_string(),
        "macro" => "${1:NAME} MACRO\n        $0\n        ENDM".to_string(),
        "rept" => "REPT    ${1:count}\n        $0\n        ENDR".to_string(),
        "if" => "IF      ${1:condition}\n        $0\n        ENDC".to_string(),
        "ifd" => "IFD     ${1:SYMBOL}\n        $0\n        ENDC".to_string(),
        "ifnd" => "IFND    ${1:SYMBOL}\n        $0\n        ENDC".to_string(),
        _ => dir.to_ascii_uppercase(),
    }
}

static COMMON_INSTRUCTIONS: &[&str] = &[
    "move", "movea", "moveq", "movem", "movep", "exg", "swap", "ext", "extb", "lea", "pea", "link",
    "unlk", "add", "adda", "addi", "addq", "addx", "sub", "suba", "subi", "subq", "subx", "cmp",
    "cmpa", "cmpi", "cmpm", "cmp2", "muls", "mulu", "divs", "divu", "clr", "neg", "negx", "tst",
    "and", "andi", "or", "ori", "eor", "eori", "not", "asl", "asr", "lsl", "lsr", "rol", "ror",
    "roxl", "roxr", "btst", "bset", "bclr", "bchg", "bra", "bsr", "beq", "bne", "bge", "blt",
    "bgt", "ble", "bpl", "bmi", "bcc", "bcs", "bvc", "bvs", "bhi", "bls", "dbra", "dbeq", "dbne",
    "dbge", "dblt", "dbgt", "dble", "jmp", "jsr", "rts", "rte", "rtr", "nop", "trap", "trapv",
    "illegal", "movec", "moves", "cas", "cas2", "fmove", "fadd", "fsub", "fmul", "fdiv",
];

static REGISTERS: &[&str] = &[
    "d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7", "a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7",
    "sp", "pc", "sr", "ccr", "usp", "ssp", "vbr", "cacr", "fp0", "fp1", "fp2", "fp3", "fp4", "fp5",
    "fp6", "fp7",
];

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_completion_contains_instructions_and_registers() {
        let uri = Url::parse("file:///test.s").unwrap();
        let doc = Document::new(uri, 1, "START:\n    rts\n".to_string());
        let res = compute_completion(
            &doc,
            Position {
                line: 0,
                character: 0,
            },
        );

        if let CompletionResponse::Array(items) = res {
            assert!(items.iter().any(|i| i.label == "move"));
            assert!(items.iter().any(|i| i.label == "MOVE"));
            assert!(items.iter().any(|i| i.label == "SECTION"));
            assert!(items.iter().any(|i| i.label == "d0"));
            assert!(items.iter().any(|i| i.label == "START"));
            assert!(items.iter().any(|i| i.label == "_LVOOpenLibrary"));
        } else {
            panic!("Expected array of completions");
        }
    }
}
