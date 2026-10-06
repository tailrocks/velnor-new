//! Native validation metadata attached without granting execution authority.
use crate::ContractError;

impl super::HelperInvocation {
    /// Attach semantic input that the compiled SDK reconstructs independently.
    /// # Errors
    /// Rejects invalid descriptors or operation mismatches.
    pub fn with_native_validation_descriptor(
        mut self,
        descriptor: super::super::native_validation_descriptor::NativeValidationDescriptor,
    ) -> Result<Self, ContractError> {
        self.native_validation_descriptor = Some(Box::new(descriptor));
        self.validate()?;
        Ok(self)
    }
}
