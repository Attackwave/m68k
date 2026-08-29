//! Motorola 68000 (m68k) Studio IDE Desktop Application.

pub mod commands;
pub mod server;

pub use server::{create_router, start_server};
