//! The recompiler's model of a guest executable.
//!
//! tl;dr, an abstraction so I could add Windows or other x86 binary
//! support if I ever feel like it.
//!
//! ---------
//!
//! Look at e.g. `xbox-recomp2-xbox` for an example implementation.
//! But it looks something like:
//!
//! ```rust
//! use xbox_recomp2_executable::{Executable, ExecutableFormat};
//!
//! struct AssFormat;
//!
//! impl ExecutableFormat for AssFormat {
//!     fn name(&self) -> &'static str {
//!         "ass" // cause this format is ass.
//!     }
//!
//!     fn detect(&self, data: &[u8]) -> bool {
//!         data.starts_with(b"ASS")
//!     }
//!
//!     fn load(&self, data: &[u8]) -> Executable {
//!         Executable {
//!             size: data.len() as u32,
//!             ..Executable::default()
//!         }
//!     }
//! }
//!
//! let formats: [&dyn ExecutableFormat; 1] = [&AssFormat];
//! let executable = xbox_recomp2_executable::parse_executable_with(&formats, b"ASS payload");
//! assert_eq!(executable.size, 11);
//! ```

pub use address::{AddressWidth, GuestAddress32, GuestAddress64, VirtualAddress};
pub use format::{ExecutableFormat, parse_executable_with};
pub use imports::{ImportMap, ImportToken, ThunkAddress};
pub use model::{ConfigDefine, Executable};
pub use read::read_c_string;
pub use section::{Section, SectionFlags};
pub use symbol::{Symbol, SymbolType};
pub use symbol_table::SymbolTable;

mod address;
mod format;
mod imports;
mod model;
mod read;
mod section;
mod symbol;
mod symbol_table;
