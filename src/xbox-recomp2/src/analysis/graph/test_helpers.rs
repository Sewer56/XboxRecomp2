//! Build guest executables for discovery tests.

use super::{DiscoveryGraph, FunctionDiscoverySource, FunctionNode, LocalEdge};
use crate::analysis::walk::FunctionWalker;
use crate::config::RecompilerConfig;
use crate::decode::Decoder;
use crate::executable::{Executable, Section, SectionFlags};
use xbox_recomp2_executable::GuestAddress32;

/// Guest base address shared by tests.
pub(in crate::analysis) const BASE: u32 = 0x1000;

/// Adds `edge` to a function.
pub(in crate::analysis) fn add_local_edge(
    graph: &mut DiscoveryGraph,
    function: GuestAddress32,
    edge: LocalEdge,
) {
    graph
        .functions
        .get_mut(&function)
        .expect("edge owner must be seeded")
        .learned
        .edges
        .insert(edge);
}

/// Returns [`BASE`] + `offset` as a guest address.
pub(in crate::analysis) fn at(offset: u32) -> GuestAddress32 {
    GuestAddress32::from(BASE + offset)
}

/// Walks `code` as an entry point func and returns its discovered node.
pub(in crate::analysis) fn function(code: &[u8]) -> FunctionNode {
    let executable = executable(code);
    let config = RecompilerConfig::default();
    let decoder = Decoder::new_compatibility_mode();
    let mut walker = FunctionWalker::new(&executable, &config, &decoder);
    let mut graph = DiscoveryGraph::default();
    graph.add_function(base(), FunctionDiscoverySource::EntryPoint, None);
    walker.walk_function(&mut graph, base()).unwrap();

    graph.functions.remove(&base()).unwrap()
}

/// Returns [`BASE`] as a guest address.
pub(in crate::analysis) fn base() -> GuestAddress32 {
    GuestAddress32::from(BASE)
}

/// Maps `code` as one `.text` section at [`BASE`], also entry point.
pub(in crate::analysis) fn executable(code: &[u8]) -> Executable {
    Executable {
        data: code.to_vec(),
        size: code.len() as u32,
        base: u64::from(BASE).into(),
        entry_point: u64::from(BASE).into(),
        sections: vec![Section::new(
            ".text",
            u64::from(BASE),
            code.len() as u32,
            SectionFlags::CODE,
            0,
        )],
        ..Executable::default()
    }
}
