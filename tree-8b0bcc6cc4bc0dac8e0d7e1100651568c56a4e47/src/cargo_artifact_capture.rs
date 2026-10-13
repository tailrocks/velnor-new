//! Versioned evidence from Cargo's public JSON stdout protocol.
//! `fresh` is Cargo's observation; it never proves native compiler execution.
//! Build-script records may describe cached output. Public JSON has no unit ID.

use serde::Serialize;
use std::io::{Read, Write};

#[path = "cargo_artifact_stderr.rs"]
mod stderr;
pub use stderr::CargoStderrCapture;
#[path = "cargo_artifact_command.rs"]
mod command;
pub use command::{
    CargoBoundStderr, CargoBoundStdout, CargoCommandBinding, CargoCommandCompletion,
};

const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_CAPTURE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RECORDS: usize = 16 * 1024;
#[path = "cargo_artifact_protocol.rs"]
mod protocol;
pub use protocol::*;

#[path = "cargo_artifact_frontend.rs"]
mod frontend;
pub use frontend::{CargoFrontend, cargo_frontend};
#[path = "cargo_artifact_proof.rs"]
mod proof;

#[derive(Debug, Serialize)]
pub struct CargoCaptureReport {
    pub schema_version: u32,
    pub frontend: CargoFrontend,
    pub command: CargoCommandBinding,
    pub messages: Vec<CargoMessage>,
    pub diagnostics: Vec<CargoParseDiagnostic>,
    pub stdout_bytes: u64,
    pub stdout_eof: bool,
    #[serde(skip_serializing)]
    pub read_error: Option<String>,
    #[serde(skip_serializing)]
    pub forward_error: Option<String>,
    pub read_failed: bool,
    pub forward_failed: bool,
    pub build_script_environment_entries: usize,
    pub unterminated_record: bool,
    pub capture_truncated: bool,
    pub process_success: bool,
    pub process_exit_code: Option<i32>,
    pub build_finished: Option<bool>,
    pub stderr: CargoStderrCapture,
    pub public_json_unit_identity_available: bool,
    pub package_source_authority: Option<String>,
    /// Capture completeness only; package/source/unit attribution is separate.
    pub protocol_complete: bool,
    pub native_process_capture_complete: bool,
    #[serde(skip)]
    native_proof: Option<proof::NativeCaptureProof>,
}

/// Only this owning reader can mint stdout EOF evidence. The captured stream
/// must be the actual child stdout; callers cannot supply a completion boolean.
pub struct CargoStdoutCapture {
    messages: Vec<CargoMessage>,
    diagnostics: Vec<CargoParseDiagnostic>,
    bytes: u64,
    eof: bool,
    read_error: Option<String>,
    forward_error: Option<String>,
    unterminated_record: bool,
    capture_truncated: bool,
    command: CargoCommandBinding,
}

impl CargoStdoutCapture {
    /// Forward stdout byte-for-byte while independently observing public JSON.
    pub fn read(stdout: CargoBoundStdout, forward: &mut impl Write) -> Self {
        Self::read_stream(stdout.stream, forward, stdout.binding)
    }

    fn read_stream(
        mut stdout: impl Read,
        forward: &mut impl Write,
        command: CargoCommandBinding,
    ) -> Self {
        let mut capture = Self {
            messages: Vec::new(),
            diagnostics: Vec::new(),
            bytes: 0,
            eof: false,
            read_error: None,
            forward_error: None,
            unterminated_record: false,
            capture_truncated: false,
            command,
        };
        let mut chunk = [0; 8192];
        let mut line = Vec::new();
        let mut line_number = 1;
        let mut oversized = false;
        let parse_json = cargo_frontend(capture.command.arguments()) != CargoFrontend::Unsupported;
        loop {
            let size = match stdout.read(&mut chunk) {
                Ok(0) => {
                    capture.eof = true;
                    break;
                }
                Ok(size) => size,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    capture.read_error = Some(error.to_string());
                    break;
                }
            };
            capture.bytes = match capture.bytes.checked_add(size as u64) {
                Some(bytes) => bytes,
                None => {
                    capture.capture_truncated = true;
                    u64::MAX
                }
            };
            if capture.forward_error.is_none() {
                if let Err(error) = forward.write_all(&chunk[..size]) {
                    capture.forward_error = Some(error.to_string());
                }
            }
            if !parse_json {
                continue;
            }
            if capture.bytes > MAX_CAPTURE_BYTES {
                capture.capture_truncated = true;
                continue;
            }
            for byte in &chunk[..size] {
                if *byte == b'\n' {
                    capture.record(&line, line_number, oversized);
                    line.clear();
                    oversized = false;
                    line_number += 1;
                } else if line.len() < MAX_LINE_BYTES {
                    line.push(*byte);
                } else {
                    oversized = true;
                }
            }
        }
        if !line.is_empty() || oversized {
            capture.unterminated_record = true;
            capture.record(&line, line_number, oversized);
        }
        capture
    }

    fn record(&mut self, line: &[u8], number: usize, oversized: bool) {
        if self.messages.len() + self.diagnostics.len() >= MAX_RECORDS {
            self.capture_truncated = true;
            return;
        }
        let parsed = if oversized {
            Err(CargoParseDiagnostic {
                line: number,
                detail: "Cargo JSON record truncated at capture bound".into(),
                reason: CargoDiagnosticReason::RecordTooLarge,
            })
        } else {
            parse_cargo_message(line).map_err(|mut diagnostic| {
                diagnostic.line = number;
                diagnostic
            })
        };
        match parsed {
            Ok(message) => {
                if let CargoMessage::CompilerArtifact(artifact) = &message {
                    if self.messages.iter().any(|previous| matches!(previous,
                        CargoMessage::CompilerArtifact(prior) if prior.package_id == artifact.package_id
                        && prior.manifest_path == artifact.manifest_path && prior.target == artifact.target
                        && prior.profile == artifact.profile && prior.features == artifact.features)) {
                        self.diagnostics.push(CargoParseDiagnostic { line: number, reason: CargoDiagnosticReason::DuplicateArtifactIdentity, detail: "duplicate or conflicting Cargo artifact identity; public JSON cannot distinguish units".into() });
                    }
                }
                self.messages.push(message);
            }
            Err(diagnostic) => self.diagnostics.push(diagnostic),
        }
    }

    /// Actual wait result, never a caller-authored success or completeness flag.
    pub fn finish(
        self,
        completion: CargoCommandCompletion,
        stderr: CargoStderrCapture,
    ) -> CargoCaptureReport {
        let frontend = cargo_frontend(self.command.arguments());
        let status = completion.status();
        let finished: Vec<bool> = self
            .messages
            .iter()
            .filter_map(|message| match message {
                CargoMessage::BuildFinished { success, .. } => Some(*success),
                _ => None,
            })
            .collect();
        let build_finished = if finished.len() == 1 {
            finished.first().copied()
        } else {
            None
        };
        let final_message = matches!(
            self.messages.last(),
            Some(CargoMessage::BuildFinished { .. })
        );
        let native_process_capture_complete = self.eof
            && self.read_error.is_none()
            && self.forward_error.is_none()
            && stderr.complete()
            && self.command == *completion.binding()
            && stderr.binding() == &self.command;
        let protocol_complete = native_process_capture_complete
            && !self.unterminated_record
            && !self.capture_truncated
            && self.diagnostics.is_empty()
            && final_message
            && build_finished == Some(status.success())
            && frontend != CargoFrontend::Unsupported;
        let build_script_environment_entries = self
            .messages
            .iter()
            .filter_map(|message| match message {
                CargoMessage::BuildScriptExecuted(script) => Some(script.env.len()),
                _ => None,
            })
            .sum();
        let mut report = CargoCaptureReport {
            schema_version: CARGO_CAPTURE_SCHEMA_VERSION,
            frontend,
            messages: self.messages,
            diagnostics: self.diagnostics,
            stdout_bytes: self.bytes,
            stdout_eof: self.eof,
            read_failed: self.read_error.is_some(),
            forward_failed: self.forward_error.is_some(),
            build_script_environment_entries,
            read_error: self.read_error,
            forward_error: self.forward_error,
            unterminated_record: self.unterminated_record,
            capture_truncated: self.capture_truncated,
            process_success: status.success(),
            process_exit_code: status.code(),
            build_finished,
            protocol_complete,
            native_process_capture_complete,
            stderr,
            public_json_unit_identity_available: false,
            package_source_authority: None,
            command: self.command,
            native_proof: None,
        };
        report.native_proof = Some(proof::NativeCaptureProof::seal(
            &report,
            native_process_capture_complete,
            status,
        ));
        report
    }
}

#[cfg(test)]
#[path = "cargo_artifact_capture_tests.rs"]
mod tests;
