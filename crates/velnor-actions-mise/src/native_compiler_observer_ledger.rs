// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CompilerCompletion, MiseError, NativeCompletion, NativeEvent, NativeInvocation, NativeLedger,
    OPERATION_COMPILATION, OPERATION_COMPILER_QUERY, ROLE_APPLICATION, ROLE_DEPENDENCY,
    is_supported_dependency_role, is_supported_native_tool_kind, is_supported_operation_kind,
    reject, validate_id, validate_lane,
};

impl NativeInvocation {
    pub fn validate(&self) -> Result<(), MiseError> {
        validate_id("event_id", &self.event_id)?;
        validate_id("recipe_id", &self.recipe_id)?;
        validate_id("package_id", &self.package_id)?;
        if !is_supported_dependency_role(&self.dependency_role) {
            return Err(reject("dependency_role", &self.dependency_role));
        }
        validate_lane(&self.lane_id)?;
        if !is_supported_native_tool_kind(&self.tool_kind) {
            return Err(reject("native_tool_kind", &self.tool_kind));
        }
        if self.executable.is_empty() || self.executable.contains('\0') {
            return Err(reject("executable", &self.executable));
        }
        if self.argv.is_empty() || self.argv.iter().any(|arg| arg.contains('\0')) {
            return Err(reject(
                "argv",
                self.argv.first().cloned().unwrap_or_default().as_str(),
            ));
        }
        for adapter in &self.adapter_chain {
            if adapter.is_empty() || adapter.contains('\0') {
                return Err(reject("adapter_chain", adapter));
            }
        }
        if let Some(parent) = &self.parent_event_id {
            validate_id("parent_event_id", parent)?;
        }
        if !is_supported_operation_kind(&self.operation_kind) {
            return Err(reject("operation_kind", &self.operation_kind));
        }
        if self.cwd.is_empty() || self.cwd.contains('\0') {
            return Err(reject("cwd", &self.cwd));
        }
        Ok(())
    }
}

impl NativeCompletion {
    fn as_compiler_completion(&self) -> CompilerCompletion {
        CompilerCompletion {
            exit_code: self.exit_code,
            signal: self.signal,
            interrupted: self.interrupted,
        }
    }

    pub fn validate(&self) -> Result<(), MiseError> {
        self.as_compiler_completion().validate()
    }

    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.as_compiler_completion().succeeded()
    }
}

impl NativeEvent {
    pub fn validate(&self) -> Result<(), MiseError> {
        self.invocation.validate()?;
        if let Some(completion) = &self.completion {
            completion.validate()?;
        }
        Ok(())
    }

    #[must_use]
    pub fn is_compilation(&self) -> bool {
        self.invocation.operation_kind == OPERATION_COMPILATION
    }

    #[must_use]
    pub fn is_query(&self) -> bool {
        self.invocation.operation_kind == OPERATION_COMPILER_QUERY
    }

    #[must_use]
    pub fn is_dependency_compilation(&self) -> bool {
        self.is_compilation() && self.invocation.dependency_role == ROLE_DEPENDENCY
    }

    #[must_use]
    pub fn is_application_compilation(&self) -> bool {
        self.is_compilation() && self.invocation.dependency_role == ROLE_APPLICATION
    }
}

impl NativeLedger {
    #[must_use]
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            unobserved_absolute_calls: 0,
        }
    }

    pub fn record(&mut self, event: NativeEvent) -> Result<(), MiseError> {
        event.validate()?;
        if self
            .events
            .iter()
            .any(|known| known.invocation.event_id == event.invocation.event_id)
        {
            return Err(reject("event_id", &event.invocation.event_id));
        }
        self.events.push(event);
        Ok(())
    }

    pub fn record_unobserved_absolute(&mut self, executable: &str) -> Result<(), MiseError> {
        if executable.is_empty() || executable.contains('\0') || !executable.starts_with('/') {
            return Err(reject("native_executable", executable));
        }
        self.unobserved_absolute_calls += 1;
        Ok(())
    }

    pub fn complete(
        &mut self,
        event_id: &str,
        completion: NativeCompletion,
    ) -> Result<(), MiseError> {
        completion.validate()?;
        let event = self
            .events
            .iter_mut()
            .find(|known| known.invocation.event_id == event_id)
            .ok_or_else(|| reject("event_id", event_id))?;
        if event.completion.is_some() {
            return Err(reject("completion", event_id));
        }
        event.completion = Some(completion);
        Ok(())
    }

    #[must_use]
    pub fn native_compilations(&self) -> u64 {
        self.events
            .iter()
            .filter(|event| event.is_compilation())
            .count() as u64
    }

    #[must_use]
    pub fn dependency_compilations(&self) -> u64 {
        self.events
            .iter()
            .filter(|event| event.is_dependency_compilation())
            .count() as u64
    }

    #[must_use]
    pub fn application_compilations(&self) -> u64 {
        self.events
            .iter()
            .filter(|event| event.is_application_compilation())
            .count() as u64
    }

    #[must_use]
    pub fn queries(&self) -> u64 {
        self.events.iter().filter(|event| event.is_query()).count() as u64
    }

    #[must_use]
    pub fn failed(&self) -> u64 {
        self.events
            .iter()
            .filter_map(|event| event.completion.as_ref())
            .filter(|completion| !completion.succeeded())
            .count() as u64
    }

    #[must_use]
    pub fn interrupted(&self) -> u64 {
        self.events
            .iter()
            .filter_map(|event| event.completion.as_ref())
            .filter(|completion| completion.interrupted)
            .count() as u64
    }

    #[must_use]
    pub fn unobserved_absolute_calls(&self) -> u64 {
        self.unobserved_absolute_calls
    }

    #[must_use]
    pub fn measured_native_compilations(&self) -> Option<u64> {
        if self.unobserved_absolute_calls > 0 {
            None
        } else {
            Some(self.native_compilations())
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
