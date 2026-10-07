//! Motorola 68000 (m68k) Studio IDE Desktop Application.

#![allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]

pub mod amiga;
pub mod commands;
pub mod languages;
pub mod server;
pub mod workspace;

pub use server::{create_router, start_server};
