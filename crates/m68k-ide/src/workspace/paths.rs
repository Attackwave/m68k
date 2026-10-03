//! Containment checking for paths arriving from the frontend.
//!
//! The IDE server runs as a local process with the user's full
//! filesystem rights, so a relative path in an API request is untrusted
//! input: without checking, `../../.ssh/id_rsa` would be readable and
//! writable through the file API. Every path crossing the API goes
//! through [`resolve_within`].

use std::fmt;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// The path pointed outside the project root.
    Escapes(String),
    /// The path was absolute, or carried a Windows prefix/root.
    NotRelative(String),
    /// The path was empty.
    Empty,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Escapes(p) => write!(f, "Pfad verlässt das Projektverzeichnis: {p}"),
            Self::NotRelative(p) => write!(f, "Pfad muss projektrelativ sein: {p}"),
            Self::Empty => write!(f, "Pfad ist leer"),
        }
    }
}

impl std::error::Error for PathError {}

impl From<PathError> for String {
    fn from(e: PathError) -> String {
        e.to_string()
    }
}

/// Join `relative` onto `root`, guaranteeing the result stays inside it.
///
/// The check is lexical — `..` components are resolved against the
/// accumulated path and a stack underflow is rejected — because the
/// target may not exist yet (a file being created), so `canonicalize`
/// is not available. Symlinks inside the project are therefore not
/// followed for the containment decision; a symlink pointing outside is
/// caught by [`verify_resolved`] when the file does exist.
pub fn resolve_within(root: &Path, relative: &str) -> Result<PathBuf, PathError> {
    if relative.trim().is_empty() {
        return Err(PathError::Empty);
    }

    let rel = Path::new(relative);
    let mut stack: Vec<std::ffi::OsString> = Vec::new();

    for component in rel.components() {
        match component {
            Component::Normal(part) => stack.push(part.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                // Popping past the root is exactly the escape we reject.
                if stack.pop().is_none() {
                    return Err(PathError::Escapes(relative.to_string()));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(PathError::NotRelative(relative.to_string()));
            }
        }
    }

    if stack.is_empty() {
        return Err(PathError::Empty);
    }

    let mut path = root.to_path_buf();
    for part in stack {
        path.push(part);
    }
    verify_resolved(root, path, relative)
}

/// Second check for paths that exist: resolve symlinks and confirm the
/// real target is still inside the root. A path that does not exist yet
/// passes — its parent chain was already checked lexically.
fn verify_resolved(root: &Path, path: PathBuf, relative: &str) -> Result<PathBuf, PathError> {
    let Ok(real) = path.canonicalize() else {
        return Ok(path);
    };
    let real_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if real.starts_with(&real_root) {
        Ok(path)
    } else {
        Err(PathError::Escapes(relative.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("/projects/demo")
    }

    #[test]
    fn accepts_plain_relative_paths() {
        let p = resolve_within(&root(), "src/main.s").unwrap();
        assert_eq!(p, PathBuf::from("/projects/demo/src/main.s"));
    }

    #[test]
    fn normalizes_curdir_and_interior_parent() {
        let p = resolve_within(&root(), "./src/../includes/custom.i").unwrap();
        assert_eq!(p, PathBuf::from("/projects/demo/includes/custom.i"));
    }

    #[test]
    fn rejects_escape_via_parent() {
        assert_eq!(
            resolve_within(&root(), "../../etc/passwd"),
            Err(PathError::Escapes("../../etc/passwd".into()))
        );
    }

    #[test]
    fn rejects_escape_that_returns_below_root() {
        // Descends then climbs past the root: the net effect leaves it.
        assert_eq!(
            resolve_within(&root(), "src/../../../etc/passwd"),
            Err(PathError::Escapes("src/../../../etc/passwd".into()))
        );
    }

    #[test]
    fn rejects_absolute_paths() {
        assert_eq!(
            resolve_within(&root(), "/etc/passwd"),
            Err(PathError::NotRelative("/etc/passwd".into()))
        );
    }

    #[test]
    fn rejects_empty_and_dot_only() {
        assert_eq!(resolve_within(&root(), ""), Err(PathError::Empty));
        assert_eq!(resolve_within(&root(), "."), Err(PathError::Empty));
    }
}
