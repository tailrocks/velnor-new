// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Fixture, api_server, runtime};
use serde_json::Value;
use std::{error::Error, fs, path::PathBuf, process::Output};

impl Fixture {
    pub(crate) fn invoke_publication_boundary(&self) -> Result<Output, std::io::Error> {
        self.invoke_publication_boundary_inner()
            .map_err(|error| std::io::Error::other(error.to_string()))
    }

    fn invoke_publication_boundary_inner(&self) -> Result<Output, Box<dyn Error + Send + Sync>> {
        let _server = api_server::start_for_isolated_worker(self)?;
        let (mut command, receipt, nonce) = self.publication_boundary_command()?;
        let output = command.output()?;
        verify_boundary_receipt(&output, &receipt, &nonce)?;
        Ok(output)
    }

    pub(super) fn publication_boundary_command(
        &self,
    ) -> Result<(std::process::Command, PathBuf, String), Box<dyn Error + Send + Sync>> {
        let binary =
            std::env::var_os("VELNOR_IMAGE_TEST_WORKER").ok_or("missing boundary worker")?;
        let input = self
            .state
            .parent()
            .ok_or("missing fixture directory")?
            .join("boundary-input.json");
        let receipt = input.with_file_name("boundary-child-receipt");
        let nonce = std::env::var("VELNOR_GH_FIXTURE_NONCE")?;
        let state: Value = serde_json::from_slice(&fs::read(&self.state)?)?;
        fs::write(
            &input,
            serde_json::to_vec(&serde_json::json!({
                "profile": self.profile.name(),
                "source": self.source,
                "producer_run_id": self.producer_run_id,
                "producer_run_attempt": self.producer_run_attempt,
                "job_attestation_run_id": self.profile.job_attestor_run_id(),
                "artifact_id": self.artifact_id,
                "archive": self.archive,
                "record": self.record,
                "qualification_proof": self.qualification_proof,
                "scratch": self.scratch,
                "receipt": self.receipt,
                "qualification_attestation": state["qualification_attestation"],
                "attestation": state["attestation"],
            "boundary_receipt": receipt,
            "nonce": nonce,
            }))?,
        )?;
        let original = runtime::publication_command(self)?;
        let mut command = std::process::Command::new(binary);
        command.env_clear();
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        for (key, value) in original.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        command.env("VELNOR_IMAGE_TEST_INPUT", input);
        command.args([
            "--exact",
            "image_maintenance::publication_boundary_worker::publication",
            "--ignored",
            "--nocapture",
        ]);
        Ok((command, receipt, nonce))
    }
}

pub(super) fn verify_boundary_receipt(
    output: &Output,
    receipt: &std::path::Path,
    nonce: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if output.status.success() && fs::read_to_string(receipt)? != nonce {
        return Err("boundary worker receipt mismatch".into());
    }
    Ok(())
}
