//! Generation-only Pages authority supplied by the compiled orchestration factory.
use velnor_actions_contract::workflow::pages::NativePagesDeploy;

/// An approved complete deployment descriptor; never deserialized from workflow IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePagesApproval {
    role: NativePagesDeploy,
}

impl NativePagesApproval {
    /// Bind the exact role resolved by the authoritative native SDK factory.
    /// Callers must derive pins, full CI and helper records from compiled owners.
    #[must_use]
    pub fn compiled(role: NativePagesDeploy) -> Self {
        Self { role }
    }

    /// Compare every source, artifact, full CI, invocation and action binding.
    #[must_use]
    pub fn admits(&self, role: &NativePagesDeploy) -> bool {
        &self.role == role
    }
}
