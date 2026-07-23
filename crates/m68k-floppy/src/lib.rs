//! Amiga floppy disk support (ADF, UAE, IPF, MFM).
//!
//! - [`adf`] — standard ADF images.
//! - [`uae`] — UAE extended ADF (raw per-track MFM bitstreams).
//! - [`ipf`] — native (clean-room) IPF chunk parser.
//! - [`mfm`] — Amiga MFM bitstream decoder shared by `uae`/`ipf`.
//! - [`factory`] — extension/content-based backend auto-selection.
//! - [`floppy_base`] — the [`floppy_base::FloppyImageReader`] trait and [`floppy_base::FloppyError`].
//!
//! Not implemented: the proprietary `capsimg`-library-backed IPF backend. It
//! would require FFI bindings to a closed-source `capsimg.{dll,so}` that may
//! not even be installed — the native IPF parser already serves as a
//! fallback whenever it's absent.

pub mod adf;
pub mod factory;
pub mod floppy_base;
pub mod ipf;
pub mod mfm;
pub mod uae;
