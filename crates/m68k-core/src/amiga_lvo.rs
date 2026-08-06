//! AmigaOS library vector offsets (LVOs).
//!
//! An Amiga program calls the operating system as `jsr -$1e(a6)`, where
//! `a6` holds a library base and the negative offset selects an entry in
//! the jump table below it. The offsets are a documented ABI — they are
//! what `dos.library`'s `Open` *is* — so a disassembler can name them and
//! turn an opaque number into the call it actually is.
//!
//! The measurement that motivated this: across four real hunk
//! executables, `jsr -$xx(a6)` outnumbers every other computed-jump form
//! roughly ten to one (44/7/48/56 occurrences against 3/3/3/8 for
//! `jsr (an)`). Those targets live in the library, outside the image, so
//! no amount of tracing or emulation recovers code for them — but naming
//! them recovers the *meaning*, which is what a reader wants.
//!
//! # Why the caller must say which library
//!
//! An offset alone is ambiguous: `-$1e` is `Supervisor` in `exec.library`
//! and `Open` in `dos.library`. Which base sits in `a6` is generally not
//! decidable from the instruction — in practice it comes from a variable
//! (`movea.l $69c,a6`) whose value is only known at run time. Guessing
//! would put confident wrong names in the listing, which is worse than no
//! name at all, so the library is named by the caller and applied
//! uniformly.
//!
//! Offsets are always negative multiples of 6 and are stored here as
//! their positive magnitude.

/// A library whose vector offsets are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Library {
    Exec,
    Dos,
    Graphics,
    Intuition,
}

impl Library {
    /// Parse a library name as a user would write it: `exec`,
    /// `exec.library`, or `dos`.
    pub fn parse(name: &str) -> Option<Self> {
        let base = name
            .trim()
            .to_ascii_lowercase()
            .trim_end_matches(".library")
            .to_string();
        match base.as_str() {
            "exec" => Some(Library::Exec),
            "dos" => Some(Library::Dos),
            "graphics" | "gfx" => Some(Library::Graphics),
            "intuition" => Some(Library::Intuition),
            _ => None,
        }
    }

    /// The names accepted by [`Library::parse`], for error messages.
    pub fn names() -> &'static [&'static str] {
        &["exec", "dos", "graphics", "intuition"]
    }

    fn table(self) -> &'static [(u16, &'static str)] {
        match self {
            Library::Exec => EXEC,
            Library::Dos => DOS,
            Library::Graphics => GRAPHICS,
            Library::Intuition => INTUITION,
        }
    }
}

/// Name of the routine at `-offset(a6)` in `library`, if known.
///
/// `offset` is the positive magnitude of the displacement, so
/// `jsr -$1e(a6)` is looked up as `lookup(lib, 0x1e)`.
pub fn lookup(library: Library, offset: u16) -> Option<&'static str> {
    // Every LVO is a multiple of 6; anything else is not a library call
    // and must not be given a name by rounding to a neighbour.
    if offset == 0 || !offset.is_multiple_of(6) {
        return None;
    }
    library
        .table()
        .iter()
        .find(|(o, _)| *o == offset)
        .map(|(_, name)| *name)
}

/// `exec.library`. The first four entries are the standard library
/// preamble (`Open`/`Close`/`Expunge`/reserved) shared by every library.
const EXEC: &[(u16, &str)] = &[
    (0x06, "Open"),
    (0x0C, "Close"),
    (0x12, "Expunge"),
    (0x1E, "Supervisor"),
    (0x48, "InitCode"),
    (0x4E, "InitStruct"),
    (0x54, "MakeLibrary"),
    (0x5A, "MakeFunctions"),
    (0x60, "FindResident"),
    (0x66, "InitResident"),
    (0x6C, "Alert"),
    (0x72, "Debug"),
    (0x78, "Disable"),
    (0x7E, "Enable"),
    (0x84, "Forbid"),
    (0x8A, "Permit"),
    (0x90, "SetSR"),
    (0x96, "SuperState"),
    (0x9C, "UserState"),
    (0xA2, "SetIntVector"),
    (0xA8, "AddIntServer"),
    (0xAE, "RemIntServer"),
    (0xB4, "Cause"),
    (0xBA, "Allocate"),
    (0xC0, "Deallocate"),
    (0xC6, "AllocMem"),
    (0xCC, "AllocAbs"),
    (0xD2, "FreeMem"),
    (0xD8, "AvailMem"),
    (0xDE, "AllocEntry"),
    (0xE4, "FreeEntry"),
    (0xEA, "Insert"),
    (0xF0, "AddHead"),
    (0xF6, "AddTail"),
    (0xFC, "Remove"),
    (0x102, "RemHead"),
    (0x108, "RemTail"),
    (0x10E, "Enqueue"),
    (0x114, "FindName"),
    (0x11A, "AddTask"),
    (0x120, "RemTask"),
    (0x126, "FindTask"),
    (0x12C, "SetTaskPri"),
    (0x132, "SetSignal"),
    (0x138, "SetExcept"),
    (0x13E, "Wait"),
    (0x144, "Signal"),
    (0x14A, "AllocSignal"),
    (0x150, "FreeSignal"),
    (0x156, "AllocTrap"),
    (0x15C, "FreeTrap"),
    (0x162, "AddPort"),
    (0x168, "RemPort"),
    (0x16E, "PutMsg"),
    (0x174, "GetMsg"),
    (0x17A, "ReplyMsg"),
    (0x180, "WaitPort"),
    (0x186, "FindPort"),
    (0x18C, "AddLibrary"),
    (0x192, "RemLibrary"),
    (0x198, "OldOpenLibrary"),
    (0x19E, "CloseLibrary"),
    (0x1A4, "SetFunction"),
    (0x1AA, "SumLibrary"),
    (0x1B0, "AddDevice"),
    (0x1B6, "RemDevice"),
    (0x1BC, "OpenDevice"),
    (0x1C2, "CloseDevice"),
    (0x1C8, "DoIO"),
    (0x1CE, "SendIO"),
    (0x1D4, "CheckIO"),
    (0x1DA, "WaitIO"),
    (0x1E0, "AbortIO"),
    (0x1E6, "AddResource"),
    (0x1EC, "RemResource"),
    (0x1F2, "OpenResource"),
    (0x20A, "RawDoFmt"),
    (0x210, "GetCC"),
    (0x216, "TypeOfMem"),
    (0x21C, "Procure"),
    (0x222, "Vacate"),
    (0x228, "OpenLibrary"),
    // V36+
    (0x22E, "InitSemaphore"),
    (0x234, "ObtainSemaphore"),
    (0x23A, "ReleaseSemaphore"),
    (0x240, "AttemptSemaphore"),
    (0x246, "ObtainSemaphoreList"),
    (0x24C, "ReleaseSemaphoreList"),
    (0x252, "FindSemaphore"),
    (0x258, "AddSemaphore"),
    (0x25E, "RemSemaphore"),
    (0x264, "SumKickData"),
    (0x26A, "AddMemList"),
    (0x270, "CopyMem"),
    (0x276, "CopyMemQuick"),
    (0x27C, "CacheClearU"),
    (0x282, "CacheClearE"),
    (0x288, "CacheControl"),
    (0x28E, "CreateIORequest"),
    (0x294, "DeleteIORequest"),
    (0x29A, "CreateMsgPort"),
    (0x2A0, "DeleteMsgPort"),
    (0x2A6, "ObtainSemaphoreShared"),
    (0x2AC, "AllocVec"),
    (0x2B2, "FreeVec"),
    (0x2B8, "CreatePool"),
    (0x2BE, "DeletePool"),
    (0x2C4, "AllocPooled"),
    (0x2CA, "FreePooled"),
    (0x2D0, "AttemptSemaphoreShared"),
    (0x2D6, "ColdReboot"),
    (0x2DC, "StackSwap"),
];

/// `dos.library`.
const DOS: &[(u16, &str)] = &[
    (0x06, "Open"),
    (0x0C, "Close"),
    (0x12, "Read"),
    (0x18, "Write"),
    (0x1E, "Input"),
    (0x24, "Output"),
    (0x2A, "Seek"),
    (0x30, "DeleteFile"),
    (0x36, "Rename"),
    (0x3C, "Lock"),
    (0x42, "UnLock"),
    (0x48, "DupLock"),
    (0x4E, "Examine"),
    (0x54, "ExNext"),
    (0x5A, "Info"),
    (0x60, "CreateDir"),
    (0x66, "CurrentDir"),
    (0x6C, "IoErr"),
    (0x72, "CreateProc"),
    (0x78, "Exit"),
    (0x7E, "LoadSeg"),
    (0x84, "UnLoadSeg"),
    (0x96, "DeviceProc"),
    (0x9C, "SetComment"),
    (0xA2, "SetProtection"),
    (0xA8, "DateStamp"),
    (0xAE, "Delay"),
    (0xB4, "WaitForChar"),
    (0xBA, "ParentDir"),
    (0xC0, "IsInteractive"),
    (0xC6, "Execute"),
    // V36+
    (0xCC, "AllocDosObject"),
    (0xD2, "FreeDosObject"),
    (0xD8, "DoPkt"),
    (0xEA, "SendPkt"),
    (0xF0, "WaitPkt"),
    (0xF6, "ReplyPkt"),
    (0x102, "LockRecord"),
    (0x10E, "UnLockRecord"),
    (0x11A, "SelectInput"),
    (0x120, "SelectOutput"),
    (0x126, "FGetC"),
    (0x12C, "FPutC"),
    (0x132, "UnGetC"),
    (0x138, "FRead"),
    (0x13E, "FWrite"),
    (0x144, "FGets"),
    (0x14A, "FPuts"),
    (0x150, "VFWritef"),
    (0x156, "VFPrintf"),
    (0x15C, "Flush"),
    (0x162, "SetVBuf"),
    (0x16E, "ExamineFH"),
    (0x174, "ParentOfFH"),
    (0x17A, "OpenFromLock"),
    (0x180, "ParentDir"),
    (0x192, "NameFromLock"),
    (0x1A4, "NameFromFH"),
    (0x1BC, "GetProgramName"),
    (0x1E0, "PutStr"),
    (0x1E6, "VPrintf"),
];

/// `graphics.library` — the entries an ordinary program reaches for.
const GRAPHICS: &[(u16, &str)] = &[
    (0x06, "Open"),
    (0x0C, "Close"),
    (0x36, "BltBitMap"),
    (0x3C, "BltTemplate"),
    (0x42, "ClearEOL"),
    (0x48, "ClearScreen"),
    (0x4E, "TextLength"),
    (0x54, "Text"),
    (0x5A, "SetFont"),
    (0x60, "OpenFont"),
    (0x66, "CloseFont"),
    (0x6C, "AskSoftStyle"),
    (0x72, "SetSoftStyle"),
    (0x78, "AddBob"),
    (0x7E, "AddVSprite"),
    (0x84, "DoCollision"),
    (0x8A, "DrawGList"),
    (0x90, "InitGels"),
    (0x96, "InitMasks"),
    (0xF6, "LoadRGB4"),
    (0xFC, "InitRastPort"),
    (0x108, "Move"),
    (0x10E, "Draw"),
    (0x114, "AreaMove"),
    (0x11A, "AreaDraw"),
    (0x120, "AreaEnd"),
    (0x126, "WaitBlit"),
    (0x144, "SetAPen"),
    (0x14A, "SetBPen"),
    (0x150, "SetDrMd"),
    (0x156, "SetRast"),
    (0x162, "WritePixel"),
    (0x168, "ReadPixel"),
    (0x186, "RectFill"),
    (0x1AA, "WaitTOF"),
    (0x1FE, "AllocRaster"),
    (0x204, "FreeRaster"),
];

/// `intuition.library` — the entries an ordinary program reaches for.
const INTUITION: &[(u16, &str)] = &[
    (0x06, "Open"),
    (0x0C, "Close"),
    (0x24, "ClearMenuStrip"),
    (0x2A, "ClearPointer"),
    (0x36, "CloseScreen"),
    (0x3C, "CloseWindow"),
    (0x48, "DisplayAlert"),
    (0x4E, "DisplayBeep"),
    (0x54, "DoubleClick"),
    (0x5A, "DrawBorder"),
    (0x60, "DrawImage"),
    (0x72, "GetDefPrefs"),
    (0x84, "InitRequester"),
    (0x8A, "ItemAddress"),
    (0x90, "ModifyIDCMP"),
    (0x96, "ModifyProp"),
    (0x9C, "MoveScreen"),
    (0xA2, "MoveWindow"),
    (0xA8, "OffGadget"),
    (0xAE, "OffMenu"),
    (0xB4, "OnGadget"),
    (0xBA, "OnMenu"),
    (0xC0, "OpenIntuition"),
    (0xC6, "OpenScreen"),
    (0xCC, "OpenWindow"),
    (0xD2, "OpenWorkBench"),
    (0xD8, "PrintIText"),
    (0xDE, "RefreshGadgets"),
    (0xE4, "RemoveGadget"),
    (0xEA, "ReportMouse"),
    (0xF0, "Request"),
    (0xFC, "ScreenToBack"),
    (0x102, "ScreenToFront"),
    (0x108, "SetDMRequest"),
    (0x114, "SetMenuStrip"),
    (0x11A, "SetPointer"),
    (0x120, "SetWindowTitles"),
    (0x126, "ShowTitle"),
    (0x132, "SizeWindow"),
    (0x138, "ViewAddress"),
    (0x13E, "ViewPortAddress"),
    (0x144, "WindowToBack"),
    (0x14A, "WindowToFront"),
    (0x156, "AutoRequest"),
    (0x162, "BuildSysRequest"),
    (0x186, "WBenchToBack"),
    (0x18C, "WBenchToFront"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_known_vectors() {
        assert_eq!(lookup(Library::Dos, 0x1e), Some("Input"));
        assert_eq!(lookup(Library::Exec, 0x228), Some("OpenLibrary"));
        assert_eq!(lookup(Library::Exec, 0xc6), Some("AllocMem"));
    }

    /// The same offset means different things in different libraries,
    /// which is why the caller has to say which one is in a6.
    #[test]
    fn the_same_offset_differs_between_libraries() {
        assert_eq!(lookup(Library::Exec, 0x1e), Some("Supervisor"));
        assert_eq!(lookup(Library::Dos, 0x1e), Some("Input"));
    }

    /// Every LVO is a multiple of 6. An offset that is not one is not a
    /// library call, and must not be given a name by rounding.
    #[test]
    fn rejects_offsets_that_are_not_multiples_of_six() {
        assert_eq!(lookup(Library::Dos, 0x1f), None);
        assert_eq!(lookup(Library::Dos, 0x20), None);
        assert_eq!(lookup(Library::Dos, 0), None);
    }

    #[test]
    fn unknown_offsets_have_no_name() {
        assert_eq!(lookup(Library::Dos, 0xFFC), None);
    }

    #[test]
    fn parses_library_names_as_written() {
        assert_eq!(Library::parse("dos"), Some(Library::Dos));
        assert_eq!(Library::parse("dos.library"), Some(Library::Dos));
        assert_eq!(Library::parse("EXEC"), Some(Library::Exec));
        assert_eq!(Library::parse("gfx"), Some(Library::Graphics));
        assert_eq!(Library::parse("nonesuch"), None);
    }

    /// Tables are keyed by offset; a duplicate would silently shadow.
    #[test]
    fn tables_have_no_duplicate_offsets() {
        for lib in [
            Library::Exec,
            Library::Dos,
            Library::Graphics,
            Library::Intuition,
        ] {
            let mut seen = std::collections::HashSet::new();
            for (offset, name) in lib.table() {
                assert!(
                    seen.insert(*offset),
                    "{:?}: offset ${:x} listed twice ({})",
                    lib,
                    offset,
                    name
                );
                assert!(
                    offset.is_multiple_of(6),
                    "{:?}: offset ${:x} ({}) is not a multiple of 6",
                    lib,
                    offset,
                    name
                );
            }
        }
    }
}
