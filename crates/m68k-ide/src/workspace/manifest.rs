//! The project manifest (`m68k.json`).
//!
//! JSON rather than TOML deliberately: `serde_json` is already a
//! workspace dependency, a TOML parser would be a new one, and the
//! frontend can read and write this file without a parser of its own.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Manifest file name, at the project root.
pub const MANIFEST_NAME: &str = "m68k.json";

/// What the build produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// AmigaOS Hunk executable, loadable by `LoadSeg()` — the normal
    /// artifact for an Amiga program run under Workbench or CLI.
    Hunk,
    /// Bootable 880 KB ADF, entered by the Kickstart bootblock.
    Adf,
    /// Flat binary at the origin address (bare metal, Mega Drive ROM).
    Binary,
    SRecord,
    IntelHex,
}

impl OutputFormat {
    /// The file extension for this format's artifact.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Hunk => "",
            Self::Adf => "adf",
            Self::Binary => "bin",
            Self::SRecord => "s68",
            Self::IntelHex => "hex",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Project-relative path of the entry source file.
    pub main: String,
    #[serde(default = "default_cpu")]
    pub cpu: String,
    pub format: OutputFormat,
    /// Origin address as a string so `$` and `0x` forms both survive a
    /// round-trip through the manifest.
    #[serde(default = "default_origin")]
    pub origin: String,
    #[serde(default)]
    pub include_dirs: Vec<String>,
    /// Artifact base name; defaults to the project name.
    #[serde(default)]
    pub output_name: Option<String>,
}

fn default_cpu() -> String {
    "68000".to_string()
}

fn default_origin() -> String {
    "$0".to_string()
}

/// How `F5` runs the built artifact.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunConfig {
    /// Emulator id: `fsuae`, `winuae`, `blastem`, `hatari`, `custom`.
    #[serde(default)]
    pub emulator: String,
    #[serde(default)]
    pub kickstart_rom: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    /// Platform profile id, e.g. `amiga500`.
    pub profile: String,
    pub build: BuildConfig,
    #[serde(default)]
    pub run: RunConfig,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

impl Manifest {
    /// Read and parse `m68k.json` from a project root.
    pub fn read_from(root: &Path) -> Result<Self, String> {
        let path = root.join(MANIFEST_NAME);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("Kein {MANIFEST_NAME} in {}: {e}", root.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{MANIFEST_NAME} ist ungültig: {e}"))
    }

    /// Write `m68k.json` to a project root.
    pub fn write_to(&self, root: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("{MANIFEST_NAME} nicht serialisierbar: {e}"))?;
        std::fs::write(root.join(MANIFEST_NAME), text)
            .map_err(|e| format!("{MANIFEST_NAME} nicht schreibbar: {e}"))
    }

    /// Parse the origin address, accepting `$1000`, `0x1000` and plain
    /// decimal — all three appear in real 68k sources.
    pub fn origin_address(&self) -> Result<u32, String> {
        parse_address(&self.build.origin)
    }

    /// Artifact base name, falling back to the project name.
    pub fn output_name(&self) -> &str {
        self.build.output_name.as_deref().unwrap_or(&self.name)
    }
}

/// Parse an address in `$hex`, `0xhex` or decimal notation.
pub fn parse_address(text: &str) -> Result<u32, String> {
    let t = text.trim();
    let (digits, radix) = if let Some(hex) = t.strip_prefix('$') {
        (hex, 16)
    } else if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        (hex, 16)
    } else {
        (t, 10)
    };
    u32::from_str_radix(digits.replace('_', "").as_str(), radix)
        .map_err(|_| format!("Ungültige Adresse: {text}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_address_notations() {
        assert_eq!(parse_address("$1000").unwrap(), 0x1000);
        assert_eq!(parse_address("0xDFF000").unwrap(), 0xDFF000);
        assert_eq!(parse_address("4096").unwrap(), 4096);
        assert_eq!(parse_address(" $FF_FF ").unwrap(), 0xFFFF);
        assert!(parse_address("nonsense").is_err());
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let m = Manifest {
            name: "demo".into(),
            version: "1.0.0".into(),
            profile: "amiga500".into(),
            build: BuildConfig {
                main: "src/main.s".into(),
                cpu: "68000".into(),
                format: OutputFormat::Adf,
                origin: "$0".into(),
                include_dirs: vec!["includes".into()],
                output_name: None,
            },
            run: RunConfig::default(),
        };
        let text = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(back.build.format, OutputFormat::Adf);
        assert_eq!(back.output_name(), "demo");
    }

    #[test]
    fn defaults_fill_in_for_a_minimal_manifest() {
        let text = r#"{
            "name": "tiny",
            "profile": "baremetal",
            "build": { "main": "src/main.s", "format": "binary" }
        }"#;
        let m: Manifest = serde_json::from_str(text).unwrap();
        assert_eq!(m.build.cpu, "68000");
        assert_eq!(m.version, "0.1.0");
        assert!(m.build.include_dirs.is_empty());
    }
}
