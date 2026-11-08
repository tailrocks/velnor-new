// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{IsolatedCommand, MiseError, OsString, Path};

impl IsolatedCommand {
    pub fn in_qualification_container(mut self, container_id: &str) -> Result<Self, MiseError> {
        if self.program != "/usr/bin/docker"
            || container_id.len() != 64
            || !container_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.extra_env.iter().any(|(key, _)| key != "DOCKER_CONFIG")
        {
            return Err(MiseError::InvalidStepInput {
                field: "qualification_container".to_owned(),
                value: "invalid_container_command".to_owned(),
            });
        }
        let mut args: Vec<OsString> = vec![
            "--host".into(),
            "unix:///var/run/docker.sock".into(),
            "exec".into(),
            "--user".into(),
            format!(
                "{}:{}",
                rustix::process::geteuid().as_raw(),
                rustix::process::getegid().as_raw()
            )
            .into(),
        ];
        for (key, value) in &self.extra_env {
            let mut pair = key.clone();
            pair.push("=");
            pair.push(value);
            args.push("--env".into());
            args.push(pair);
        }
        args.extend([
            container_id.into(),
            "/native/docker".into(),
            "--host".into(),
            "unix:///run/velnor-q/docker.sock".into(),
        ]);
        args.append(&mut self.args);
        self.args = args;
        self.extra_env.clear();
        Ok(self)
    }

    pub fn with_private_docker_socket(mut self, socket: &Path) -> Result<Self, MiseError> {
        let path = socket.to_str().ok_or_else(|| MiseError::InvalidStepInput {
            field: "docker_socket".to_owned(),
            value: "invalid_utf8".to_owned(),
        })?;
        if self.program != "/usr/bin/docker"
            || !socket.is_absolute()
            || path.contains(['\n', '\r', '\0'])
        {
            return Err(MiseError::InvalidStepInput {
                field: "docker_socket".to_owned(),
                value: "invalid_private_endpoint".to_owned(),
            });
        }
        self.args
            .splice(0..0, ["--host".into(), format!("unix://{path}").into()]);
        Ok(self)
    }
}
