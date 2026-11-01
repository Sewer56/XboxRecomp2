//! First pass over a function: finds which addresses need a label, and
//! which addresses have hooks.
//!
//! See "Find labels and hooks" in `doc/docs/recompiling-functions.md` for
//! why labels exist and which addresses get one.

use crate::analysis::FunctionBody;
use crate::config::{MidAsmHook, RecompilerConfig};
use crate::decode::{
    DecodeBuffer, DecodeOutcome, Decoder, Insn, Mnemonic, RangeDecoder, immediate_signed,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use xbox_recomp2_executable::GuestAddress32;

/// What the first pass found in one function.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct LabelPass {
    /// Addresses that need a `loc_<address>:` label.
    pub labels: BTreeSet<GuestAddress32>,

    /// Addresses with a mid-assembly hook, in address order.
    ///
    /// Look each hook up in the config's `midasm_hooks`.
    pub hook_addresses: Vec<GuestAddress32>,

    /// Maps an instruction to the one that runs next when it doesn't jump.
    /// Key = instruction address, Value = next address.
    pub fallthroughs: BTreeMap<GuestAddress32, GuestAddress32>,
}

/// Finds labels and hooks by decoding every byte from `base` to `base + size`.
///
/// Used for user defined ranges (`fixes.functions`). See "From a user
/// defined range" in `doc/docs/recompiling-functions.md`.
///
/// # Arguments
///
/// * `config` - supplies invalid and banned ranges, switch tables and hooks.
/// * `decoder` - decodes the instructions.
/// * `buffer` - reusable decode buffer.
/// * `data` - the executable's bytes, starting at `base`.
/// * `base` - address of the function's first byte.
/// * `size` - the function's size in bytes. May be a guess.
/// * `diag` - receives a warning for each hook that jumps outside the range.
///
/// # Remarks
///
/// A range may wrap past `u32::MAX` back to `0x0`.
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_labels(
    config: &RecompilerConfig,
    decoder: &Decoder,
    buffer: &mut DecodeBuffer,
    data: &[u8],
    base: GuestAddress32,
    size: u32,
    diag: &mut dyn Write,
) -> LabelPass {
    let mut result = LabelPass::default();
    let mut range = RangeDecoder::new(config, decoder, buffer, data, base, size);

    while let Some(step) = range.next_step() {
        let insn = match step.decoded {
            DecodeOutcome::Ok(insn) => Some(insn),
            DecodeOutcome::Failed { .. } => None,
        };

        collect_configured_labels(
            config,
            step.address,
            &mut result,
            |target| target.wrapping_sub(base) < size,
            "function range",
            diag,
            insn.is_some(),
        );

        if let Some(insn) = insn {
            collect_jump_target(insn, step.address, base, size, &mut result.labels);
        }
    }

    result
}

/// Finds labels, hooks and fallthroughs from a [`FunctionBody`].
///
/// See "Which addresses get a label?" in `doc/docs/recompiling-functions.md`
/// for which addresses need a label and why.
///
/// # Arguments
///
/// * `config` - supplies switch tables and mid-assembly hooks.
/// * `body` - the function's instructions and the edges between them.
/// * `diag` - receives a warning for each hook that jumps outside the body.
///
/// # Remarks
///
/// This function only labels addresses in the body:
///
/// - It skips switch cases and hook jump targets outside the body.
/// - It labels the entry for the opening `goto` when the entry isn't
///   the lowest address. See "Jump to the entry, if needed".
pub(super) fn collect_labels_from_body(
    config: &RecompilerConfig,
    body: &FunctionBody,
    diag: &mut dyn Write,
) -> LabelPass {
    let mut result = LabelPass::default();
    let is_target_in_body = |address: GuestAddress32| body.contains(address);

    for &address in body.starts() {
        // Collect switch and hook labels within the body.
        collect_configured_labels(
            config,
            address,
            &mut result,
            is_target_in_body,
            "selected body",
            diag,
            true,
        );
    }

    for edge in body.edges() {
        if !body.contains(edge.target) {
            continue;
        }

        // Insert a label (only) if we're not falling into the very next
        // instruction.
        if edge.kind.is_sequential() {
            // We need to also ensure the instruction is the very next address.
            // If you're wondering 'why it may not be', the answer is- data!
            //
            // If some insane-ass compiler decided that to put something like
            // switch targets in the middle of the function, the next instruction
            // would have a gap, in which case we need a goto.
            if body.next_start_after(edge.source) != Some(edge.target) {
                result.labels.insert(edge.target);
            }
            result.fallthroughs.insert(edge.source, edge.target);
        } else {
            result.labels.insert(edge.target);
        }
    }

    // If the entry isn't the first instruction, the function starts
    // with a `goto` to it.
    if body
        .first_address()
        .is_some_and(|first| first != body.entry())
        && body.contains(body.entry())
    {
        result.labels.insert(body.entry());
    }

    result
}

/// Returns whether `mnemonic` is a jump (`jmp`, any conditional `j*`, or `loope`).
///
/// Zydis numbers all `j*` mnemonics, `JMP` included, between `JB` and `JZ`.
pub(in crate::codegen) fn is_jump(mnemonic: Mnemonic) -> bool {
    let value = mnemonic as u32;
    (value >= Mnemonic::JB as u32 && value <= Mnemonic::JZ as u32) || mnemonic == Mnemonic::LOOPE
}

/// Collects configured labels at one address.
#[allow(clippy::too_many_arguments)]
fn collect_configured_labels(
    config: &RecompilerConfig,
    address: GuestAddress32,
    result: &mut LabelPass,
    is_local: impl Fn(GuestAddress32) -> bool,
    boundary: &str,
    diag: &mut dyn Write,
    include_hook: bool,
) {
    // Check configured switch tables
    if let Some(table) = config.switch_tables.get(address.get().into()) {
        result.labels.extend(
            table
                .labels
                .iter()
                .copied()
                .map(GuestAddress32::from)
                // Must ensure falls within this function.
                .filter(|&target| is_local(target)),
        );
    }

    // Now add the midasm hooks.
    if include_hook && let Some(hook) = config.midasm_hooks.get(address.get().into()) {
        result.hook_addresses.push(address);

        // Our midasm hook terminator/result should branch to
        // a location within the function; where we'll need to add a label.
        //
        // Currently branching outside is not supported.
        collect_hook_labels(hook, address, &mut result.labels, is_local, boundary, diag);
    }
}

/// Adds a label for a jump's target when the target is inside the function.
///
/// # Arguments
///
/// * `insn` - the decoded instruction found at `addr`.
/// * `addr` - address of `insn`.
/// * `base` - address of the function's first byte.
/// * `size` - the function's size in bytes.
/// * `labels` - receives the target address if it needs a label.
///
/// # Remarks
///
/// A relative jump measures from the end of the jump.
/// So for instance, a 2-byte `jz +4` at `0x1001` lands on
/// `0x1001 + 2 + 4 = 0x1007`.
///
/// (👆 Noting this for anyone unfamiliar with x86, because not all arches
///     work like this)
fn collect_jump_target(
    insn: &Insn,
    addr: GuestAddress32,
    base: GuestAddress32,
    size: u32,
    labels: &mut BTreeSet<GuestAddress32>,
) {
    // Ignore jumps.
    if !is_jump(insn.mnemonic()) {
        return;
    }

    // Relative jumps are represented as `jmp imm32`; so check for that.
    if let Some(displacement) = immediate_signed(&insn.operands()[0]) {
        let target = addr
            .wrapping_add(insn.length() as u32)
            .wrapping_add(displacement as u32); // 👈 may be negative

        // Not ensure it lands in function before we insert it. Thanks.
        let offset_from_base = target.wrapping_sub(base);
        if offset_from_base > 0 && offset_from_base < size {
            labels.insert(target);
        }
    }
}

/// Returns the warning line for a hook at `address` jumping to `target`,
/// outside `boundary`.
fn outside_hook_warning(address: GuestAddress32, target: GuestAddress32, boundary: &str) -> String {
    format!(
        "WARNING: Mid-assembly hook at 0x{address:X} jumps outside the {boundary}: 0x{target:X}\n"
    )
}

/// Adds the hook's local targets, and warns about the others.
fn collect_hook_labels(
    hook: &MidAsmHook,
    address: GuestAddress32,
    labels: &mut BTreeSet<GuestAddress32>,
    is_local: impl Fn(GuestAddress32) -> bool,
    boundary: &str,
    diag: &mut dyn Write,
) {
    for target in [
        hook.jump_address,
        hook.jump_address_on_true,
        hook.jump_address_on_false,
    ] {
        // Zero means this path doesn't jump.
        if target == 0 {
            continue;
        }

        // Only local targets have a label to jump to.
        let target = GuestAddress32::from(target);
        if is_local(target) {
            labels.insert(target);
        } else {
            let _ = write!(diag, "{}", outside_hook_warning(address, target, boundary));
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the label pass, using small hand-written byte sequences and
    //! bodies.

    use super::{LabelPass, collect_labels, collect_labels_from_body, outside_hook_warning};
    use crate::analysis::{BodyEdge, EdgeKind, FunctionBody};
    use crate::config::{
        FunctionAddress, InvalidInstructionSize, MidAsmHook, RecompilerConfig, Register,
    };
    use crate::decode::{DecodeBuffer, Decoder};
    use crate::executable::SwitchTable;
    use rstest::rstest;
    use std::collections::{BTreeMap, BTreeSet};
    use xbox_recomp2_executable::GuestAddress32;

    // ── collect_labels ──────────────────────────────────────────────────────

    /// Switch and hook targets get a label only inside the function range.
    #[rstest]
    #[case::inside(
        &[0x74, 0x00, 0x90], // jz +0; nop
        0x1000,
        &[0x1002], // jz lands on the nop
    )]
    #[case::first_byte_and_end(
        &[0xEB, 0xFE, 0x74, 0x00], // jmp -2; jz +0
        0x1000,
        &[], // jmp lands on the first byte, jz on the end (0x1004)
    )]
    #[case::outside(
        &[0x74, 0x20], // jz +0x20
        0x1000,
        &[], // jz lands outside, at 0x1022
    )]
    #[case::through_register(
        &[0xFF, 0xE0], // jmp eax
        0x1000,
        &[], // no fixed target
    )]
    #[case::wrapped(
        &[0x74, 0x00, 0x74, 0xFD, 0x74, 0xFA], // jz +0; jz -3; jz -6
        u32::MAX - 1,
        &[0, u32::MAX], // first two jz; the last lands on the first byte
    )]
    fn range_should_label_jump_targets_inside_function(
        #[case] data: &[u8],
        #[case] base: u32,
        #[case] expected: &[u32],
    ) {
        // Arrange
        let config = RecompilerConfig::default();

        // Act
        let (pass, _) = collect(&config, data, base);

        // Assert
        assert_eq!(pass.labels, addresses(expected));
    }

    /// Switch and hook targets get a label only inside the function range.
    #[test]
    fn range_should_label_configured_targets_inside_function() {
        // Arrange
        let base = u32::MAX;
        let mut config = RecompilerConfig::default();
        add_switch_table(&mut config, base, &[base - 1, 0]);
        add_hook(&mut config, 0, base, 1);

        // Act
        let (pass, diag) = collect(&config, &[0x90, 0x90], base);

        // Assert
        assert_eq!(pass.labels, addresses(&[0, base])); // switch -> 0x0, hook -> base
        assert_eq!(pass.hook_addresses, addresses_vec(&[0])); // the hook at 0x0
        assert_eq!(
            diag,
            outside_hook_warning(0.into(), 1.into(), "function range") // 0x1 is the end
        );
    }

    /// Invalid and banned bytes aren't decoded, so a jump inside them adds no
    /// label. Decoding carries on right after them.
    ///
    /// ```text
    /// 0x1000  nop
    /// 0x1001  jz +1  -> 0x1004  skipped, no label
    /// 0x1003  jz +0  -> 0x1005  label
    /// 0x1005  nop
    /// ```
    #[rstest]
    #[case::invalid(Skip::Invalid)]
    #[case::banned(Skip::Banned)]
    fn range_should_skip_unusable_bytes(#[case] skip: Skip) {
        // Arrange
        // Mark the bytes at 0x1001..=0x1002 as unusable.
        let mut config = RecompilerConfig::default();
        match skip {
            Skip::Invalid => config
                .invalid_instructions
                .insert_or_overwrite(0x1001.into(), InvalidInstructionSize(2)),
            Skip::Banned => config
                .banned_addresses
                .insert_or_overwrite(0x1001.into(), FunctionAddress::from(0x1002)),
        }

        // Act
        // (Bytes match example in func description)
        let (pass, _) = collect(&config, &[0x90, 0x74, 0x01, 0x74, 0x00, 0x90], 0x1000);

        // Assert
        assert_eq!(pass.labels, addresses(&[0x1005]));
    }

    // ── collect_labels_from_body ────────────────────────────────────────────

    /// An edge labels its target unless the code can just fall into it.
    ///
    /// ```text
    /// 0x1000  edge starts here
    /// 0x1002  written next: no goto needed
    /// 0x1008  0x1002 is in the way: needs a goto
    /// ```
    #[rstest]
    #[case::fallthrough_to_next(
        EdgeKind::NextInstruction,
        0x1002,       // target
        None,         // label: written right after the source
        Some(0x1002), // fallthrough
    )]
    #[case::fallthrough_over_code(
        EdgeKind::NextInstruction,
        0x1008,       // target
        Some(0x1008), // label: points at 0x1008 because we skip 0x1002's due
                      //        due to control flow of instruction set by edge.
        Some(0x1008), // fallthrough
    )]
    #[case::jump(
        EdgeKind::Jump,
        0x1008,       // target
        Some(0x1008), // label
        None,         // fallthrough: none; is a jump.
    )]
    fn body_should_label_edge_target_when_goto_needed(
        #[case] kind: EdgeKind,
        #[case] target: u32,
        #[case] label: Option<u32>,
        #[case] fallthrough: Option<u32>,
    ) {
        // Arrange
        let config = RecompilerConfig::default();
        let edge = BodyEdge {
            source: GuestAddress32::from(0x1000),
            target: GuestAddress32::from(target),
            kind,
        };
        let body = fixture_body(0x1000, [(0x1000, 2), (0x1002, 1), (0x1008, 1)], [edge]);

        // Act
        let (pass, _) = collect_from_body(&config, &body);

        // Assert
        assert_eq!(pass.labels, addresses(label.as_slice()));
        assert_eq!(
            pass.fallthroughs,
            BTreeMap::from_iter(fallthrough.map(|to| (0x1000.into(), to.into())))
        );
    }

    /// A function that doesn't start at its first instruction opens with a
    /// `goto` to its entry.
    ///
    /// ```text
    /// 0x1000  first instruction
    /// 0x1002  second instruction
    /// ```
    #[rstest]
    #[case::starts_at_first(0x1000, &[])] // runs from the top, no goto
    #[case::starts_later(0x1002, &[0x1002])] // goto 0x1002
    #[case::starts_outside(0x0FFF, &[])] // no instruction to jump to
    fn body_should_label_entry_when_it_is_not_first_instruction(
        #[case] entry: u32,
        #[case] expected: &[u32],
    ) {
        // Arrange
        let config = RecompilerConfig::default();
        let body = fixture_body(entry, [(0x1000, 2), (0x1002, 1)], []);

        // Act
        let (pass, _) = collect_from_body(&config, &body);

        // Assert
        assert_eq!(pass.labels, addresses(expected));
    }

    // ── Helpers ─────────────────────────────────────────────────────────────

    /// Turns raw addresses into a set of [`GuestAddress32`].
    fn addresses(raw: &[u32]) -> BTreeSet<GuestAddress32> {
        raw.iter().copied().map(GuestAddress32::from).collect()
    }

    /// Turns raw addresses into a list of [`GuestAddress32`].
    fn addresses_vec(raw: &[u32]) -> Vec<GuestAddress32> {
        raw.iter().copied().map(GuestAddress32::from).collect()
    }

    /// How [`range_should_skip_unusable_bytes`] marks bytes as unusable.
    enum Skip {
        Invalid,
        Banned,
    }

    /// Runs `collect_labels` over all of `data`, returning the diagnostics.
    fn collect(config: &RecompilerConfig, data: &[u8], base: u32) -> (LabelPass, String) {
        let mut diag = Vec::new();
        let pass = collect_labels(
            config,
            &Decoder::new(),
            &mut DecodeBuffer::new(),
            data,
            base.into(),
            data.len() as u32,
            &mut diag,
        );
        (pass, String::from_utf8(diag).expect("utf-8 diag"))
    }

    /// Runs `collect_labels_from_body` on `body`, returning the diagnostics.
    fn collect_from_body(config: &RecompilerConfig, body: &FunctionBody) -> (LabelPass, String) {
        let mut diag = Vec::new();
        let pass = collect_labels_from_body(config, body, &mut diag);
        (pass, String::from_utf8(diag).expect("utf-8 diag"))
    }

    /// Registers a switch table at `at`, dispatching to `labels`.
    fn add_switch_table(config: &mut RecompilerConfig, at: u32, labels: &[u32]) {
        config.switch_tables.insert_or_overwrite(
            at.into(),
            SwitchTable::new(Register::Ebx, labels.to_vec(), u64::from(at)),
        );
    }

    /// Registers a hook at `at` that jumps to `on_true` or `on_false`.
    fn add_hook(config: &mut RecompilerConfig, at: u32, on_true: u32, on_false: u32) {
        config.midasm_hooks.insert_or_overwrite(
            at.into(),
            MidAsmHook {
                name: "hook".into(),
                jump_address_on_true: on_true,
                jump_address_on_false: on_false,
                ..MidAsmHook::default()
            },
        );
    }

    /// Builds a body from raw `(address, length)` pairs and edges.
    fn fixture_body<const N: usize>(
        entry: u32,
        instructions: [(u32, u8); N],
        edges: impl IntoIterator<Item = BodyEdge>,
    ) -> FunctionBody {
        FunctionBody::new(
            entry.into(),
            instructions
                .into_iter()
                .map(|(address, length)| (address.into(), length)),
            edges,
        )
    }
}
