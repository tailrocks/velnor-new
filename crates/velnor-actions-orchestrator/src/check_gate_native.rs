#[path = "check_gate_native/install.rs"]
mod install;
#[path = "check_gate_native/proof.rs"]
mod proof;

pub(super) use proof::{valid_native_check_proof, valid_native_proofs, valid_qualified_proofs};
