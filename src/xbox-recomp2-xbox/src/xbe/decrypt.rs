//! Decodes the entry point and kernel thunk table address in an XBE header.
//!
//! The header stores both addresses XORed with a fixed key.
//! Debug and retail builds use different keys, so we try both.

use xbox_recomp2_executable::VirtualAddress;

/// Debug key for the entry point.
const DEBUG_ENTRY_KEY: u32 = 0x94859D4B;
/// Debug key for the kernel thunk (import) table address.
const DEBUG_THUNK_KEY: u32 = 0xEFB1F152;
/// Retail key for the entry point.
const RETAIL_ENTRY_KEY: u32 = 0xA8FC57AB;
/// Retail key for the kernel thunk (import) table address.
const RETAIL_THUNK_KEY: u32 = 0x5B6D40B6;

/// Decodes the entry point from the XBE header.
///
/// # Arguments
///
/// * `encoded` - the encoded entry point from the header.
/// * `max_size` - size of the XBE file in bytes.
///
/// # Returns
///
/// The decoded entry point, or zero if neither key gives a usable address.
///
/// # Remarks
///
/// We try the debug key first, then the retail key.
/// A result is usable if it's below `max_size - 1000`.
///
/// For files under 1000 bytes, that subtraction wraps to a huge limit.
/// So we take almost any debug result, even a nonsense one.
pub(super) fn entry_point(encoded: u32, max_size: u32) -> VirtualAddress<u32> {
    with_keys(encoded, max_size, DEBUG_ENTRY_KEY, RETAIL_ENTRY_KEY).into()
}

/// Decodes the kernel thunk table address from the XBE header.
///
/// # Arguments
///
/// * `encoded` - the encoded kernel thunk table address from the header.
/// * `max_size` - size of the XBE file in bytes.
///
/// # Returns
///
/// The decoded address, or zero if neither key gives a usable address.
///
/// # Remarks
///
/// We try the debug key first, then the retail key.
/// A result is usable if it's below `max_size - 1000`.
///
/// For files under 1000 bytes, that subtraction wraps to a huge limit.
/// So we take almost any debug result, even a nonsense one.
pub(super) fn kernel_thunk_addr(encoded: u32, max_size: u32) -> VirtualAddress<u32> {
    with_keys(encoded, max_size, DEBUG_THUNK_KEY, RETAIL_THUNK_KEY).into()
}

/// Shared logic for [`entry_point`] and [`kernel_thunk_addr`].
fn with_keys(encoded: u32, max_size: u32, debug_key: u32, retail_key: u32) -> u32 {
    let debug_decoded = encoded ^ debug_key;
    if debug_decoded < max_size.wrapping_sub(1000) {
        return debug_decoded;
    }

    let retail_decoded = encoded ^ retail_key;
    if retail_decoded < max_size.wrapping_sub(1000) {
        return retail_decoded;
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// Either [`entry_point`] or [`kernel_thunk_addr`].
    type Decrypt = fn(u32, u32) -> VirtualAddress<u32>;

    #[rstest]
    #[case::entry_point(entry_point, DEBUG_ENTRY_KEY)]
    #[case::thunk(kernel_thunk_addr, DEBUG_THUNK_KEY)]
    fn decrypt_should_prefer_debug_key_when_in_range(
        #[case] decrypt: Decrypt,
        #[case] debug_key: u32,
    ) {
        assert_eq!(decrypt(0x1000 ^ debug_key, 0x100000).get(), 0x1000);
    }

    #[rstest]
    #[case::entry_point(entry_point, RETAIL_ENTRY_KEY)]
    #[case::thunk(kernel_thunk_addr, RETAIL_THUNK_KEY)]
    fn decrypt_should_fall_back_to_retail_key(#[case] decrypt: Decrypt, #[case] retail_key: u32) {
        // The debug key gives an out-of-range address here, so we fall back to retail.
        assert_eq!(decrypt(0x1000 ^ retail_key, 0x100000).get(), 0x1000);
    }

    #[rstest]
    #[case::entry_point(entry_point, DEBUG_ENTRY_KEY, RETAIL_ENTRY_KEY)]
    #[case::thunk(kernel_thunk_addr, DEBUG_THUNK_KEY, RETAIL_THUNK_KEY)]
    fn decrypt_should_return_zero_when_no_key_fits(
        #[case] decrypt: Decrypt,
        #[case] debug_key: u32,
        #[case] retail_key: u32,
    ) {
        // Both addresses are within the last 1000 bytes of the file.
        let max_size = 0x100000;
        let debug_encoded = (max_size - 500) ^ debug_key;
        let retail_encoded = (max_size - 500) ^ retail_key;
        assert_ne!(debug_encoded, retail_encoded);
        assert_eq!(decrypt(debug_encoded, max_size).get(), 0);
        assert_eq!(decrypt(retail_encoded, max_size).get(), 0);
    }

    #[rstest]
    #[case::entry_point(entry_point, DEBUG_ENTRY_KEY)]
    #[case::thunk(kernel_thunk_addr, DEBUG_THUNK_KEY)]
    fn decrypt_should_accept_nonsense_when_margin_wraps(
        #[case] decrypt: Decrypt,
        #[case] debug_key: u32,
    ) {
        // `max_size - 1000` wraps to a huge value, so we take any debug result.
        let encoded = u32::MAX;
        assert_eq!(decrypt(encoded, 500).get(), encoded ^ debug_key);
    }
}
