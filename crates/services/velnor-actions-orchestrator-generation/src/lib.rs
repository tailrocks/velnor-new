//! Generation pipeline: validated input, tree rendering, and emission.
//!
//! [`prepare`] validates the repository into the shared
//! [`prepare::GenerationPreparation`]; [`generate`] renders the staged
//! `.github` tree from it ([`finalized`] IR over the [`attach`]
//! points, [`provenance`] records, [`routing`] dispatch) and commits
//! or previews the result; [`release_emit`] ([`release_identity`],
//! [`release_steps`]) and [`freshness_emit`] add the release and
//! freshness files the policy selects.

pub mod attach;
pub mod finalized;
pub mod freshness_emit;
pub mod generate;
pub mod prepare;
pub mod provenance;
pub mod release_checkouts;
pub mod release_emit;
pub mod release_identity;
pub mod release_steps;
pub mod routing;
