// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::{Child, Output},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{Fixture, runtime};

pub(crate) struct PublicationProcess {
    child: Option<Child>,
    ready_path: PathBuf,
    release_path: PathBuf,
    reference: String,
}

impl Fixture {
    pub(crate) fn update_github_state(
        &self,
        update: impl FnOnce(&mut Value) -> Result<(), Box<dyn Error>>,
    ) -> Result<(), Box<dyn Error>> {
        let mut state = self.read_github_state()?;
        update(&mut state)?;
        self.write_github_state(&state)
    }

    pub(crate) fn set_ci_job_pages(&self, pages: Value) -> Result<(), Box<dyn Error>> {
        self.update_github_state(|state| {
            state["ci_job_pages"] = pages;
            Ok(())
        })
    }

    pub(crate) fn start_at_manifest_lookup(&self) -> Result<PublicationProcess, Box<dyn Error>> {
        let mut state = self.read_github_state()?;
        let reference = state["crane"]["tag_reference"]
            .as_str()
            .ok_or("fixture tag reference is missing")?
            .to_owned();
        state["crane"]["manifest_lookup_barrier"] = json!({"reference":reference});
        self.write_github_state(&state)?;
        remove_if_present(&self.error_path)?;
        let home = self
            .state
            .parent()
            .ok_or("fixture state has no parent directory")?;
        let ready_path = home.join(".image-publication-manifest-ready");
        let release_path = home.join(".image-publication-manifest-release");
        remove_if_present(&ready_path)?;
        remove_if_present(&release_path)?;
        let child = runtime::publication_command(self)?.spawn()?;
        Ok(PublicationProcess {
            child: Some(child),
            ready_path,
            release_path,
            reference,
        })
    }

    fn read_github_state(&self) -> Result<Value, Box<dyn Error>> {
        Ok(serde_json::from_slice(&fs::read(&self.state)?)?)
    }

    fn write_github_state(&self, state: &Value) -> Result<(), Box<dyn Error>> {
        fs::write(&self.state, serde_json::to_vec(state)?)?;
        Ok(())
    }
}

impl PublicationProcess {
    pub(crate) fn wait_until_blocked(&mut self) -> Result<(), Box<dyn Error>> {
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            if let Some(child) = self.child.as_mut()
                && let Some(status) = child.try_wait()?
            {
                return Err(format!("publication exited before manifest barrier: {status}").into());
            }
            match fs::symlink_metadata(&self.ready_path) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    let reference = fs::read_to_string(&self.ready_path)?;
                    if reference != self.reference {
                        return Err(
                            "manifest barrier reference did not match the selected tag".into()
                        );
                    }
                    return Ok(());
                }
                Ok(_) => return Err("invalid manifest barrier ready marker".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            if Instant::now() >= deadline {
                return Err("publication did not reach the manifest lookup barrier".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    pub(crate) fn release_and_wait(&mut self) -> Result<Output, Box<dyn Error>> {
        self.release()?;
        let child = self
            .child
            .take()
            .ok_or("publication process was already joined")?;
        Ok(child.wait_with_output()?)
    }

    fn release(&self) -> Result<(), Box<dyn Error>> {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.release_path)
        {
            Ok(mut file) => file.write_all(self.reference.as_bytes())?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
}

impl Drop for PublicationProcess {
    fn drop(&mut self) {
        let _release_result = self.release();
        if let Some(child) = self.child.take() {
            let _wait_result = child.wait_with_output();
        }
    }
}

fn remove_if_present(path: &std::path::Path) -> Result<(), std::io::Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
