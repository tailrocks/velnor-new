//! Local module edges: S6 grammar, H5 canonicalization, M2 identity.
//!
//! `module` block `source` attributes classify per S6: a literal
//! starting `./`/`../` is local (an edge), an absolute path is an
//! external package copy (a conservative finding, never an edge),
//! any other literal is a recorded remote, and a non-literal or
//! missing source is a dynamic finding. JSON and override-file
//! `module` blocks count exactly like native ones. Lexical targets
//! resolve against the caller directory; the filesystem boundary
//! canonicalizes both edge ends, rejects escapes, and requires an
//! effective config file in every target. Cycles and missing targets
//! are errors; findings feed selection fallback, never edges.

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

pub use canonical::canonicalize_side;
pub(crate) use grammar::{source_from_json, source_from_native};

/// Project module edges to neutral pairs (rust `local_edge_pairs` precedent).
///
/// Single owner of the [`ModuleEdge`] projection into
/// [`reverse_closure`](velnor_actions_contract::reverse_closure);
/// source spellings never affect selection.
#[must_use]
pub fn module_edge_pairs(edges: &[ModuleEdge]) -> Vec<(String, String)> {
    edges
        .iter()
        .map(|edge| (edge.from.clone(), edge.to.clone()))
        .collect()
}
