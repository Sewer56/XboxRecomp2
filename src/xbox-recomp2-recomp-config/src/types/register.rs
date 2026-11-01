//! Guest register names accepted in config files.

use core::error::Error;
use core::str::FromStr;
use serde::{Deserialize, de::Error as _};
use std::fmt;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// A 32-bit general-purpose guest register.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, EnumString, Display, EnumIter, IntoStaticStr,
)]
#[repr(u8)]
#[strum(serialize_all = "lowercase")]
pub enum Register {
    #[default]
    Eax = 0,

    Ebx = 1,
    Ecx = 2,
    Edx = 3,
    Esi = 4,
    Edi = 5,
    Ebp = 6,
    Esp = 7,
}

/// Error for a register index above 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterIndexError(pub u32);

impl Register {
    /// Returns the name used in config files.
    ///
    /// # Returns
    ///
    /// The lowercase name, such as `"eax"`.
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

impl TryFrom<u32> for Register {
    type Error = RegisterIndexError;

    /// Converts an index (number) to a register.
    ///
    /// # Errors
    ///
    /// Returns [`RegisterIndexError`] if `index` is above 7.
    fn try_from(index: u32) -> Result<Self, RegisterIndexError> {
        Ok(match index {
            0 => Self::Eax,
            1 => Self::Ebx,
            2 => Self::Ecx,
            3 => Self::Edx,
            4 => Self::Esi,
            5 => Self::Edi,
            6 => Self::Ebp,
            7 => Self::Esp,
            index => return Err(RegisterIndexError(index)),
        })
    }
}

impl<'de> Deserialize<'de> for Register {
    /// Accepts a general register name, such as `eax`.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Self::from_str(&name).map_err(|_| D::Error::custom(invalid_register_reason(&name)))
    }
}

impl fmt::Display for RegisterIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid register index: {}", self.0)
    }
}

impl Error for RegisterIndexError {}

/// Translates legacy XboxRecomp `rN` notation to `eax`, `ebx`, etc.
pub(crate) fn legacy_register_alias(name: &str) -> Option<&'static str> {
    let digits = name.strip_prefix('r')?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    let index: u32 = digits.parse().ok()?;
    Register::try_from(index).ok().map(Register::as_str)
}

/// Builds the error message for an unknown register name.
fn invalid_register_reason(name: &str) -> String {
    if let Some(alias) = legacy_register_alias(name) {
        format!("unknown register '{name}'; write '{alias}' instead")
    } else {
        format!("unknown register '{name}'; expected 'eax'..'esp'")
    }
}
