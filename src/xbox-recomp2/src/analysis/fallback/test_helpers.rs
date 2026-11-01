//! Test helper for the fallback scan.

use super::{Decoder, DiscoveryGraph, Executable, GuestAddress32, RecompilerConfig};

pub(super) fn collect(
    executable: &Executable,
    config: &RecompilerConfig,
    decoder: &Decoder,
) -> Vec<GuestAddress32> {
    let mut graph = DiscoveryGraph::default();
    super::collect(executable, config, decoder, &mut graph);
    graph.functions.into_keys().collect()
}
