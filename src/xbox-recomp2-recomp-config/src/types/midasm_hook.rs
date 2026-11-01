//! Mid-assembly hooks.

use serde::Deserialize;

/// A mid-assembly hook.
///
/// Examples:
///
/// ```toml
/// [[midasm_hook]]
/// name = "OnLoad"
/// return = true
/// address = 0x8000
///
/// [[midasm_hook]]
/// name = "OnPoll"
/// return_on_true = false
/// jump_address_on_true = 0x8100
/// address = 0x8200
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct MidAsmHook {
    /// Hook function name, required.
    pub name: Box<str>,

    /// Ends the guest function right after the hook.
    ///
    /// ```c
    /// your_hook_function(...);
    /// return;  // 👈 return: return from hooked function
    /// // original code... never runs
    /// ```
    #[serde(default, rename = "return")]
    pub ret: bool,

    /// Ends the guest function when the hook returns true.
    ///
    /// ```c
    /// if (your_hook_function(...)) {
    ///     return;  // 👈 return_on_true: return from hooked function
    /// }
    /// else {
    ///     // return_on_false / jump_address_on_false if set
    /// }
    /// // original code...
    /// ```
    #[serde(default)]
    pub return_on_true: bool,

    /// Ends the guest function when the hook returns false.
    ///
    /// ```c
    /// if (your_hook_function(...)) {
    ///     // return_on_true / jump_address_on_true if set
    /// }
    /// else {
    ///     return;  // 👈 return_on_false: return from hooked function
    /// }
    /// // original code...
    /// ```
    #[serde(default)]
    pub return_on_false: bool,

    /// Resumes the guest at this address instead of the hooked instruction.
    ///
    /// ```c
    /// your_hook_function(...);
    /// goto <address>;  // 👈 jump_address: resume at that guest address
    /// // original code... never runs
    /// ```
    #[serde(default)]
    pub jump_address: u32,

    /// Resumes the guest at this address when the hook returns true.
    ///
    /// ```c
    /// if (your_hook_function(...)) {  // hook returned true
    ///     goto <address>;  // 👈 jump_address_on_true: resume here
    /// }
    /// else {
    ///     // return_on_false / jump_address_on_false if set
    /// }
    /// // original code...
    /// ```
    #[serde(default)]
    pub jump_address_on_true: u32,

    /// Resumes the guest at this address when the hook returns false.
    ///
    /// ```c
    /// if (your_hook_function(...)) {  // hook returned true
    ///     // return_on_true / jump_address_on_true if set
    /// }
    /// else {
    ///     goto <address>;  // 👈 jump_address_on_false: resume here
    /// }
    /// // original code...
    /// ```
    /// Zero: no jump in the else branch.
    #[serde(default)]
    pub jump_address_on_false: u32,
}
