//! Workspace API: opening projects, the file tree, reading and writing
//! files, and the recent-projects list.
//!
//! This is what replaces the browser-`localStorage` project model. Every
//! request names a project root, which is resolved and containment-checked
//! before any file is touched.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::workspace::manifest::{BuildConfig, Manifest, OutputFormat, RunConfig};
use crate::workspace::{FileEntry, Project};

/// A project as the frontend sees it.
#[derive(Debug, Clone, Serialize)]
pub struct OpenProjectResponse {
    /// Absolute path of the project root.
    pub root: String,
    pub manifest: Manifest,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectPathRequest {
    pub root: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReadFileRequest {
    pub root: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WriteFileRequest {
    pub root: String,
    pub path: String,
    pub contents: String,
}

/// Open a project directory and return its manifest and file tree.
pub fn open_project(req: ProjectPathRequest) -> Result<OpenProjectResponse, String> {
    let project = Project::open(&req.root)?;
    let files = project.file_tree()?;
    let root = project.root().to_string_lossy().to_string();

    remember_recent(&root);

    Ok(OpenProjectResponse {
        root,
        manifest: project.manifest().clone(),
        files,
    })
}

/// Re-read the file tree, for after files change on disk.
pub fn list_files(req: ProjectPathRequest) -> Result<Vec<FileEntry>, String> {
    Project::open(&req.root)?.file_tree()
}

pub fn read_file(req: ReadFileRequest) -> Result<String, String> {
    Project::open(&req.root)?.read_file(&req.path)
}

pub fn write_file(req: WriteFileRequest) -> Result<(), String> {
    Project::open(&req.root)?.write_file(&req.path, &req.contents)
}

/// Build the project and return diagnostics plus the artifact path.
pub fn build(req: ProjectPathRequest) -> Result<crate::workspace::build::BuildResult, String> {
    let project = Project::open(&req.root)?;
    Ok(crate::workspace::build::build_project(&project))
}

// --- Recent projects -------------------------------------------------
//
// Kept server-side rather than in the browser: the list points at
// filesystem paths, so it belongs with the process that can read them,
// and it survives clearing site data.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentProject {
    pub root: String,
    pub name: String,
    /// Unix seconds when last opened.
    pub opened_at: u64,
}

/// Where IDE state that is not part of any project lives.
fn state_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".m68k-ide")
}

fn recents_file() -> PathBuf {
    state_dir().join("recent-projects.json")
}

/// The most recently opened projects, newest first, with entries whose
/// directory has since disappeared filtered out — a recent list that
/// offers dead paths is worse than a short one.
pub fn recent_projects() -> Vec<RecentProject> {
    let Ok(text) = std::fs::read_to_string(recents_file()) else {
        return Vec::new();
    };
    let mut list: Vec<RecentProject> = serde_json::from_str(&text).unwrap_or_default();
    list.retain(|r| {
        Path::new(&r.root)
            .join(crate::workspace::manifest::MANIFEST_NAME)
            .is_file()
    });
    list.sort_by_key(|r| std::cmp::Reverse(r.opened_at));
    list
}

/// How many recent projects to keep.
const MAX_RECENTS: usize = 12;

fn remember_recent(root: &str) {
    let name = Path::new(root)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut list = recent_projects();
    list.retain(|r| r.root != root);
    list.insert(
        0,
        RecentProject {
            root: root.to_string(),
            name,
            opened_at: now,
        },
    );
    list.truncate(MAX_RECENTS);

    let dir = state_dir();
    if std::fs::create_dir_all(&dir).is_ok()
        && let Ok(text) = serde_json::to_string_pretty(&list)
    {
        let _ = std::fs::write(recents_file(), text);
    }
}

// --- Creating projects -----------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct CreateProjectRequest {
    /// Directory to create the project in. Created if absent.
    pub root: String,
    pub name: String,
    /// Template id, e.g. `amiga-assembly`.
    pub template: String,
}

/// Create a new project on disk from a template and open it.
pub fn create_project(req: CreateProjectRequest) -> Result<OpenProjectResponse, String> {
    let root = PathBuf::from(&req.root);
    if root
        .join(crate::workspace::manifest::MANIFEST_NAME)
        .exists()
    {
        return Err(format!("In {} liegt bereits ein Projekt.", root.display()));
    }
    std::fs::create_dir_all(root.join("src"))
        .map_err(|e| format!("Projektverzeichnis nicht anlegbar: {e}"))?;

    let template = Template::for_id(&req.template);
    std::fs::create_dir_all(root.join("includes"))
        .map_err(|e| format!("includes/ nicht anlegbar: {e}"))?;

    for (rel, contents) in template.files() {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{rel} nicht anlegbar: {e}"))?;
        }
        std::fs::write(&path, contents).map_err(|e| format!("{rel} nicht schreibbar: {e}"))?;
    }

    let manifest = Manifest {
        name: req.name.trim().to_string(),
        version: "0.1.0".to_string(),
        profile: template.profile.to_string(),
        build: BuildConfig {
            main: template.main.to_string(),
            cpu: template.cpu.to_string(),
            format: template.format,
            origin: template.origin.to_string(),
            include_dirs: vec!["includes".to_string()],
            output_name: None,
        },
        run: RunConfig {
            emulator: template.emulator.to_string(),
            ..RunConfig::default()
        },
    };
    manifest.write_to(&root)?;

    // `.gitignore` from the start: build artifacts are not source, and a
    // project that dirties `git status` on first build feels unfinished.
    let _ = std::fs::write(root.join(".gitignore"), "/build/\n");

    open_project(ProjectPathRequest {
        root: root.to_string_lossy().to_string(),
    })
}

/// A project template.
struct Template {
    profile: &'static str,
    cpu: &'static str,
    main: &'static str,
    origin: &'static str,
    format: OutputFormat,
    emulator: &'static str,
    source: &'static str,
    include: Option<(&'static str, &'static str)>,
}

/// Templates offered by the new-project flow.
#[derive(Debug, Clone, Serialize)]
pub struct TemplateInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

pub fn list_templates() -> Vec<TemplateInfo> {
    vec![
        TemplateInfo {
            id: "amiga-assembly",
            name: "Amiga 500 — Assembly",
            description: "OCS-Demo mit Copperlist, baut eine bootfähige ADF.",
        },
        TemplateInfo {
            id: "amiga-hunk",
            name: "Amiga — AmigaOS-Programm",
            description: "Hunk-Executable für Workbench/CLI, startet über LoadSeg().",
        },
        TemplateInfo {
            id: "baremetal",
            name: "Bare Metal 68000",
            description: "Minimales Programm mit Vektortabelle, flaches Binary.",
        },
    ]
}

impl Template {
    fn for_id(id: &str) -> Self {
        match id {
            "amiga-hunk" => Self {
                profile: "amiga500",
                cpu: "68000",
                main: "src/main.s",
                origin: "$0",
                format: OutputFormat::Hunk,
                emulator: "fsuae",
                source: AMIGA_HUNK_SOURCE,
                include: None,
            },
            "baremetal" => Self {
                profile: "baremetal",
                cpu: "68000",
                main: "src/main.s",
                origin: "$1000",
                format: OutputFormat::Binary,
                emulator: "custom",
                source: BAREMETAL_SOURCE,
                include: None,
            },
            // Amiga assembly is the default: it is the platform this
            // IDE targets first.
            _ => Self {
                profile: "amiga500",
                cpu: "68000",
                main: "src/main.s",
                origin: "$0",
                format: OutputFormat::Adf,
                emulator: "fsuae",
                source: AMIGA_ASM_SOURCE,
                include: Some(("includes/custom.i", CUSTOM_INCLUDE)),
            },
        }
    }

    fn files(&self) -> Vec<(&'static str, &'static str)> {
        let mut v = vec![(self.main, self.source)];
        if let Some(inc) = self.include {
            v.push(inc);
        }
        v
    }
}

const CUSTOM_INCLUDE: &str = r#"; Amiga custom chip register offsets from CUSTOM_BASE.
CUSTOM_BASE     EQU $DFF000
DMACON          EQU $096
DMACONR         EQU $002
INTENA          EQU $09A
COP1LCH         EQU $080
COPJMP1         EQU $088
BPLCON0         EQU $100
COLOR00         EQU $180
"#;

const AMIGA_ASM_SOURCE: &str = r#"; Amiga 500 (OCS) copper raster demo.
; Builds to a bootable ADF: the bootblock loader reads this program
; from disk and enters it.

    SECTION Code,CODE

    INCLUDE "custom.i"

Start:
    lea     CUSTOM_BASE,a6
    move.w  #$7FFF,DMACON(a6)       ; all DMA off
    move.w  #$7FFF,INTENA(a6)       ; all interrupts off

    lea     CopperList(pc),a0
    move.l  a0,COP1LCH(a6)
    move.w  #0,COPJMP1(a6)          ; latch the new list
    move.w  #$8280,DMACON(a6)       ; DMA + copper on

MainLoop:
    btst    #6,$BFE001              ; left mouse button (CIAA PRA)
    bne.s   MainLoop

    move.w  #$8020,DMACON(a6)
    rts

    SECTION Data,DATA_C

CopperList:
    dc.w    BPLCON0,$0000           ; no bitplanes
    dc.w    COLOR00,$0002           ; dark blue
    dc.w    $8001,$FFFE             ; wait for line 128
    dc.w    COLOR00,$0F80           ; orange bar
    dc.w    $8801,$FFFE             ; wait for line 136
    dc.w    COLOR00,$0002
    dc.w    $FFFF,$FFFE             ; end of list
"#;

const AMIGA_HUNK_SOURCE: &str = r#"; AmigaOS program, started from Workbench or the CLI.
; Builds to a Hunk executable loaded by LoadSeg().

    SECTION Code,CODE

Start:
    move.l  $4.w,a6                 ; ExecBase
    lea     DosName(pc),a1
    moveq   #36,d0                  ; Kickstart 2.0+
    jsr     -552(a6)                ; OpenLibrary
    move.l  d0,d7
    beq.s   Done

    move.l  d7,a6
    jsr     -60(a6)                 ; Output()
    move.l  d0,d1
    lea     Message(pc),a0
    move.l  a0,d2
    moveq   #MsgEnd-Message,d3
    jsr     -48(a6)                 ; Write()

    move.l  $4.w,a6
    move.l  d7,a1
    jsr     -414(a6)                ; CloseLibrary

Done:
    moveq   #0,d0                   ; return code
    rts

    SECTION Data,DATA

DosName:
    dc.b    "dos.library",0
    even
Message:
    dc.b    "Hello from m68k Studio.",10
MsgEnd:
    even
"#;

const BAREMETAL_SOURCE: &str = r#"; Minimal bare-metal 68000 program.

    ORG     $1000

Start:
    moveq   #10,d0
    moveq   #0,d1

.loop:
    add.l   d0,d1
    subq.l  #1,d0
    bne.s   .loop

Halt:
    stop    #$2700
    bra.s   Halt
"#;
