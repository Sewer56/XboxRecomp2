//! Parses `recompConfig.toml`, the recompiler's configuration file.

use core::error::Error;
use std::fs::read;
use std::io;
use std::path::Path;
pub use types::{
    AddressMap, FunctionAddress, FunctionSize, InvalidInstructionSize, MidAsmHook, Optimizations,
    RecompilerConfig, Register, RegisterIndexError, SpeedHacks, SwitchTable,
};

mod parse;
mod types;

/// Errors produced while loading a recompiler config.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The config file could not be read.
    #[error("{path}: {source}")]
    Io {
        /// Path that failed to read.
        path: Box<str>,
        /// Underlying I/O failure.
        source: io::Error,
    },

    /// The config file is not valid UTF-8 or not valid TOML.
    #[error("{path}: {source}")]
    Parse {
        /// Path that failed to parse.
        path: Box<str>,
        /// Underlying parse failure.
        source: Box<dyn Error + Send + Sync>,
    },

    /// The config file does not match the config schema; the message
    /// carries the source position.
    ///
    /// - A required field is missing.
    /// - A value has the wrong type.
    /// - A number is out of range, like a switch base beyond 32 bits.
    /// - A register name is unknown; the message names the fix.
    /// - An unknown key appears in `[optimizations]` or `[speed_hacks]`.
    #[error("{path}: {source}")]
    Schema {
        /// Path of the file being parsed.
        path: Box<str>,
        /// Underlying deserialisation failure.
        source: Box<dyn Error + Send + Sync>,
    },

    /// A removed or relocated key is present.
    /// Basically stuff from original XboxRecomp that's no longer supported.
    #[error("{path}: {field}: {advice}")]
    DeprecatedKey {
        /// Path of the file being parsed.
        path: Box<str>,
        /// Path of the offending key, e.g. `main.fast_trig`.
        field: Box<str>,
        /// What to write instead.
        advice: &'static str,
    },
}

/// A successfully loaded config and any warnings..
#[derive(Debug, Clone, Default)]
pub struct LoadedConfig {
    /// The parsed configuration.
    pub config: RecompilerConfig,

    /// Non-fatal warning messages.
    pub warnings: Vec<Box<str>>,
}

/// Derives the config's directory prefix from a config file path.
///
/// # Arguments
///
/// - `config_path`: path of the config file.
///
/// # Returns
///
/// - Everything up to and including the last `/` of `config_path`.
/// - An empty string when the path contains no `/`.
pub fn directory_of(config_path: &str) -> &str {
    match config_path.rfind('/') {
        Some(index) => &config_path[..=index],
        None => "",
    }
}

/// Loads a recompiler config from `config_path`.
///
/// # Arguments
///
/// - `config_path`: path of the config file to read and parse.
///
/// # Returns
///
/// The [`LoadedConfig`] with the parsed config and any `warnings`.
///
/// # Errors
///
/// Returns:
/// - [`ConfigError::DeprecatedKey`] when a legacy XboxRecomp key is used.
/// - [`ConfigError::Io`] when the config file cannot be read.
/// - [`ConfigError::Parse`] when the config file is not valid UTF-8 / TOML.
/// - [`ConfigError::Schema`] when the document does not match the
///   config schema:
///   - a required field is missing;
///   - a value has the wrong type;
///   - a number is out of range;
///   - a register name is unknown
///   - an unknown key appears in `[optimizations]` or `[speed_hacks]`.
pub fn load(config_path: &Path) -> Result<LoadedConfig, ConfigError> {
    let path: Box<str> = config_path.to_string_lossy().into_owned().into();
    let bytes = read(config_path).map_err(|source| ConfigError::Io {
        path: path.clone(),
        source,
    })?;
    parse::load_bytes(&path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::env::temp_dir;
    use std::fs;
    use std::process::id;

    // ── Loading ──────────────────────────────────────────────────────────────

    /// `load()` reads the config from disk and parses inlined `[[switch]]`
    /// entries into the run's switch tables.
    #[test]
    fn load_reads_config_and_switch_entries_from_disk() {
        let dir = temp_dir().join(format!("xboxrecomp2-config-{}", id()));
        fs::create_dir_all(&dir).expect("create temp dir");

        let config_path = dir.join("recomp.toml");
        fs::write(
            &config_path,
            indoc! {r#"
                [main]
                file_path = "game.bin"

                [fixes]
                functions = [{ address = 0x1000, size = 0x50 }]

                [[switch]]
                register = "ebx"
                base = 0x1000
                labels = [0x10]
            "#},
        )
        .expect("write config");

        let loaded = load(&config_path).unwrap();
        let config = loaded.config;

        assert_eq!(
            &*config.directory_path,
            format!("{}/", dir.to_string_lossy())
        );
        assert_eq!(&*config.file_path, "game.bin");
        assert_eq!(
            config.functions.get(0x1000.into()),
            Some(&FunctionSize(0x50))
        );
        assert_eq!(
            config.switch_tables.get(0x1000.into()).unwrap().labels,
            vec![0x10],
            "switch entries load from the config document itself"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_reports_missing_config_file() {
        let result = load(Path::new("absent/recomp.toml"));
        assert!(matches!(result, Err(ConfigError::Io { .. })));
    }

    // ── Convenience ──────────────────────────────────────────────────────────

    #[test]
    fn directory_of_keeps_trailing_separator() {
        assert_eq!(directory_of("a/b/c.toml"), "a/b/");
        assert_eq!(directory_of("c.toml"), "");
        assert_eq!(directory_of("/abs/path.toml"), "/abs/");
    }
}
