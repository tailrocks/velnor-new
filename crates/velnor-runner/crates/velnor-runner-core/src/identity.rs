//! Validated identifiers. Interchangeable strings are not used at this boundary.

use crate::error::IdError;

/// Capacity grant. Copying it does not reserve or release a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GrantId(u64);

impl GrantId {
    /// Construct a non-zero grant id.
    ///
    /// # Errors
    ///
    /// Returns [`IdError::Zero`] when `raw` is zero.
    pub const fn new(raw: u64) -> Result<Self, IdError> {
        if raw == 0 {
            Err(IdError::Zero)
        } else {
            Ok(Self(raw))
        }
    }

    /// Borrow the raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Worker slot identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkerId(u64);

impl WorkerId {
    /// Construct a non-zero worker id.
    ///
    /// # Errors
    ///
    /// Returns [`IdError::Zero`] when `raw` is zero.
    pub const fn new(raw: u64) -> Result<Self, IdError> {
        if raw == 0 {
            Err(IdError::Zero)
        } else {
            Ok(Self(raw))
        }
    }

    /// Borrow the raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Acquire intent. Replaying the same id must not mint a second grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AcquireIntentId(u64);

impl AcquireIntentId {
    /// Construct a non-zero intent id.
    ///
    /// # Errors
    ///
    /// Returns [`IdError::Zero`] when `raw` is zero.
    pub const fn new(raw: u64) -> Result<Self, IdError> {
        if raw == 0 {
            Err(IdError::Zero)
        } else {
            Ok(Self(raw))
        }
    }

    /// Borrow the raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Provision intent. Distinct from the acquire intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProvisionIntentId(u64);

impl ProvisionIntentId {
    /// Construct a non-zero provision intent.
    ///
    /// # Errors
    ///
    /// Returns [`IdError::Zero`] when `raw` is zero.
    pub const fn new(raw: u64) -> Result<Self, IdError> {
        if raw == 0 {
            Err(IdError::Zero)
        } else {
            Ok(Self(raw))
        }
    }

    /// Borrow the raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Controller epoch. A stale epoch cannot mutate current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Epoch(u64);

impl Epoch {
    /// Construct an epoch. Zero is the first epoch.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Borrow the raw value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// GitHub request id. Preserved as int64 end to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(i64);

impl RequestId {
    /// Construct a non-negative request id. Zero is a real id.
    ///
    /// # Errors
    ///
    /// Returns [`IdError::Negative`] for negative values.
    pub const fn new(raw: i64) -> Result<Self, IdError> {
        if raw < 0 {
            Err(IdError::Negative)
        } else {
            Ok(Self(raw))
        }
    }

    /// Borrow the raw value.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// Service message id. Zero is real. Negative one is the synthetic marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageId(i64);

impl MessageId {
    /// Synthetic initial statistics marker. Never acknowledge it.
    pub const SYNTHETIC: Self = Self(-1);

    /// Construct a message id. Negative one is [`Self::SYNTHETIC`].
    #[must_use]
    pub const fn new(raw: i64) -> Self {
        Self(raw)
    }

    /// Borrow the raw value.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    /// Real service ids are zero or positive.
    #[must_use]
    pub const fn is_ackable(self) -> bool {
        self.0 >= 0
    }
}
