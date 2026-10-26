// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub mod canonical;
pub mod grammar;
pub mod identity;
pub mod resolve;

pub use canonical::{canonicalize_edges, check_acyclic, qualify_module_edges};
pub use grammar::{
    ModuleDecl, ModuleError, ModuleRef, ModuleSource, RemoteKind, SourceClass, classify_literal,
};
pub use identity::identities_digest;
pub use resolve::{ModuleEdge, ModuleEdges, ModuleFinding, resolve_local_target, resolve_refs};

pub(crate) use canonical::canonicalize_side;
pub(crate) use grammar::{source_from_json, source_from_native};

#[must_use]
pub fn module_edge_pairs(edges: &[ModuleEdge]) -> Vec<(String, String)> {
    edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect()
}
