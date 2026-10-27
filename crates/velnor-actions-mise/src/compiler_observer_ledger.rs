// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CompilerCompletion, CompilerEvent, CompilerInvocation, CompilerLedger, MiseError,
    OPERATION_COMPILATION, OPERATION_COMPILER_QUERY, OsString, ROLE_APPLICATION, ROLE_DEPENDENCY,
    TOOL_KIND_RUSTDOC, is_supported_dependency_role, is_supported_doc_phase,
    is_supported_operation_kind, is_supported_tool_kind, reject, validate_id, validate_lane,
};

impl CompilerInvocation {
    pub fn validate(&self) -> Result<(), MiseError> {
        validate_id("event_id", &self.event_id)?;
        validate_id("recipe_id", &self.recipe_id)?;
        validate_id("package_id", &self.package_id)?;
        if !is_supported_dependency_role(&self.dependency_role) {
            return Err(reject("dependency_role", &self.dependency_role));
        }
        validate_lane(&self.lane_id)?;
        if !is_supported_tool_kind(&self.tool_kind) {
            return Err(reject("tool_kind", &self.tool_kind));
        }
        if self.executable.is_empty() || self.executable.contains('\0') {
            return Err(reject("executable", &self.executable));
        }
        if self.argv.is_empty() || self.argv.iter().any(|arg| arg.contains('\0')) {
            return Err(reject(
                "argv",
                &self.argv.first().cloned().unwrap_or_default(),
            ));
        }
        for wrapper in &self.wrapper_chain {
            if wrapper.is_empty() || wrapper.contains('\0') {
                return Err(reject("wrapper_chain", wrapper));
            }
        }
        if !is_supported_operation_kind(&self.operation_kind) {
            return Err(reject("operation_kind", &self.operation_kind));
        }
        match (&self.tool_kind as &str, &self.doc_phase) {
            (TOOL_KIND_RUSTDOC, Some(phase)) => {
                if !is_supported_doc_phase(phase) {
                    return Err(reject("doc_phase", phase));
                }
            }
            (TOOL_KIND_RUSTDOC, None) => {
                return Err(reject("doc_phase", "rustdoc_requires_doc_phase"));
            }
            (_, Some(phase)) => {
                return Err(reject("doc_phase", phase));
            }
            (_, None) => {}
        }
        if self.cwd.is_empty() || self.cwd.contains('\0') {
            return Err(reject("cwd", &self.cwd));
        }
        Ok(())
    }
}

impl CompilerCompletion {
    pub fn validate(&self) -> Result<(), MiseError> {
        if let Some(code) = self.exit_code
            && !(0..=255).contains(&code)
        {
            return Err(reject("exit_code", &code.to_string()));
        }
        if let Some(signal) = self.signal
            && !(1..=64).contains(&signal)
        {
            return Err(reject("signal", &signal.to_string()));
        }
        match (self.exit_code, self.signal, self.interrupted) {
            (Some(_), Some(_), _) => Err(reject("completion", "exit_and_signal")),
            (Some(_), None, true) | (None, Some(_), true) => {
                Err(reject("completion", "interrupted_with_status"))
            }
            (None, None, false) => Err(reject("completion", "no_terminal_status")),
            _ => Ok(()),
        }
    }

    #[must_use]
    pub fn succeeded(&self) -> bool {
        !self.interrupted && self.exit_code == Some(0)
    }
}

impl CompilerEvent {
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

impl CompilerLedger {
    #[must_use]
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn record(&mut self, event: CompilerEvent) -> Result<(), MiseError> {
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

    pub fn complete(
        &mut self,
        event_id: &str,
        completion: CompilerCompletion,
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
    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

pub fn compose_observer_env(
    rust_env: Vec<(OsString, OsString)>,
    native_env: Vec<(OsString, OsString)>,
) -> Result<Vec<(OsString, OsString)>, MiseError> {
    for (key, _) in &native_env {
        let name = key.to_string_lossy();
        if name.is_empty() || name.contains('\0') {
            return Err(reject("observer_env_key", &name));
        }
        if rust_env.iter().any(|(known, _)| known == key) {
            return Err(reject("observer_env_key", &name));
        }
    }
    let mut merged = rust_env;
    merged.extend(native_env);
    Ok(merged)
}
