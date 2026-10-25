use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobTimeout(u16);

impl JobTimeout {
    pub const MIN_MINUTES: u16 = 1;

    pub const MAX_MINUTES: u16 = 360;

    pub const DEFAULT_MINUTES: u16 = 5;

    pub const DEFAULT: Self = Self(Self::DEFAULT_MINUTES);

    pub const PLAN: Self = Self::DEFAULT;

    pub const CRATE: Self = Self::DEFAULT;

    pub const REQUIRED: Self = Self::DEFAULT;

    pub const VALIDATOR: Self = Self::DEFAULT;

    pub const CANDIDATE: Self = Self::DEFAULT;

    pub const RELEASE: Self = Self::DEFAULT;

    pub const MSRV: Self = Self::DEFAULT;

    pub const PUBLISH: Self = Self::DEFAULT;

    pub const PRODUCT_COMPILER: Self = Self(10);

    pub const PRODUCT_QUALIFICATION: Self = Self(10);

    pub const PRODUCT_IMAGE_BUILD: Self = Self(30);

    #[must_use]
    pub fn is_valid_minutes(minutes: u32) -> bool {
        (u32::from(Self::MIN_MINUTES)..=u32::from(Self::MAX_MINUTES)).contains(&minutes)
    }

    pub fn check_config(minutes: u32, file: &str, key: &str) -> Result<(), ContractError> {
        if Self::is_valid_minutes(minutes) {
            return Ok(());
        }
        Err(ContractError::config(
            file,
            key,
            format!(
                "bad_timeout:{minutes}:expected_{}..={}",
                Self::MIN_MINUTES,
                Self::MAX_MINUTES
            ),
        ))
    }

    pub fn new(minutes: u16) -> Result<Self, ContractError> {
        let timeout = Self(minutes);
        timeout.validate()?;
        Ok(timeout)
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if !Self::is_valid_minutes(u32::from(self.0)) {
            return Err(ContractError::identity(
                "job.timeout_minutes",
                format!("bad_timeout:{}", self.0),
            ));
        }
        Ok(())
    }

    #[must_use]
    pub const fn minutes(&self) -> u16 {
        self.0
    }
}

impl TryFrom<u32> for JobTimeout {
    type Error = ContractError;

    fn try_from(minutes: u32) -> Result<Self, Self::Error> {
        let minutes = u16::try_from(minutes).map_err(|_| {
            ContractError::identity("job.timeout_minutes", format!("bad_timeout:{minutes}"))
        })?;
        Self::new(minutes)
    }
}
