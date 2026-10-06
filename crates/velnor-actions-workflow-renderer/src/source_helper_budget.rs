//! Typed optional-helper transport admission; malformed authority remains an error.
use std::collections::BTreeMap;

/// A measured transport limit that prevents this helper from running safely.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportBudgetExceeded {
    /// The independently enforced limit.
    pub reason: &'static str,
    /// Actual bytes, strings, or characters required.
    pub measured: usize,
    /// Supported bound for this runner.
    pub limit: usize,
}

/// Fully prepared immutable launcher and environment from the serializer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedTransport {
    pub(crate) run: String,
    pub(crate) environment: BTreeMap<String, String>,
}

impl PreparedTransport {
    /// The fixed launcher text.
    #[must_use]
    pub fn run(&self) -> &str {
        &self.run
    }

    /// Complete compiler-generated environment, including verified transport.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}

/// Optional producers may be omitted only for the explicit budget outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportAdmission {
    /// The exact serializer output fits the qualified runner.
    Supported(PreparedTransport),
    /// A measured transport limit prevents allocation of this producer.
    UnsupportedBudget(TransportBudgetExceeded),
}
