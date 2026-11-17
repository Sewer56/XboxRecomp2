//! C ABI bridge for XboxRecomp2.
//!
//! Keep in sync with `include/xbox_recomp2/xbox_recomp2.h`.

use core::ffi::{CStr, c_char, c_int, c_void};
use std::ffi::{CString};
use std::io::{self, Write};
use std::path::Path;
use xbox_recomp2::{LoadError, RunError};
use xbox_recomp2_executable::Executable;

// Diagnostic Output callback.
pub type XbDiagFn = Option<unsafe extern "C" fn(data: *const u8, len: usize, user: *mut c_void)>;

// Mirrors the variants of [`RunError`]. Keep in sync with the header.
pub const XB_OK: c_int = 0;
pub const XB_ERR_LOAD: c_int = 1;
pub const XB_ERR_NO_DERIVABLE_FUNCTIONS: c_int = 2;
pub const XB_ERR_MISSING_ENTRY_POINT: c_int = 3;
pub const XB_ERR_OUTPUT_PATH_IS_SYMLINK: c_int = 4;
pub const XB_ERR_IO: c_int = 5;
// Bad argument, e.g. null or non-UTF-8 path.
pub const XB_ERR_INVALID_ARGUMENT: c_int = 6;
// A Rust panic was caught (only possible when built with `panic = "unwind"`)
pub const XB_ERR_PANIC: c_int = 7;

/// Error details. Free with [`xb_error_free`].
#[repr(C)]
pub struct XbRunError {
    /// One of the `XB_ERR_*` codes, or [`XB_OK`].
    pub code: c_int,
    /// Whether `address` holds a value.
    pub has_address: bool,
    /// Guest entry point address, for [`XB_ERR_MISSING_ENTRY_POINT`].
    pub address: u64,
    /// Associated path, or null. Owned
    pub path: *mut c_char,
    /// Full error text, or null. Owned
    pub message: *mut c_char,
}

impl XbRunError {
    const fn ok() -> Self {
        Self {
            code: XB_OK,
            has_address: false,
            address: 0,
            path: core::ptr::null_mut(),
            message: core::ptr::null_mut(),
        }
    }

    fn new(code: c_int, path: Option<&Path>, message: &str) -> Self {
        Self {
            code,
            has_address: false,
            address: 0,
            path: path.map_or(core::ptr::null_mut(), |p| to_c_string(&p.to_string_lossy())),
            message: to_c_string(message),
        }
    }

    fn from_run_error(err: &RunError) -> Self {
        let message = err.to_string();
        match err {
            RunError::Load(LoadError::Io { path, .. } | LoadError::EmptyInput { path }) => {
                Self::new(XB_ERR_LOAD, Some(path), &message)
            }
            RunError::Load(LoadError::Config(_)) => Self::new(XB_ERR_LOAD, None, &message),
            RunError::NoDerivableFunctions { path } => {
                Self::new(XB_ERR_NO_DERIVABLE_FUNCTIONS, Some(path), &message)
            }
            RunError::MissingEntryPoint { path, address } => Self {
                has_address: true,
                address: address.get(),
                ..Self::new(XB_ERR_MISSING_ENTRY_POINT, Some(path), &message)
            },
            RunError::Out(out) => match out {
                xbox_recomp2::OutError::OutputPathIsSymlink { path } => {
                    Self::new(XB_ERR_OUTPUT_PATH_IS_SYMLINK, Some(path), &message)
                }
                xbox_recomp2::OutError::Io { path, .. } => {
                    Self::new(XB_ERR_IO, Some(path), &message)
                }
            },
        }
    }
}

/// Makes a C string, dropping any NULs.
fn to_c_string(s: &str) -> *mut c_char {
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != 0).collect();
    CString::new(bytes).map_or(core::ptr::null_mut(), CString::into_raw)
}

struct CallbackWriter {
    callback: XbDiagFn,
    user: *mut c_void,
}

impl Write for CallbackWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(callback) = self.callback {
            // SAFETY: `xb_run` requires `callback` to be callable with `user`,
            // and this writer doesn't outlive that call.
            unsafe { callback(buf.as_ptr(), buf.len(), self.user) };
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Writes `err` to `out` if `out` isn't null.
///
/// # Safety
///
/// `out` must be null or valid for writes.
unsafe fn report(out: *mut XbRunError, err: XbRunError) -> c_int {
    let code = err.code;
    if !out.is_null() {
        // SAFETY: non-null, and valid for writes per `# Safety`.
        unsafe { out.write(err) };
    }
    code
}

/// Runs the recompiler on the config at `input`.
///
/// # Arguments
///
/// * `input` - NUL-terminated UTF-8 path to the config TOML.
/// * `diag` - receives progress text. May be null.
/// * `user` - passed back to `diag`.
/// * `out_error` - receives error details if this returns nonzero. May be null.
///   Release with [`xb_error_free`].
///
/// # Returns
///
/// [`XB_OK`] on success, else an `XB_ERR_*` code.
///
/// # Safety
///
/// `input` must be null or a valid C string. `diag` must be callable with
/// `user` for the duration of the call. `out_error` must be null or valid
/// for writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_run(
    input: *const c_char,
    diag: XbDiagFn,
    user: *mut c_void,
    out_error: *mut XbRunError,
) -> c_int {
    if !out_error.is_null() {
        // SAFETY: non-null, and valid for writes per `# Safety`.
        unsafe { out_error.write(XbRunError::ok()) };
    }

    if input.is_null() {
        // SAFETY: `out_error` is null or valid for writes per `# Safety`.
        return unsafe {
            report(
                out_error,
                XbRunError::new(XB_ERR_INVALID_ARGUMENT, None, "input path is null"),
            )
        };
    }

    // SAFETY: non-null, and a valid C string for the whole call per `# Safety`.
    let Ok(path) = unsafe { CStr::from_ptr(input) }.to_str() else {
        // SAFETY: `out_error` is null or valid for writes per `# Safety`.
        return unsafe {
            report(
                out_error,
                XbRunError::new(XB_ERR_INVALID_ARGUMENT, None, "input path is not UTF-8"),
            )
        };
    };

    let mut writer = CallbackWriter {
        callback: diag,
        user,
    };

    // Unwinding into C++ is UB. Release builds use `panic = "abort"`, so this only matters on dev builds
    let result = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
        xbox_recomp2_xbox::run(Path::new(path), &mut writer)
    }));

    match result {
        Ok(Ok(())) => XB_OK,
        // SAFETY: `out_error` is null or valid for writes per `# Safety`.
        Ok(Err(err)) => unsafe { report(out_error, XbRunError::from_run_error(&err)) },
        // SAFETY: same as above.
        Err(_) => unsafe {
            report(
                out_error,
                XbRunError::new(XB_ERR_PANIC, None, "recompiler panicked"),
            )
        },
    }
}

/// Frees the strings inside `err`, and resets it.
///
/// # Safety
///
/// `err` must be null or an [`XbRunError`] filled by [`xb_run`] and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_error_free(err: *mut XbRunError) {
    if err.is_null() {
        return;
    }

    // SAFETY: non-null, and points to an `XbRunError` filled by `xb_run` per `# Safety`.
    let e = unsafe { &mut *err };

    if !e.path.is_null() {
        // SAFETY: non-null, from `to_c_string` (`CString::into_raw`), not yet freed.
        drop(unsafe { CString::from_raw(e.path) });
    }

    if !e.message.is_null() {
        // SAFETY: same as `e.path`.
        drop(unsafe { CString::from_raw(e.message) });
    }
    *e = XbRunError::ok();
}

/// Opaque handle to a parsed [`Executable`].
pub struct XbExecutable(Executable);

/// A section as seen from C. `name` is NOT NUL-terminated, and is only valid
/// while the [`XbExecutable`] lives.
#[repr(C)]
pub struct XbSection {
    pub name: *const c_char,
    pub name_len: usize,
    pub base: u64,
    pub size: u32,
    pub raw_address: u32,
    pub raw_size: u32,
}

/// Parses an Xbox executable (XBE).
///
/// # Returns
///
/// A handle to free with [`xb_executable_free`], or null if `data` is null.
/// A failed parse gives a handle to an empty executable. Check with [`xb_executable_is_empty`].
///
/// # Safety
///
/// `data` must be null or valid for reads of `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_parse_executable(data: *const u8, len: usize) -> *mut XbExecutable {
    if data.is_null() {
        return core::ptr::null_mut();
    }

    // SAFETY: non-null, and valid for reads of `len` bytes per `# Safety`.
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };
    let result = std::panic::catch_unwind(|| xbox_recomp2_xbox::parse_executable(bytes));
    match result {
        Ok(exe) => Box::into_raw(Box::new(XbExecutable(exe))),
        Err(_) => core::ptr::null_mut(),
    }
}

/// Frees a handle from [`xb_parse_executable`].
///
/// # Safety
///
/// `exe` must be null or a live handle, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_executable_free(exe: *mut XbExecutable) {
    if !exe.is_null() {
        // SAFETY: non-null, and a live handle from `Box::into_raw` in
        // `xb_parse_executable`, not freed before, per `# Safety`.
        drop(unsafe { Box::from_raw(exe) });
    }
}

/// Whether or not the executable is empty. A null handle is considered empty.
///
/// # Safety
///
/// `exe` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_executable_is_empty(exe: *const XbExecutable) -> bool {
    // SAFETY: `exe` is null or a live handle per `# Safety`.
    unsafe { exe.as_ref() }.is_none_or(|e| e.0 == Executable::default())
}

/// Guest address the executable is loaded at. Returns 0 for a null handle.
///
/// # Safety
///
/// `exe` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_executable_base(exe: *const XbExecutable) -> u64 {
    // SAFETY: `exe` is null or a live handle per `# Safety`.
    unsafe { exe.as_ref() }.map_or(0, |e| e.0.base.get())
}

/// Guest address of the entry point. Returns 0 for a null handle.
///
/// # Safety
///
/// `exe` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_executable_entry_point(exe: *const XbExecutable) -> u64 {
    // SAFETY: `exe` is null or a live handle per `# Safety`.
    unsafe { exe.as_ref() }.map_or(0, |e| e.0.entry_point.get())
}

/// Input file size in bytes. 0 for null.
///
/// # Safety
///
/// `exe` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_executable_size(exe: *const XbExecutable) -> u32 {
    // SAFETY: `exe` is null or a live handle per `# Safety`.
    unsafe { exe.as_ref() }.map_or(0, |e| e.0.size)
}

/// Size of the image in guest memory. 0 for null.
///
/// # Safety
///
/// `exe` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_executable_size_of_image(exe: *const XbExecutable) -> u64 {
    // SAFETY: `exe` is null or a live handle per `# Safety`.
    unsafe { exe.as_ref() }.map_or(0, |e| e.0.size_of_image)
}

/// Number of sections. Returns 0 for a null handle.
///
/// # Safety
///
/// `exe` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_executable_section_count(exe: *const XbExecutable) -> usize {
    // SAFETY: `exe` is null or a live handle per `# Safety`.
    unsafe { exe.as_ref() }.map_or(0, |e| e.0.sections.len())
}

/// Fills `out` with section `index`.
///
/// # Returns
///
/// `true` on success, `false` if `exe`/`out` is null or `index` is out of range.
///
/// # Safety
///
/// `exe` must be null or a live handle. `out` must be null or valid for writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xb_executable_section(
    exe: *const XbExecutable,
    index: usize,
    out: *mut XbSection,
) -> bool {
    // SAFETY: `exe` is null or a live handle per `# Safety`.
    let Some(section) = (unsafe { exe.as_ref() }).and_then(|e| e.0.sections.get(index)) else {
        return false;
    };

    if out.is_null() {
        return false;
    }

    // SAFETY: non-null, and valid for writes per `# Safety`.
    unsafe {
        out.write(XbSection {
            name: section.name.as_ptr().cast(),
            name_len: section.name.len(),
            base: section.base.get(),
            size: section.size,
            raw_address: section.raw_address,
            raw_size: section.raw_size,
        });
    }
    true
}
