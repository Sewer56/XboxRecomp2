//! Abstraction for parsing the raw bytes into an [`Executable`].
//!
//! Each [`ExecutableFormat`] does 2 things:
//! - Check magic header (file format check)
//! - Parse and load the executable
//!
//! # Usage
//!
//! Use [`parse_executable_with`].
//!
//! It tries each format in order, until one succeeds.
//!
//! (Adding a win32 implementation is an exercise to the reader 😉)

use crate::model::Executable;

/// A guest executable format the recompiler can load.
///
/// Includes:
/// - Magic header check ([`ExecutableFormat::detect`]), to verify file format
/// - Loader ([`ExecutableFormat::load`]) to parse it.
pub trait ExecutableFormat {
    /// The format's name.
    fn name(&self) -> &'static str;

    /// Returns `true` when `data` begins with this format's magic.
    fn detect(&self, data: &[u8]) -> bool;

    /// Loads `data` into an [`Executable`].
    fn load(&self, data: &[u8]) -> Executable;
}

/// Parses `data` with the first format in `formats` whose magic
/// matches.
///
/// # Arguments
///
/// * `formats` - formats to try, in order; the first match loads.
/// * `data` - raw input bytes to detect and load.
///
/// # Returns
///
/// The loaded executable, or an empty [`Executable`] when no format
/// matches.
pub fn parse_executable_with(formats: &[&dyn ExecutableFormat], data: &[u8]) -> Executable {
    formats
        .iter()
        .find(|format| format.detect(data))
        .map_or_else(Executable::default, |format| format.load(data))
}

#[cfg(test)]
mod tests {
    // This code is trivial, but it's technically the public API.
    // Don't want someone coming in with an LLM and breaking the API.
    // When I eventually drop this for the public.

    use super::*;
    use crate::address::VirtualAddress;

    /// Fake format for testing.
    ///
    /// Matches when the first byte is `magic`.
    struct ProbeFormat {
        magic: u8,
        base: u64,
    }

    impl ProbeFormat {
        fn new(magic: u8, base: u64) -> Self {
            Self { magic, base }
        }
    }

    impl ExecutableFormat for ProbeFormat {
        fn name(&self) -> &'static str {
            "probe"
        }

        fn detect(&self, data: &[u8]) -> bool {
            data.first() == Some(&self.magic)
        }

        fn load(&self, _data: &[u8]) -> Executable {
            Executable {
                base: self.base.into(),
                ..Executable::default()
            }
        }
    }

    #[test]
    fn first_matching_format_wins() {
        // Both match 'P', so the first one in the list should load.
        let first = ProbeFormat::new(b'P', 0x1000);
        let second = ProbeFormat::new(b'P', 0x2000);

        let executable = parse_executable_with(&[&first, &second], b"Payload");
        assert_eq!(executable.base, VirtualAddress::from(0x1000u64));
    }

    #[test]
    fn skips_formats_that_dont_match() {
        let miss = ProbeFormat::new(b'X', 0x1000);
        let hit = ProbeFormat::new(b'P', 0x2000);

        let executable = parse_executable_with(&[&miss, &hit], b"Payload");
        assert_eq!(executable.base, VirtualAddress::from(0x2000u64));
    }

    #[test]
    fn returns_empty_executable_when_nothing_matches() {
        let miss = ProbeFormat::new(b'X', 0x1000);

        let executable = parse_executable_with(&[&miss], b"Payload");
        assert_eq!(executable, Executable::default());
    }
}
