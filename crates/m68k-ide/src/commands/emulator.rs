//! Emulator runner, auto-downloader, and launcher integration.

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatorProfile {
    pub id: String,
    pub name: String,
    pub executable_path: String,
    pub default_args: Vec<String>,
    pub supported_formats: Vec<String>,
    pub available: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LaunchEmulatorRequest {
    pub emulator_type: String, // "fsuae", "winuae", "blastem", "hatari", "custom"
    pub target_file_path: Option<String>,
    pub target_file_name: Option<String>,
    pub target_file_data_base64: Option<String>,
    pub custom_executable: Option<String>,
    pub kickstart_rom_path: Option<String>,
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchEmulatorResponse {
    pub success: bool,
    pub command_executed: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstallEmulatorRequest {
    pub emulator_id: String, // "blastem", "hatari", "emutos_rom", "aros_rom", "winuae", "fsuae"
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallEmulatorResponse {
    pub success: bool,
    pub executable_path: String,
    pub message: String,
}

fn get_app_emulators_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".m68k-ide").join("emulators")
}

fn check_executable_exists(name: &str) -> bool {
    let p = Path::new(name);
    if p.is_file() {
        return true;
    }

    #[cfg(target_os = "windows")]
    let check_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let check_cmd = "which";

    Command::new(check_cmd)
        .arg(name)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn find_file_recursive(dir: &Path, filename: &str) -> Option<PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Some(name) = path.file_name()
                && name.to_string_lossy().eq_ignore_ascii_case(filename)
            {
                return Some(path);
            } else if path.is_dir()
                && let Some(found) = find_file_recursive(&path, filename)
            {
                return Some(found);
            }
        }
    }
    None
}

pub fn detect_available_emulators() -> Vec<EmulatorProfile> {
    let emu_dir = get_app_emulators_dir();

    // 1. BlastEm
    let blastem_local = find_file_recursive(&emu_dir.join("blastem"), "blastem")
        .or_else(|| find_file_recursive(&emu_dir.join("blastem"), "blastem.exe"));
    let blastem_avail = check_executable_exists("blastem")
        || check_executable_exists("blastem.exe")
        || blastem_local.is_some();
    let blastem_exe = if let Some(ref p) = blastem_local {
        p.to_string_lossy().to_string()
    } else {
        "blastem".to_string()
    };

    // 2. Hatari
    let hatari_local = find_file_recursive(&emu_dir.join("hatari"), "hatari.AppImage")
        .or_else(|| find_file_recursive(&emu_dir.join("hatari"), "hatari"))
        .or_else(|| find_file_recursive(&emu_dir.join("hatari"), "hatari.exe"));
    let hatari_avail = check_executable_exists("hatari")
        || check_executable_exists("hatari.exe")
        || hatari_local.is_some();
    let hatari_exe = if let Some(ref p) = hatari_local {
        p.to_string_lossy().to_string()
    } else {
        "hatari".to_string()
    };

    // 3. FS-UAE
    let fsuae_local = find_file_recursive(&emu_dir.join("fsuae"), "fs-uae")
        .or_else(|| find_file_recursive(&emu_dir.join("fsuae"), "fs-uae.exe"));
    let fsuae_avail = check_executable_exists("fs-uae")
        || check_executable_exists("fs-uae.exe")
        || fsuae_local.is_some();
    let fsuae_exe = if let Some(ref p) = fsuae_local {
        p.to_string_lossy().to_string()
    } else {
        "fs-uae".to_string()
    };

    // 4. WinUAE
    let winuae_local = find_file_recursive(&emu_dir.join("winuae"), "winuae64.exe")
        .or_else(|| find_file_recursive(&emu_dir.join("winuae"), "winuae.exe"));
    let winuae_avail = check_executable_exists("winuae.exe")
        || check_executable_exists("winuae")
        || winuae_local.is_some();
    let winuae_exe = if let Some(ref p) = winuae_local {
        p.to_string_lossy().to_string()
    } else {
        "winuae".to_string()
    };

    vec![
        EmulatorProfile {
            id: "blastem".to_string(),
            name: "BlastEm (Sega Mega Drive)".to_string(),
            executable_path: blastem_exe,
            default_args: vec!["{FILE}".to_string()],
            supported_formats: vec!["bin".to_string(), "md".to_string(), "gen".to_string()],
            available: blastem_avail,
        },
        EmulatorProfile {
            id: "hatari".to_string(),
            name: "Hatari (Atari ST)".to_string(),
            executable_path: hatari_exe,
            default_args: vec!["--disk-a".to_string(), "{FILE}".to_string()],
            supported_formats: vec!["st".to_string(), "msa".to_string(), "bin".to_string()],
            available: hatari_avail,
        },
        EmulatorProfile {
            id: "fsuae".to_string(),
            name: "FS-UAE (Amiga)".to_string(),
            executable_path: fsuae_exe,
            default_args: vec!["--floppy_drive_0={FILE}".to_string()],
            supported_formats: vec!["adf".to_string(), "uae".to_string(), "ipf".to_string()],
            available: fsuae_avail,
        },
        EmulatorProfile {
            id: "winuae".to_string(),
            name: "WinUAE (Amiga)".to_string(),
            executable_path: winuae_exe,
            default_args: vec!["-f".to_string(), "{FILE}".to_string()],
            supported_formats: vec!["adf".to_string(), "uae".to_string(), "ipf".to_string()],
            available: winuae_avail,
        },
        EmulatorProfile {
            id: "custom".to_string(),
            name: "Custom / Anderer Emulator".to_string(),
            executable_path: "".to_string(),
            default_args: vec!["{FILE}".to_string()],
            supported_formats: vec!["*".to_string()],
            available: false,
        },
    ]
}

pub fn install_emulator_package(
    req: InstallEmulatorRequest,
) -> Result<InstallEmulatorResponse, String> {
    let base_dir = get_app_emulators_dir().join(&req.emulator_id);
    let _ = std::fs::create_dir_all(&base_dir);

    match req.emulator_id.as_str() {
        "blastem" => {
            #[cfg(target_os = "windows")]
            let url = "https://www.retrodev.com/blastem/blastem-win32-0.6.2.zip";
            #[cfg(not(target_os = "windows"))]
            let url = "https://www.retrodev.com/blastem/blastem64-0.6.2.tar.gz";

            let archive_name = if url.ends_with(".zip") {
                "blastem.zip"
            } else {
                "blastem.tar.gz"
            };
            let archive_path = base_dir.join(archive_name);

            let status = Command::new("curl")
                .arg("-sSL")
                .arg(url)
                .arg("-o")
                .arg(&archive_path)
                .status()
                .map_err(|e| format!("Download fehlgeschlagen (curl): {}", e))?;

            if !status.success() {
                return Err("Download von BlastEm fehlgeschlagen.".to_string());
            }

            if archive_name.ends_with(".tar.gz") {
                let _ = Command::new("tar")
                    .arg("-xzf")
                    .arg(&archive_path)
                    .arg("-C")
                    .arg(&base_dir)
                    .status();
            } else {
                let _ = Command::new("unzip")
                    .arg("-o")
                    .arg(&archive_path)
                    .arg("-d")
                    .arg(&base_dir)
                    .status();
            }

            #[cfg(target_os = "windows")]
            let target_exe = "blastem.exe";
            #[cfg(not(target_os = "windows"))]
            let target_exe = "blastem";

            let final_exe = find_file_recursive(&base_dir, target_exe).ok_or_else(|| {
                format!(
                    "BlastEm Binärdatei wurde nach dem Entpacken nicht gefunden in {:?}",
                    base_dir
                )
            })?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = std::fs::metadata(&final_exe) {
                    let mut perms = metadata.permissions();
                    perms.set_mode(0o755);
                    let _ = std::fs::set_permissions(&final_exe, perms);
                }
            }

            let exe_str = final_exe.to_string_lossy().to_string();
            Ok(InstallEmulatorResponse {
                success: true,
                executable_path: exe_str.clone(),
                message: format!(
                    "BlastEm erfolgreich heruntergeladen und eingebunden: {}",
                    exe_str
                ),
            })
        }
        "hatari" => {
            #[cfg(target_os = "windows")]
            {
                let url = "https://download.tuxfamily.org/hatari/2.5.0/hatari-2.5.0_windows.zip";
                let archive_path = base_dir.join("hatari.zip");
                let status = Command::new("curl")
                    .arg("-sSL")
                    .arg(url)
                    .arg("-o")
                    .arg(&archive_path)
                    .status()
                    .map_err(|e| format!("Download fehlgeschlagen (curl): {}", e))?;

                if !status.success() {
                    return Err("Download von Hatari fehlgeschlagen.".to_string());
                }

                let _ = Command::new("tar")
                    .arg("-xf")
                    .arg(&archive_path)
                    .arg("-C")
                    .arg(&base_dir)
                    .status();

                let final_exe = find_file_recursive(&base_dir, "hatari.exe")
                    .ok_or_else(|| format!("Hatari.exe wurde nicht gefunden in {:?}", base_dir))?;

                let exe_str = final_exe.to_string_lossy().to_string();
                Ok(InstallEmulatorResponse {
                    success: true,
                    executable_path: exe_str.clone(),
                    message: format!(
                        "Hatari erfolgreich heruntergeladen und eingebunden: {}",
                        exe_str
                    ),
                })
            }
            #[cfg(not(target_os = "windows"))]
            {
                let appimage_url = "https://github.com/pkgforge-dev/Hatari-AppImage/releases/download/2.6.1-1%402026-08-22_1787385232/Hatari-2.6.1-1-anylinux-x86_64.AppImage";
                let appimage_path = base_dir.join("hatari.AppImage");

                let status = Command::new("curl")
                    .arg("-sSL")
                    .arg(appimage_url)
                    .arg("-o")
                    .arg(&appimage_path)
                    .status()
                    .map_err(|e| format!("Download fehlgeschlagen (curl): {}", e))?;

                if !status.success() || !appimage_path.exists() {
                    return Err("Download von Hatari AppImage fehlgeschlagen.".to_string());
                }

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(metadata) = std::fs::metadata(&appimage_path) {
                        let mut perms = metadata.permissions();
                        perms.set_mode(0o755);
                        let _ = std::fs::set_permissions(&appimage_path, perms);
                    }
                }

                let exe_str = appimage_path.to_string_lossy().to_string();
                Ok(InstallEmulatorResponse {
                    success: true,
                    executable_path: exe_str.clone(),
                    message: format!(
                        "Hatari AppImage erfolgreich heruntergeladen und eingebunden: {}",
                        exe_str
                    ),
                })
            }
        }
        "emutos_rom" => {
            let url = "https://sourceforge.net/projects/emutos/files/emutos/1.3/emutos-512k-1.3.zip/download";
            let archive_path = base_dir.join("emutos.zip");

            let status = Command::new("curl")
                .arg("-sSL")
                .arg(url)
                .arg("-o")
                .arg(&archive_path)
                .status()
                .map_err(|e| format!("Download von EmuTOS fehlgeschlagen: {}", e))?;

            if !status.success() {
                return Err("Download von EmuTOS fehlgeschlagen.".to_string());
            }

            let _ = Command::new("unzip")
                .arg("-o")
                .arg(&archive_path)
                .arg("-d")
                .arg(&base_dir)
                .status();

            let rom_path = find_file_recursive(&base_dir, "etos512us.img")
                .or_else(|| find_file_recursive(&base_dir, "etos512de.img"))
                .ok_or_else(|| "EmuTOS ROM Image wurde im Archiv nicht gefunden.".to_string())?;

            let rom_str = rom_path.to_string_lossy().to_string();
            Ok(InstallEmulatorResponse {
                success: true,
                executable_path: rom_str.clone(),
                message: format!(
                    "EmuTOS Open-Source ROM erfolgreich heruntergeladen: {}",
                    rom_str
                ),
            })
        }
        "aros_rom" => {
            let rom_url =
                "https://github.com/tonioni/vAmiga/raw/master/vAmiga/ROMs/aros-amiga-m68k-rom.bin";
            let rom_path = base_dir.join("aros-amiga-m68k-rom.bin");

            let status = Command::new("curl")
                .arg("-sSL")
                .arg(rom_url)
                .arg("-o")
                .arg(&rom_path)
                .status()
                .map_err(|e| format!("Fehler beim Download des AROS Kickstart ROMs: {}", e))?;

            if !status.success() || !rom_path.exists() {
                return Err("Download des AROS Kickstart ROMs fehlgeschlagen.".to_string());
            }

            let rom_str = rom_path.to_string_lossy().to_string();
            Ok(InstallEmulatorResponse {
                success: true,
                executable_path: rom_str.clone(),
                message: format!(
                    "AROS Open-Source Kickstart ROM erfolgreich heruntergeladen: {}",
                    rom_str
                ),
            })
        }
        "winuae" => {
            let url = "https://download.abime.net/winuae/releases/WinUAE6030_x64.zip";
            let archive_path = base_dir.join("winuae64.zip");

            let status = Command::new("curl")
                .arg("-sSL")
                .arg(url)
                .arg("-o")
                .arg(&archive_path)
                .status()
                .map_err(|e| format!("Download fehlgeschlagen (curl): {}", e))?;

            if !status.success() {
                return Err("Download von WinUAE fehlgeschlagen.".to_string());
            }

            let _ = Command::new("unzip")
                .arg("-o")
                .arg(&archive_path)
                .arg("-d")
                .arg(&base_dir)
                .status();

            let final_exe = find_file_recursive(&base_dir, "winuae64.exe")
                .or_else(|| find_file_recursive(&base_dir, "winuae.exe"))
                .ok_or_else(|| "WinUAE Executable nicht im Archiv gefunden.".to_string())?;

            let exe_str = final_exe.to_string_lossy().to_string();
            Ok(InstallEmulatorResponse {
                success: true,
                executable_path: exe_str.clone(),
                message: format!(
                    "WinUAE erfolgreich heruntergeladen und entpackt: {}",
                    exe_str
                ),
            })
        }
        "fsuae" => {
            #[cfg(target_os = "windows")]
            let url =
                "https://fs-uae.net/files/FS-UAE/Stable/3.1.66/FS-UAE_3.1.66_Windows_x86-64.zip";
            #[cfg(not(target_os = "windows"))]
            let url =
                "https://fs-uae.net/files/FS-UAE/Stable/3.1.66/FS-UAE_3.1.66_Linux_x86-64.tar.xz";

            let archive_name = if url.ends_with(".zip") {
                "fs-uae.zip"
            } else {
                "fs-uae.tar.xz"
            };
            let archive_path = base_dir.join(archive_name);

            let status = Command::new("curl")
                .arg("-sSL")
                .arg(url)
                .arg("-o")
                .arg(&archive_path)
                .status()
                .map_err(|e| format!("Download fehlgeschlagen (curl): {}", e))?;

            if !status.success() {
                return Err("Download von FS-UAE fehlgeschlagen.".to_string());
            }

            if archive_name.ends_with(".tar.xz") {
                let _ = Command::new("tar")
                    .arg("-xf")
                    .arg(&archive_path)
                    .arg("-C")
                    .arg(&base_dir)
                    .status();
            } else {
                let _ = Command::new("unzip")
                    .arg("-o")
                    .arg(&archive_path)
                    .arg("-d")
                    .arg(&base_dir)
                    .status();
            }

            let final_exe = find_file_recursive(&base_dir, "fs-uae")
                .or_else(|| find_file_recursive(&base_dir, "fs-uae.exe"))
                .ok_or_else(|| {
                    "FS-UAE Binärdatei nach dem Entpacken nicht gefunden.".to_string()
                })?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = std::fs::metadata(&final_exe) {
                    let mut perms = metadata.permissions();
                    perms.set_mode(0o755);
                    let _ = std::fs::set_permissions(&final_exe, perms);
                }
            }

            let exe_str = final_exe.to_string_lossy().to_string();
            Ok(InstallEmulatorResponse {
                success: true,
                executable_path: exe_str.clone(),
                message: format!(
                    "FS-UAE erfolgreich heruntergeladen und eingebunden: {}",
                    exe_str
                ),
            })
        }
        _ => Err(format!(
            "Automatischer Download für '{}' wird noch nicht unterstützt. Bitte Pfad manuell in den Einstellungen angeben.",
            req.emulator_id
        )),
    }
}

pub fn launch_emulator(req: LaunchEmulatorRequest) -> Result<LaunchEmulatorResponse, String> {
    // 1. Determine or write target file to disk
    let target_path: PathBuf = if let Some(ref path_str) = req.target_file_path {
        PathBuf::from(path_str)
    } else if let Some(ref b64_data) = req.target_file_data_base64 {
        let file_bytes = base64::engine::general_purpose::STANDARD
            .decode(b64_data)
            .map_err(|e| format!("Invalid base64 target file data: {}", e))?;

        let file_name = req
            .target_file_name
            .unwrap_or_else(|| "build_output.adf".to_string());

        let out_dir = std::env::temp_dir().join("m68k_ide_build");
        let _ = std::fs::create_dir_all(&out_dir);
        let out_file = out_dir.join(&file_name);

        std::fs::write(&out_file, &file_bytes)
            .map_err(|e| format!("Failed to write emulator target file: {}", e))?;

        out_file
    } else {
        return Err("No target file or binary data provided to launch emulator".to_string());
    };

    let target_str = target_path.to_string_lossy().to_string();

    // 2. Resolve executable and arguments
    let mut args: Vec<String> = Vec::new();
    let exe = match req.emulator_type.as_str() {
        "fsuae" => {
            let e = req.custom_executable.as_deref().unwrap_or("fs-uae");
            args.push(format!("--floppy_drive_0={}", target_str));
            if let Some(ref rom) = req.kickstart_rom_path
                && !rom.is_empty()
            {
                args.push(format!("--kickstart_file={}", rom));
            }
            e
        }
        "winuae" => {
            let e = req.custom_executable.as_deref().unwrap_or("winuae");
            args.push("-f".to_string());
            args.push(target_str.clone());
            if let Some(ref rom) = req.kickstart_rom_path
                && !rom.is_empty()
            {
                args.push(format!("-s=kickstart_rom_file={}", rom));
            }
            e
        }
        "blastem" => {
            let e = req.custom_executable.as_deref().unwrap_or("blastem");
            args.push(target_str.clone());
            e
        }
        "hatari" => {
            let e = req.custom_executable.as_deref().unwrap_or("hatari");
            args.push("--disk-a".to_string());
            args.push(target_str.clone());
            if let Some(ref rom) = req.kickstart_rom_path
                && !rom.is_empty()
            {
                args.push("--tos".to_string());
                args.push(rom.clone());
            }
            e
        }
        _ => {
            let e = req.custom_executable.as_deref().unwrap_or("");
            if e.is_empty() {
                return Err("No executable path specified for custom emulator. Please configure it in Settings.".to_string());
            }
            args.push(target_str.clone());
            e
        }
    };

    // Add extra arguments and replace {FILE} placeholder
    if let Some(extras) = req.extra_args {
        for mut arg in extras {
            if arg.contains("{FILE}") {
                arg = arg.replace("{FILE}", &target_str);
            }
            args.push(arg);
        }
    }

    // Check if executable exists
    if !check_executable_exists(exe) {
        return Err(format!(
            "Emulator executable '{}' not found. Please verify the path in ⚙️ Settings & Profile.",
            exe
        ));
    }

    let mut cmd = Command::new(exe);
    for arg in &args {
        cmd.arg(arg);
    }

    let full_cmd_str = format!("{:?}", cmd);

    match cmd.spawn() {
        Ok(_) => Ok(LaunchEmulatorResponse {
            success: true,
            command_executed: full_cmd_str,
            message: format!(
                "Emulator '{}' launched successfully with {}",
                exe, target_str
            ),
        }),
        Err(e) => Err(format!(
            "Failed to spawn emulator '{}': {}. Please check your configuration in ⚙️ Settings.",
            exe, e
        )),
    }
}
