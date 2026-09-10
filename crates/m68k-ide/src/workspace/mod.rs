//! On-disk project model for m68k Studio.
//!
//! A project is a directory containing an `m68k.json` manifest, source
//! files and (after a build) a `build/` directory. This replaces the
//! previous browser-`localStorage` model, which could not support
//! multi-file assembly, `INCLUDE` resolution, an LSP workspace index or
//! version control.
//!
//! Every path crossing the API is validated against the open project
//! root by [`Project::resolve`] — the server has full filesystem access,
//! so a path arriving from the frontend is a trust boundary.

pub mod build;
pub mod manifest;
pub mod paths;

use std::path::{Path, PathBuf};

use serde::Serialize;

pub use manifest::{Manifest, OutputFormat};
pub use paths::PathError;

/// An opened project rooted at a directory containing `m68k.json`.
#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    manifest: Manifest,
}

/// One entry in the project file tree.
#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    /// Path relative to the project root, using `/` separators.
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

impl Project {
    /// Open the project rooted at `root`, reading its `m68k.json`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|e| format!("Projektverzeichnis nicht lesbar: {e}"))?;
        let manifest = Manifest::read_from(&root)?;
        Ok(Self { root, manifest })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Resolve a project-relative path to an absolute one, rejecting any
    /// path that escapes the project root.
    pub fn resolve(&self, relative: &str) -> Result<PathBuf, PathError> {
        paths::resolve_within(&self.root, relative)
    }

    /// The absolute path of the manifest's `main` source file.
    pub fn main_file(&self) -> Result<PathBuf, PathError> {
        self.resolve(&self.manifest.build.main)
    }

    /// Absolute include directories declared in the manifest, skipping
    /// any that do not exist so a stale entry cannot fail a build.
    pub fn include_dirs(&self) -> Vec<PathBuf> {
        self.manifest
            .build
            .include_dirs
            .iter()
            .filter_map(|d| self.resolve(d).ok())
            .filter(|p| p.is_dir())
            .collect()
    }

    /// The `build/` output directory, created on demand.
    pub fn build_dir(&self) -> Result<PathBuf, String> {
        let dir = self.root.join("build");
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("build/-Verzeichnis nicht anlegbar: {e}"))?;
        Ok(dir)
    }

    pub fn read_file(&self, relative: &str) -> Result<String, String> {
        let path = self.resolve(relative)?;
        std::fs::read_to_string(&path).map_err(|e| format!("{relative} nicht lesbar: {e}"))
    }

    /// Write a file, creating parent directories as needed.
    pub fn write_file(&self, relative: &str, contents: &str) -> Result<(), String> {
        let path = self.resolve(relative)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Verzeichnis für {relative} nicht anlegbar: {e}"))?;
        }
        std::fs::write(&path, contents).map_err(|e| format!("{relative} nicht schreibbar: {e}"))
    }

    /// Recursively list the project's files, skipping `build/`, `.git/`
    /// and other noise. Entries are sorted directories-first, then by
    /// name, so the frontend can render the tree without re-sorting.
    pub fn file_tree(&self) -> Result<Vec<FileEntry>, String> {
        let mut out = Vec::new();
        collect_entries(&self.root, &self.root, &mut out)?;
        Ok(out)
    }
}

/// Directory names never shown in the project tree.
fn is_ignored(name: &str) -> bool {
    matches!(
        name,
        "build" | ".git" | "node_modules" | "target" | ".DS_Store"
    )
}

fn collect_entries(root: &Path, dir: &Path, out: &mut Vec<FileEntry>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("Verzeichnis {} nicht lesbar: {e}", dir.display()))?
        .filter_map(Result::ok)
        .collect();

    // Directories first, then alphabetical — the order the tree renders in.
    entries.sort_by_key(|e| {
        let is_file = !e.path().is_dir();
        (is_file, e.file_name().to_string_lossy().to_lowercase())
    });

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_ignored(&name) {
            continue;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let is_dir = path.is_dir();

        out.push(FileEntry {
            path: rel_str,
            name,
            is_dir,
            size: if is_dir {
                0
            } else {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            },
        });

        if is_dir {
            collect_entries(root, &path, out)?;
        }
    }
    Ok(())
}
