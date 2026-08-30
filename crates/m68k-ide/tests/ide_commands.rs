//! Comprehensive Unit and Integration Tests for m68k-ide commands.

use m68k_ide::commands::assembler::{AssembleRequest, assemble_code};
use m68k_ide::commands::bitplane::{ConvertImageRequest, convert_image_to_bitplanes};
use m68k_ide::commands::copper::parse_copperlist;
use m68k_ide::commands::disassembler::{DisassembleRequest, disassemble_bytes};
use m68k_ide::commands::floppy::{CreateAdfRequest, create_new_adf, inspect_adf};
use m68k_ide::commands::lsp_bridge::{
    LspQueryRequest, lsp_completion, lsp_diagnostics, lsp_format, lsp_hover,
};
use m68k_ide::commands::project::{ScaffoldProjectRequest, scaffold_project};

#[test]
fn test_assemble_code_success() {
    let req = AssembleRequest {
        source: "    ORG $1000\nSTART:\n    moveq #10,d0\n    rts\n".to_string(),
        cpu: Some("68000".to_string()),
        base_address: Some(0x1000),
    };

    let res = assemble_code(req);
    assert!(res.success);
    assert!(res.byte_count >= 4);
    assert!(res.binary_base64.is_some());
    assert!(res.hex_dump.is_some());
}

#[test]
fn test_assemble_code_with_errors() {
    let req = AssembleRequest {
        source: "START:\n    UNKNOWN_OPCODE #1,d0\n".to_string(),
        cpu: Some("68000".to_string()),
        base_address: None,
    };

    let res = assemble_code(req);
    assert!(!res.success);
    assert!(!res.errors.is_empty());
}

#[test]
fn test_disassemble_bytes() {
    let bytes = vec![0x70, 0x0A, 0x4E, 0x75]; // moveq #10,d0; rts
    let req = DisassembleRequest {
        bytes,
        origin: Some(0x1000),
        cpu: Some("68000".to_string()),
    };

    let res = disassemble_bytes(req);
    assert_eq!(res.lines.len(), 2);
    assert!(res.lines[0].text.to_ascii_uppercase().contains("MOVEQ"));
    assert!(res.lines[1].text.to_ascii_uppercase().contains("RTS"));
}

#[test]
fn test_floppy_create_and_inspect() {
    let req = CreateAdfRequest {
        disk_name: "TestDisk".to_string(),
        is_ffs: false,
        boot_code: None,
    };

    let adf_bytes = create_new_adf(req).expect("should create ADF");
    assert_eq!(adf_bytes.len(), 901120);

    let info = inspect_adf(adf_bytes).expect("should inspect ADF");
    assert_eq!(info.total_blocks, 1760);
    assert!(!info.is_ffs);
}

#[test]
fn test_copper_parsing() {
    // MOVE $0000 -> COLOR00 ($0180), WAIT VPOS=128, HPOS=0 ($8001, $FFFE), END ($FFFF, $FFFE)
    let bytes = vec![
        0x01, 0x80, 0x00, 0x00, 0x80, 0x01, 0xFF, 0xFE, 0xFF, 0xFF, 0xFF, 0xFE,
    ];

    let res = parse_copperlist(bytes);
    assert_eq!(res.instructions.len(), 3);
    assert!(res.instructions[0].description.contains("MOVE"));
    assert!(res.instructions[1].description.contains("WAIT"));
    assert_eq!(res.instructions[1].vpos, Some(128));
}

#[test]
fn test_scaffold_project() {
    let temp_dir = std::env::temp_dir().join("m68k_ide_scaffold_test");
    let req = ScaffoldProjectRequest {
        target_directory: temp_dir.to_string_lossy().to_string(),
        template: "amiga500".to_string(),
        project_name: "AmigaDemo".to_string(),
    };

    let config = scaffold_project(req).expect("should scaffold project");
    assert_eq!(config.name, "AmigaDemo");
    assert_eq!(config.target_cpu, "68000");
    assert!(temp_dir.join("src/main.s").exists());
    assert!(temp_dir.join("includes/custom.i").exists());
    assert!(temp_dir.join("project.json").exists());
}

#[test]
fn test_lsp_bridge_queries() {
    let src = "START:\n    move.w  #1,d0\n    rts\n".to_string();

    // Hover
    let hover = lsp_hover(LspQueryRequest {
        source: src.clone(),
        line: 1,
        character: 5,
    });
    assert!(hover.is_some());

    // Completion
    let comp = lsp_completion(LspQueryRequest {
        source: src.clone(),
        line: 1,
        character: 13,
    });
    assert!(!comp.is_empty());

    // Diagnostics
    let diags = lsp_diagnostics(src.clone(), Some("68000".to_string()));
    assert!(diags.is_empty());

    // Formatting
    let formatted = lsp_format("move.w #1,d0\n".to_string(), Some(4));
    assert!(formatted.contains("    move.w"));
}

#[test]
fn test_bitplane_conversion() {
    // 16x16 dummy RGB image
    let mut img_bytes = Vec::new();
    let img = image::RgbImage::new(16, 16);
    let mut cursor = std::io::Cursor::new(&mut img_bytes);
    img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();

    let req = ConvertImageRequest {
        image_bytes: img_bytes,
        max_bitplanes: 4,
        interleaved: false,
    };

    let res = convert_image_to_bitplanes(req).expect("should convert image");
    assert_eq!(res.width, 16);
    assert_eq!(res.height, 16);
    assert!(res.total_bytes > 0);
    assert!(!res.palette_hex.is_empty());
    assert!(res.copper_palette_asm.contains("COLOR00"));
}
