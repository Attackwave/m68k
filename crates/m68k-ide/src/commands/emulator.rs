//! Emulator runner and launcher integration.

use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatorProfile {
    pub name: String,
    pub executable_path: String,
    pub default_args: Vec<String>,
    pub supported_formats: Vec<String>,
    pub available: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LaunchEmulatorRequest {
    pub emulator_type: String, // "fsuae", "winuae", "blastem", "hatari", "custom"
    pub target_file_path: String,
    pub custom_executable: Option<String>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchEmulatorResponse {
    pub success: bool,
    pub command_executed: String,
    pub message: String,
}

fn check_executable_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

pub fn detect_available_emulators() -> Vec<EmulatorProfile> {
    vec![
        EmulatorProfile {
            name: "FS-UAE (Amiga)".to_string(),
            executable_path: "fs-uae".to_string(),
            default_args: vec!["--floppy_drive_0={FILE}".to_string()],
            supported_formats: vec!["adf".to_string(), "uae".to_string(), "ipf".to_string()],
            available: check_executable_exists("fs-uae"),
        },
        EmulatorProfile {
            name: "WinUAE (Amiga)".to_string(),
            executable_path: "winuae".to_string(),
            default_args: vec!["-f".to_string(), "{FILE}".to_string()],
            supported_formats: vec!["adf".to_string(), "uae".to_string(), "ipf".to_string()],
            available: check_executable_exists("winuae.exe") || check_executable_exists("winuae"),
        },
        EmulatorProfile {
            name: "BlastEm (Sega Mega Drive)".to_string(),
            executable_path: "blastem".to_string(),
            default_args: vec!["{FILE}".to_string()],
            supported_formats: vec!["bin".to_string(), "md".to_string(), "gen".to_string()],
            available: check_executable_exists("blastem"),
        },
        EmulatorProfile {
            name: "Hatari (Atari ST)".to_string(),
            executable_path: "hatari".to_string(),
            default_args: vec!["--disk-a".to_string(), "{FILE}".to_string()],
            supported_formats: vec!["st".to_string(), "msa".to_string()],
            available: check_executable_exists("hatari"),
        },
    ]
}

pub fn launch_emulator(req: LaunchEmulatorRequest) -> Result<LaunchEmulatorResponse, String> {
    let (exe, args) = match req.emulator_type.as_str() {
        "fsuae" => (
            req.custom_executable.as_deref().unwrap_or("fs-uae"),
            vec![format!("--floppy_drive_0={}", req.target_file_path)],
        ),
        "winuae" => (
            req.custom_executable.as_deref().unwrap_or("winuae"),
            vec!["-f".to_string(), req.target_file_path.clone()],
        ),
        "blastem" => (
            req.custom_executable.as_deref().unwrap_or("blastem"),
            vec![req.target_file_path.clone()],
        ),
        "hatari" => (
            req.custom_executable.as_deref().unwrap_or("hatari"),
            vec!["--disk-a".to_string(), req.target_file_path.clone()],
        ),
        _ => (
            req.custom_executable.as_deref().unwrap_or(""),
            vec![req.target_file_path.clone()],
        ),
    };

    if exe.is_empty() {
        return Err("No executable specified for custom emulator".to_string());
    }

    let mut cmd = Command::new(exe);
    for arg in args {
        cmd.arg(arg);
    }
    if let Some(extras) = req.extra_args {
        for arg in extras {
            cmd.arg(arg);
        }
    }

    let full_cmd_str = format!("{:?}", cmd);

    match cmd.spawn() {
        Ok(_) => Ok(LaunchEmulatorResponse {
            success: true,
            command_executed: full_cmd_str,
            message: "Emulator launched successfully".to_string(),
        }),
        Err(e) => Err(format!("Failed to spawn emulator process: {}", e)),
    }
}
