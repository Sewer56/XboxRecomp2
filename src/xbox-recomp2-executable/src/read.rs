//! Helpers for reading bytes.

/// Read a NUL-terminated string at `offset`.
///
/// # Arguments
///
/// * `data` - where to read string from.
/// * `offset` - offset to first byte in `data`.
///
/// # Returns
///
/// The string from `offset` up to the:
/// - first NUL byte or
/// - the buffer end
///
/// NUL byte not included.
pub fn read_c_string(data: &[u8], offset: usize) -> String {
    let bytes = data.get(offset..).unwrap_or(&[]);
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}
