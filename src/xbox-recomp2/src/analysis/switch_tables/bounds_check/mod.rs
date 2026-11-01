//! Find how many switch-table slots a nearby bounds check allows.
//!
//! A slot holds one 4-byte target address. A bounds check uses `cmp` and
//! `ja`/`jae` to send out-of-range inputs away from the table jump.
//!
//! ```text
//! cmp eax, 3             ; compare the input with the last allowed value
//! ja  default_case       ; inputs above 3 go elsewhere
//! mov esi, eax           ; copy the checked input
//! jmp [esi*4 + targets]  ; use it to pick a slot
//! ```
//!
//! Here, inputs 0 through 3 reach `jmp`, so the table needs four slots.
//! With `jae` instead of `ja`, input 3 also goes elsewhere: only three slots.
//!
//! # Between `cmp` and `jmp`
//!
//! Between `cmp` and `ja`/`jae`, keep these unchanged:
//!
//! * The compared register: `eax` in the example.
//! * The jump index: `esi`, which picks the table slot.
//! * The flags the branch reads: carry (`CF`) for `jae`; `CF` and zero (`ZF`)
//!   for `ja`.
//!
//! For example, before the branch:
//!
//! * `mov edx, 5` is OK: it leaves those registers and flags alone.
//! * `inc ebx` is OK before `jae`, but not `ja`: it changes `ZF`, not `CF`.
//! * `add edx, 1` changes `CF` and `ZF`, so we reject it before either branch.
//!
//! From `ja`/`jae` to `jmp`, flag changes are OK. Allow one index change:
//!
//! * `mov esi, eax` copies the input, keeping the same slot count.
//! * `movzx esi, byte [eax + remap]` reads a slot number from a byte array,
//!   called a remap. See [`infer_byte_remap`] for a worked example.
//!
//! Other instructions must keep the input and index intact and stay on this
//! path. We reject calls, extra branches, and CPU-state restores like `xrstor`.
//!
//! A memory write between `cmp` and the byte lookup might change the remap.
//! We then reject the lookup because we cannot trust the bytes in the file.
//!
//! # Remarks
//!
//! The section scan must also find the `cmp`, which helps reject matches
//! inside another instruction. This does not prove every path runs the check.
//!
//! [`BoundsCheck::can_trim`] checks known entry addresses for paths that skip
//! `cmp`. Unknown paths can still bypass it.
//!
//! See "Direct Index Guard", "Byte-index remapping", and "Important Limitation"
//! under "Discover switch tables" in `doc/docs/function-discovery.md` for the
//! broader context.

use super::candidate::{Candidate, is_code, mapped_bytes};
use crate::decode::{
    CpuFlag, DecodeBuffer, DecodeOutcome, Decoder, Insn, Mnemonic, Register, immediate, memory,
    register,
};
use crate::executable::Executable;
use endian_writer::{EndianReader, LittleEndianReader};

mod gap;

/// Look back at most 64 bytes for each jump.
const LOOKBACK_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// A slot count from a bounds check, plus addresses used to check for bypasses.
///
/// A bypass is another path that reaches the table jump without running `cmp`.
/// Call [`Self::can_trim`] before using this count to remove collected targets.
pub(super) struct BoundsCheck {
    /// Number of 4-byte slots to keep; several slots may hold the same target.
    pub(super) slots: usize,

    compared_at: u64,
    jump_end: u64,
}

impl BoundsCheck {
    /// Finds a nearby bounds check and uses it to count the table's slots.
    ///
    /// We try earlier bytes as starting points and decode forward to `jmp`.
    /// All matches must agree on the count and comparison address.
    ///
    /// # Arguments
    ///
    /// * `executable` - supplies the code, table, and byte-lookup data.
    /// * `decoder` - reads the jump and the instructions before it.
    /// * `buffer` - scratch space reused for each decoded instruction.
    /// * `candidate` - identifies the table and its `jmp` instruction.
    /// * `comparisons` - addresses where the section scan found `cmp`.
    ///
    /// # Remarks
    ///
    /// `comparisons` must be sorted in ascending order.
    /// Call [`Self::can_trim`] on the result to check for known bypasses.
    ///
    /// # Returns
    ///
    /// `Some` with the slot count and bounds-check addresses, or `None` if:
    ///
    /// * The jump is unreadable or is not a supported table jump.
    /// * No nearby sequence matches, or the matches disagree.
    /// * The section scan did not find the matching `cmp`.
    /// * An address or table-size calculation overflows, or table bytes are missing.
    /// * Any slot's target is outside a section marked as code.
    pub(super) fn find(
        executable: &Executable,
        decoder: &Decoder,
        buffer: &mut DecodeBuffer,
        candidate: &Candidate,
        comparisons: &[u64],
    ) -> Option<Self> {
        // Read the jump from its section.
        let section = executable.section_containing(candidate.jump)?;
        let offset = usize::try_from(candidate.jump - section.base.get()).ok()?;
        let bytes = section.file_bytes(&executable.data);

        // We only handle table jumps of this shape:
        //
        // ```text
        // jmp [esi*4 + 0x11400]
        //      |   |     |
        //      |   |     +-- disp: the table address
        //      |   +--------- scale: slot size, always 4 here
        //      +------------- index: register that picks the slot
        // ```
        //
        // A base register would add an address we cannot determine to the
        // table address. The jump must use the default DS segment.
        let DecodeOutcome::Ok(jump) =
            buffer.decode_single_instruction(decoder, bytes.get(offset..)?)
        else {
            return None;
        };
        let operand = &jump.operands()[0];
        let mem = memory(operand)?;
        if jump.mnemonic() != Mnemonic::JMP
            || jump.instruction().address_width != 32
            || operand.size != 32
            || mem.base != Register::NONE
            || mem.scale != 4
            || !is_index(mem.index)
            || mem.segment != Register::DS
            || mem.disp.displacement as u64 != candidate.address
        {
            return None;
        }
        let index = mem.index;
        let jump_end = candidate.jump.checked_add(jump.length() as u64)?;

        // Try each earlier byte as a start and decode forward towards the known jump.
        //
        // Search at most LOOKBACK_BYTES, oldest first, without crossing the
        // section start. Do not decode instructions backward. NOP or INT3
        // padding is not a reliable boundary, so we try every start byte.
        //
        // Example: the section starts at 0x1000 and the jump is at 0x1005.
        // ```text
        //   0x1000: 83 F8 07  cmp eax, 7
        //   0x1003: 77 1B     ja 0x1020
        //   0x1005: ...       jmp [eax*4 + targets] (decoded above)
        // ```
        //
        // Try 0x1000, 0x1001, 0x1002, 0x1003 and 0x1004. Only 0x1000
        // matches: cmp -> ja ends exactly at the jump.
        //
        // Do not accept the first match immediately: all matches must agree
        // on the count and addresses, even starts the section scan did not find.
        let mut matched = None;
        for start in offset.saturating_sub(LOOKBACK_BYTES)..offset {
            let Some(check) = match_sequence(
                decoder,
                buffer,
                &bytes[start..offset],
                section.base.get() + start as u64,
                jump_end,
                index,
                |address| mapped_bytes(executable, address),
            ) else {
                continue;
            };
            if matched.is_some_and(|previous| previous != check) {
                return None;
            }
            matched = Some(check);
        }

        // The whole table must be readable, and every target must point to code.
        let check = matched?;

        // The section scan must also decode a comparison at this address. A
        // `cmp` found only inside another instruction is not real code.
        if comparisons.binary_search(&check.compared_at).is_err() {
            return None;
        }

        // If one target is not code, keep the fallback targets instead of
        // accepting only part of the table.
        let table = mapped_bytes(executable, candidate.address)?;
        let extent = table.get(..check.slots.checked_mul(4)?)?;
        let mut reader = unsafe { LittleEndianReader::new(extent.as_ptr()) };
        for _ in 0..check.slots {
            let target = unsafe { reader.read_u32() };
            if !is_code(executable, u64::from(target)) {
                return None;
            }
        }

        Some(check)
    }

    /// Checks whether we can remove collected targets beyond the slot count.
    ///
    /// We call this trimming: it changes our target list, not the executable.
    /// An entry is an address where execution may start, such as a branch target.
    ///
    /// Another branch could skip the comparison by landing at `dispatch`:
    ///
    /// ```text
    ///           cmp eax, 3
    ///           ja  default_case
    /// dispatch: jmp [eax*4 + targets]  ; entering here skips the check
    /// ```
    ///
    /// We check known entries against the sequence:
    ///
    /// * At `cmp`: OK, the comparison still runs.
    /// * After `cmp`, but before the end of `jmp`: block trimming.
    /// * Before `cmp`, or at or after the end of `jmp`: outside this sequence.
    ///
    /// # Arguments
    ///
    /// * `entries` - known entry addresses, sorted in ascending order.
    ///
    /// # Remarks
    ///
    /// This only checks known entries. An undiscovered branch can still skip
    /// the comparison.
    ///
    /// # Returns
    ///
    /// `false` if an entry is strictly between `compared_at` and `jump_end`;
    /// `true` otherwise. Entries at either boundary do not block trimming.
    pub(super) fn can_trim(self, entries: &[u64]) -> bool {
        // An entry at the CMP still runs the check. Find the first entry after
        // it; because entries are sorted, only that address needs checking.
        let next = entries.partition_point(|&address| address <= self.compared_at);

        // An entry before the jump's end might bypass the check. An entry at
        // or after the end is outside this sequence and does not block trimming.
        entries
            .get(next)
            .is_none_or(|&address| address >= self.jump_end)
    }
}

/// Checks how many jump-table slots a nearby bounds check allows.
///
/// We decode from `cmp` towards the table jump. For example:
///
/// ```text
/// cmp eax, 3             ; compare the input with the last allowed value
/// ja  default_case       ; inputs above 3 take the default path
/// nop                    ; leaves eax unchanged
/// jmp [eax*4 + targets]  ; eax selects one of four table slots
/// ```
///
/// Here, inputs 0 through 3 reach the table jump. The `nop` does not affect
/// the check, so we can still use it to get a count of four slots.
/// The module docs describe the supported register copies and byte lookups.
///
/// # Arguments
///
/// * `decoder` - decodes each instruction along the way.
/// * `buffer` - reusable scratch space for decoding one instruction.
/// * `bytes` - bytes from the comparison up to, but not including, the jump.
/// * `start` - guest address of the comparison at the start of `bytes`.
/// * `jump_end` - guest address just past the table jump.
/// * `index` - register selecting the table slot: `eax` in the example above.
/// * `read_bytes` - returns bytes from a guest address to the end of the
///   available section data, or `None` if unavailable. Used for byte lookups.
///
/// # Returns
///
/// `Some(BoundsCheck)` with the slot count, comparison address, and jump end.
///
/// Returns `None` if we cannot follow a supported bounds check to the jump:
///
/// * The bytes do not decode fully, or do not start with a supported `cmp`.
/// * No relative `ja`/`jae` follows, or flags it needs have changed.
/// * The bound does not give a usable slot count.
/// * The branch lands inside `start..jump_end`, including the table jump.
/// * An instruction breaks the check or changes the index in an unsupported way.
/// * The jump uses another register without a copy or byte lookup from the input.
/// * A byte lookup uses an unsupported form, lacks data, or follows a memory write.
fn match_sequence<'a>(
    decoder: &Decoder,
    buffer: &mut DecodeBuffer,
    bytes: &[u8],
    start: u64,
    jump_end: u64,
    index: Register,
    read_bytes: impl Fn(u64) -> Option<&'a [u8]>,
) -> Option<BoundsCheck> {
    // Start with `cmp eax, N`:
    //
    // * the register must be usable as a table index,
    // * the bound must be a plain number, not a relative address.
    let DecodeOutcome::Ok(cmp) = buffer.decode_single_instruction(decoder, bytes) else {
        return None;
    };
    if cmp.mnemonic() != Mnemonic::CMP || cmp.operands()[0].size != 32 {
        return None;
    }
    let compared = register(&cmp.operands()[0])?;
    if !is_index(compared) {
        return None;
    }
    let bound = immediate(&cmp.operands()[1])?;
    if bound.is_relative {
        return None;
    }
    let bound = bound.value as u32;
    let mut offset = cmp.length();

    // Track flag changes between `cmp` and `ja`/`jae`.
    let mut changed_flags = CpuFlag::empty();
    let mut stored = false;
    let count = loop {
        let DecodeOutcome::Ok(insn) =
            buffer.decode_single_instruction(decoder, bytes.get(offset..)?)
        else {
            return None;
        };
        offset += insn.length();

        // The bound branch, e.g. after `cmp eax, 3`:
        //
        // ```text
        // ja  default_case   ; inputs above 3 default (Zydis: JNBE)
        // jae default_case   ; input 3 defaults too (Zydis: JNB)
        // ```
        let bound_and_flags = match insn.mnemonic() {
            Mnemonic::JNBE => bound
                .checked_add(1)
                .map(|count| (count, CpuFlag::CF | CpuFlag::ZF)),
            Mnemonic::JNB => Some((bound, CpuFlag::CF)),
            _ => None,
        };
        if let Some((count, needed_flags)) = bound_and_flags {
            // Found `ja`/`jae`. Out-of-range inputs must jump outside
            // this `cmp`-to-`jmp` sequence.
            let displacement = immediate(&insn.operands()[0])?;
            if changed_flags.intersects(needed_flags) || !displacement.is_relative || count == 0 {
                return None;
            }

            let target = (start + offset as u64).wrapping_add(displacement.value) as u32;
            if (start..jump_end).contains(&u64::from(target)) {
                return None;
            }
            break count as usize;
        }

        // Reject instructions that break the check, like `call helper` or
        // `inc eax` after `cmp eax, N`.
        if !gap::preserves_index_path(insn, compared, index) {
            return None;
        }

        // Track flag changes, like `add edx, 1`, for the `ja`/`jae` check.
        changed_flags |= gap::flag_writes(insn);

        // Track stores, like `mov [remap], dl`, that might change a byte lookup.
        stored |= gap::writes_memory(insn);
    };

    // Follow from `ja`/`jae` to `jmp`.
    // Allow one copy or byte lookup; flag changes are OK here.
    let mut slots = count;
    let mut transformed = false;
    while offset < bytes.len() {
        let DecodeOutcome::Ok(insn) = buffer.decode_single_instruction(decoder, &bytes[offset..])
        else {
            return None;
        };
        offset += insn.length();

        // For `jmp [esi*4 + targets]`, look for `esi` as the destination.
        let writes_index =
            insn.operands()[0].size == 32 && register(&insn.operands()[0]) == Some(index);
        if !transformed && writes_index {
            // `mov esi, eax`: copy the input; keep the same slot count.
            if insn.mnemonic() == Mnemonic::MOV && register(&insn.operands()[1]) == Some(compared) {
                transformed = true;
                continue;
            }

            // `movzx esi, byte [eax + remap]`: look up the slot number.
            if insn.mnemonic() == Mnemonic::MOVZX {
                // An earlier store might have changed the lookup bytes,
                // so we cannot trust the file's copy.
                if stored {
                    return None;
                }
                slots = infer_byte_remap(insn, compared, count, &read_bytes)?;
                transformed = true;
                continue;
            }
        }

        // Reject other input or index changes, like `inc esi`, and
        // instructions that break the path, like `call helper`.
        if !gap::preserves_index_path(insn, compared, index) {
            return None;
        }

        // Track stores, like `mov [remap], dl`, before a later byte lookup.
        stored |= gap::writes_memory(insn);
    }

    // If `cmp` uses `eax` but `jmp` uses `esi`, require a copy or lookup.
    if compared != index && !transformed {
        return None;
    }

    // Keep these addresses so can_trim can check for a bypass later.
    Some(BoundsCheck {
        slots,
        compared_at: start,
        jump_end,
    })
}

/// Counts the jump-table slots selected by a byte lookup.
///
/// A remap is a byte array: each input picks a byte, and that byte picks
/// a slot in the jump table. Follow EAX through this example:
///
/// ```text
/// cmp eax, 3                    ; inputs 0 through 3 can use the lookup
/// ja  default_case              ; larger inputs skip the table jump
/// movzx ecx, byte [remap + eax] ; read the slot number into ecx
/// jmp [ecx*4 + targets]         ; jump to the target in that slot
/// ```
///
/// With remap = [0, 1, 1, 0], EAX = 2 loads `remap[2]` = 1 into ECX.
/// The jump then reads `targets[1]`. Four inputs share two table slots.
///
/// # Arguments
///
/// * `load` - the `movzx` instruction that reads the byte.
/// * `compared` - the input register checked by `cmp`: EAX above.
/// * `count` - number of allowed inputs, so also the number of bytes to read.
/// * `read_bytes` - returns file bytes at a guest address, or `None` if missing.
///
/// # Remarks
///
/// The caller checks:
///
/// * The load writes the 32-bit jump index.
/// * No memory write between `cmp` and the lookup could have changed the remap.
///
/// # Returns
///
/// The largest byte value plus one, or `None` if:
///
/// * The instruction is not a `movzx` byte load from memory with 32-bit addressing.
/// * The lookup uses extra registers or scaling, or does not use `compared`.
/// * The lookup uses a segment other than DS or SS.
/// * The lookup bytes are missing, shorter than `count`, or empty.
fn infer_byte_remap<'a>(
    load: &Insn,
    compared: Register,
    count: usize,
    read_bytes: impl FnOnce(u64) -> Option<&'a [u8]>,
) -> Option<usize> {
    if load.mnemonic() != Mnemonic::MOVZX
        || load.instruction().address_width != 32
        || load.operands()[1].size != 8
    {
        return None;
    }

    // The lookup must use the same register as the comparison. The decoder
    // can report EAX as the base or as an index with scale 1:
    //
    // ```text
    //   [eax + remap]     base = eax, no index
    //   [eax*1 + remap]   no base, index = eax, scale = 1
    // ```
    //
    // Both pick the same byte. Reject extra registers or scaling, and allow
    // only the DS or SS segment.
    let mem = memory(&load.operands()[1])?;
    let indexed_by_compared = (mem.base == compared && mem.index == Register::NONE)
        || (mem.base == Register::NONE && mem.index == compared && mem.scale == 1);
    if !indexed_by_compared || !matches!(mem.segment, Register::DS | Register::SS) {
        return None;
    }
    // Read one byte per allowed input: four bytes in the example above.
    // If any byte is missing, we cannot tell which slots the lookup selects.
    let address = u64::from(mem.disp.displacement as u32);
    let remap = read_bytes(address)?.get(..count)?;

    // Slot numbers start at 0: a largest value of 1 needs 2 slots.
    // Widen before adding so byte 255 gives 256 rather than overflowing.
    Some(usize::from(*remap.iter().max()?) + 1)
}

fn is_index(register: Register) -> bool {
    matches!(
        register,
        Register::EAX
            | Register::ECX
            | Register::EDX
            | Register::EBX
            | Register::ESP
            | Register::EBP
            | Register::ESI
            | Register::EDI
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ── Guard matching ───────────────────────────────────────────────────────

    /// These tests check instruction matching without a full executable.
    #[rstest]
    // jae with bound 1: only input 0 reaches the table jump.
    #[case::exclusive(0x73, Some(1))]
    // je: not an unsigned upper-bound check
    #[case::unsupported(0x74, None)]
    fn match_sequence_should_accept_unsigned_bounds(
        #[case] branch: u8,
        #[case] expected_count: Option<usize>,
    ) {
        // Arrange: bytes stop before jmp [eax*4 + 0x11400] at 0x1100D.
        // The jump ends at 0x11014; this match needs no table bytes.
        let bytes = [
            0x83, 0xF8, 0x01, // 0x11008: cmp eax, 1
            branch, 0x30, // 0x1100B: jae/je 0x1103D (see cases above)
        ];

        // Act.
        let check = sequence(&bytes, Register::EAX);

        // Assert: only the unsigned upper-bound branch gives a slot count.
        assert_eq!(check.map(|check| check.slots), expected_count);
    }

    #[rstest]
    #[case::unmapped(None)]
    #[case::truncated(Some(&[0, 1, 1][..]))]
    fn match_sequence_should_reject_incomplete_remap(#[case] remap: Option<&[u8]>) {
        // Arrange: the guard requires four readable remap bytes.
        //
        // Each byte maps an EAX input to an ECX slot. The bytes end before the
        // jump at 0x11015, which ends at 0x1101C.
        //
        // The byte map is missing or lacks the fourth input's byte.
        let bytes = [
            0x83, 0xF8, 0x03, // 0x11008: cmp eax, 3
            0x77, 0x60, // 0x1100B: ja 0x1106D
            0x0F, 0xB6, 0x0C, 0x05, 0x0C, 0x14, 0x01, 0x00, // movzx ecx, byte [eax+0x1140C]
        ];
        let decoder = Decoder::new();
        let mut buffer = DecodeBuffer::new();

        // Act.
        let check = match_sequence(
            &decoder,
            &mut buffer,
            &bytes,
            0x11008,
            0x1101C,
            Register::ECX,
            |address| {
                assert_eq!(address, 0x1140C);
                remap
            },
        );

        // Assert: missing or short byte-map data gives no slot count.
        assert!(check.is_none());
    }

    // ── Instructions between the compare, branch, and jump ───────────────────

    #[rstest]
    #[case::register_move(&[0x89, 0xD3], 0x77, Some(2))] // mov ebx, edx
    #[case::inc_before_ja(&[0x43], 0x77, None)] // inc ebx changes ZF
    #[case::inc_before_jae(&[0x43], 0x73, Some(1))] // INC preserves CF
    #[case::set_carry(&[0xF9], 0x73, None)] // stc
    #[case::clear_carry(&[0xF8], 0x73, None)] // clc
    #[case::undefined_carry(&[0x0F, 0xBC, 0xDA], 0x73, None)] // bsf ebx, edx
    fn match_sequence_should_preserve_required_flags_when_gap_precedes_branch(
        #[case] gap: &[u8],
        #[case] branch: u8,
        #[case] expected_count: Option<usize>,
    ) {
        // Arrange: CMP allows inputs 0..1 for JA, only 0 for JAE.
        let mut bytes = vec![0x83, 0xF8, 1]; // cmp eax, 1
        bytes.extend_from_slice(gap);
        bytes.extend_from_slice(&[branch, 0x60]); // skip the dispatch

        // Act.
        let check = sequence(&bytes, Register::EAX);

        // Assert.
        assert_eq!(check.map(|check| check.slots), expected_count);
    }

    #[rstest]
    #[case::flags_no_longer_needed(&[0x43], Some(2))] // inc ebx
    #[case::stack_store(&[0x53], Some(2))] // push ebx: ESP/memory, not EAX
    #[case::low_alias(&[0xB0, 2], None)] // mov al, 2
    #[case::high_alias(&[0xB4, 2], None)] // mov ah, 2
    #[case::read_write_register(&[0x83, 0xC0, 2], None)] // add eax, 2
    #[case::conditional_write(&[0x0F, 0x44, 0xC3], None)] // cmovz eax, ebx
    #[case::implicit_write(&[0xF7, 0xE3], None)] // mul ebx writes EDX:EAX
    #[case::segment_write(&[0x8E, 0xDB], None)] // mov ds, bx
    #[case::call(&[0xE8, 0, 0, 0, 0], None)]
    #[case::jump(&[0xEB, 0], None)]
    #[case::trap(&[0xCC], None)]
    #[case::cpu_state(&[0x0F, 0xAE, 0x2B], None)] // xrstor [ebx]
    fn match_sequence_should_protect_indexes_when_gap_follows_branch(
        #[case] gap: &[u8],
        #[case] expected_count: Option<usize>,
    ) {
        // Arrange: the branch has consumed the CMP flags before the gap.
        let mut bytes = vec![0x83, 0xF8, 1, 0x77, 0x60]; // cmp eax, 1; ja default
        bytes.extend_from_slice(gap);

        // Act.
        let check = sequence(&bytes, Register::EAX);

        // Assert.
        assert_eq!(check.map(|check| check.slots), expected_count);
    }

    #[rstest]
    #[case::preserved(&[0x43], Some(2))] // inc ebx
    #[case::overwritten(&[0xB1, 2], None)] // mov cl, 2 overwrites copied index
    fn match_sequence_should_track_copy_when_surrounded_by_gaps(
        #[case] after_copy: &[u8],
        #[case] expected_count: Option<usize>,
    ) {
        // Arrange: CMP checks EAX; the jump uses its copy in ECX.
        let mut bytes = vec![
            0x83, 0xF8, 1, // cmp eax, 1
            0x77, 0x60, // ja default
            0x53, // push ebx
            0x89, 0xC1, // mov ecx, eax
        ];
        bytes.extend_from_slice(after_copy);

        // Act.
        let check = sequence(&bytes, Register::ECX);

        // Assert.
        assert_eq!(check.map(|check| check.slots), expected_count);
    }

    #[rstest]
    #[case::no_store(&[0x90], false, Some(2))]
    #[case::explicit_before_compare_branch(&[0xC6, 0x03, 2], true, None)] // mov byte [ebx], 2
    #[case::explicit_before_lookup(&[0xC6, 0x03, 2], false, None)]
    #[case::implicit_before_lookup(&[0x53], false, None)] // push ebx
    #[case::read_write_memory(&[0xFF, 0x03], false, None)] // inc dword [ebx]
    #[case::conditional_before_lookup(&[0x0F, 0xB1, 0x0B], false, None)] // cmpxchg [ebx], ecx
    #[case::masked_before_lookup(&[0x0F, 0xF7, 0xC1], false, None)] // maskmovq mm0, mm1
    fn match_sequence_should_withhold_remap_when_gap_may_write_memory(
        #[case] gap: &[u8],
        #[case] before_branch: bool,
        #[case] expected_count: Option<usize>,
    ) {
        // Arrange: [0, 1] in file memory implies two slots only if unchanged.
        let mut bytes = vec![0x83, 0xF8, 1]; // cmp eax, 1
        if !before_branch {
            bytes.extend_from_slice(&[0x77, 0x60]);
        }
        bytes.extend_from_slice(gap);
        if before_branch {
            bytes.extend_from_slice(&[0x77, 0x60]);
        }
        bytes.extend_from_slice(&[
            0x0F, 0xB6, 0x88, 0x0C, 0x14, 1, 0, // movzx ecx, byte [eax + 0x1140C]
        ]);

        // Act.
        let check = sequence(&bytes, Register::ECX);

        // Assert.
        assert_eq!(check.map(|check| check.slots), expected_count);
    }

    #[test]
    fn match_sequence_should_keep_loaded_index_when_store_follows_remap() {
        // Arrange: after MOVZX, changing remap bytes cannot change ECX.
        let bytes = [
            0x83, 0xF8, 1, 0x77, 0x60, // cmp eax, 1; ja default
            0x0F, 0xB6, 0x88, 0x0C, 0x14, 1, 0, // movzx ecx, byte [eax + 0x1140C]
            0xC6, 0x03, 2, // mov byte [ebx], 2
        ];

        // Act.
        let check = sequence(&bytes, Register::ECX);

        // Assert.
        assert_eq!(check.map(|check| check.slots), Some(2));
    }

    fn sequence(bytes: &[u8], index: Register) -> Option<BoundsCheck> {
        let decoder = Decoder::new();
        let mut buffer = DecodeBuffer::new();

        // The seven-byte jmp [index*4 + disp32] follows these prefix bytes.
        match_sequence(
            &decoder,
            &mut buffer,
            bytes,
            0x11008,
            0x11008 + bytes.len() as u64 + 7,
            index,
            |address| (address == 0x1140C).then_some(&[0, 1][..]),
        )
    }

    // ── Paths that can skip the comparison ───────────────────────────────────

    #[rstest]
    #[case::at_comparison(0x11008, true)]
    #[case::at_branch(0x1100B, false)]
    #[case::after_dispatch(0x11014, true)]
    fn can_trim_should_reject_entries_inside_guard(
        #[case] entry: u64,
        #[case] expected_can_trim: bool,
    ) {
        // Arrange: the comparison starts at 0x11008; the table jump ends at
        // 0x11014. The two fixed code entries are outside this sequence.
        //
        // - 0x11008 (comparison): allow trimming; the check still runs.
        // - 0x1100B (branch): block trimming; entry skips the comparison.
        // - 0x11014 (dispatch end): allow trimming; entry is outside the sequence.
        let check = BoundsCheck {
            slots: 2,
            compared_at: 0x11008,
            jump_end: 0x11014,
        };
        let entries = [0x11000, entry, 0x11060];

        // Act.
        let can_trim = check.can_trim(&entries);

        // Assert: only entries strictly inside the guard span prevent trimming.
        assert_eq!(can_trim, expected_can_trim);
    }
}
