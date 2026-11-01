//! Re-exports executable types in one convenient place.

pub use xbox_recomp2_executable::Section;
pub use xbox_recomp2_executable::SectionFlags;
pub use xbox_recomp2_executable::Symbol;
pub use xbox_recomp2_executable::SymbolTable;
pub use xbox_recomp2_executable::SymbolType;
pub use xbox_recomp2_executable::{
    ConfigDefine, Executable, ExecutableFormat, parse_executable_with,
};
pub use xbox_recomp2_executable::{ImportMap, ImportToken, ThunkAddress};
pub use xbox_recomp2_recomp_config::SwitchTable;
