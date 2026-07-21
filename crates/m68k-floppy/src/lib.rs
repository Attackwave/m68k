//! Amiga floppy disk support (ADF, UAE, IPF, MFM).
//!
//! Port of the Python reference's `m68k_disasm/floppy/` package:
//! - [`adf`] — standard ADF images (`adf_backend.py`).
//! - [`uae`] — UAE extended ADF (raw per-track MFM bitstreams, `uae_backend.py`).
//! - [`ipf`] — native (clean-room) IPF chunk parser (`native_ipf_backend.py`).
//! - [`mfm`] — Amiga MFM bitstream decoder shared by `uae`/`ipf` (`mfm_decoder.py`).
//! - [`factory`] — extension/content-based backend auto-selection (`factory.py`).
//! - [`floppy_base`] — the [`floppy_base::FloppyImageReader`] trait and [`floppy_base::FloppyError`].
//!
//! Not ported: the proprietary `capsimg`-library-backed IPF backend
//! (Python's `caps_backend.py`). It requires FFI bindings to a closed-source
//! `capsimg.{dll,so}` that may not even be installed — Python's own factory
//! already falls back to the native IPF parser whenever it's absent. See
//! `plan.md`/B5 for details.

pub mod adf;
pub mod factory;
pub mod floppy_base;
pub mod ipf;
pub mod mfm;
pub mod uae;
