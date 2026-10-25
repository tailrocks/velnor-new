use crate::error::IdError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GrantId(u64);

impl GrantId {
    pub const fn new(raw: u64) -> Result<Self, IdError> {
        if raw == 0 {
            Err(IdError::Zero)
        } else {
            Ok(Self(raw))
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkerId(u64);

impl WorkerId {
    pub const fn new(raw: u64) -> Result<Self, IdError> {
        if raw == 0 {
            Err(IdError::Zero)
        } else {
            Ok(Self(raw))
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AcquireIntentId(u64);

impl AcquireIntentId {
    pub const fn new(raw: u64) -> Result<Self, IdError> {
        if raw == 0 {
            Err(IdError::Zero)
        } else {
            Ok(Self(raw))
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProvisionIntentId(u64);

impl ProvisionIntentId {
    pub const fn new(raw: u64) -> Result<Self, IdError> {
        if raw == 0 {
            Err(IdError::Zero)
        } else {
            Ok(Self(raw))
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Epoch(u64);

impl Epoch {
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(i64);

impl RequestId {
    pub const fn new(raw: i64) -> Result<Self, IdError> {
        if raw < 0 {
            Err(IdError::Negative)
        } else {
            Ok(Self(raw))
        }
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageId(i64);

impl MessageId {
    pub const SYNTHETIC: Self = Self(-1);

    #[must_use]
    pub const fn new(raw: i64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    #[must_use]
    pub const fn is_ackable(self) -> bool {
        self.0 >= 0
    }
}
