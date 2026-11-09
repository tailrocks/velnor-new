// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

impl Fixture {
    pub(crate) fn test_interpreter_boundary(&self) -> Result<(), Box<dyn Error>> {
        let fake_bin = self.fake_gh.parent().ok_or("missing fake bin")?;
        let sentinel = self.root.path().join("sentinel");
        let poison = self.root.path().join("poison");
        fs::create_dir(&poison)?;
        for module in ["base64", "hashlib"] {
            fs::write(
                poison.join(format!("{module}.py")),
                "import os\nopen(os.environ['SENTINEL'], 'w').write('poisoned')\n",
            )?;
        }
        for binary in ["python3", "bash"] {
            let path = fake_bin.join(binary);
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\ncase \"$*\" in *script.sh*|*hashlib*|*base64*) printf invoked > \"$SENTINEL\";; esac\nexec {} \"$@\"\n",
                    if binary == "bash" {
                        "/bin/bash"
                    } else {
                        "/usr/bin/python3"
                    }
                ),
            )?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
        }
        let directory = self.materialize()?;
        let output = self.execute_materialized(&directory)?;
        assert!(
            output.status.success(),
            "{} sentinel={:?}",
            output_text(&output),
            fs::read_to_string(&sentinel)
        );
        assert!(
            !sentinel.exists(),
            "untrusted interpreter or module executed"
        );
        Ok(())
    }
    pub(super) fn materialize(&self) -> Result<PathBuf, Box<dyn Error>> {
        let fake_bin = self.fake_gh.parent().ok_or("missing fake bin")?;
        let sentinel = self.root.path().join("sentinel");
        let poison = self.root.path().join("poison");
        let path = format!("{}:{}", fake_bin.display(), std::env::var("PATH")?);
        let github_env = self.root.path().join("materializer-env");
        fs::write(&github_env, "")?;
        let mut directory = PathBuf::new();
        for run in &self.materializers {
            let output = Command::new("/bin/bash")
                .args(["--noprofile", "--norc", "-e", "-o", "pipefail", "-c"])
                .arg(run)
                .env("PATH", &path)
                .env("PYTHONPATH", &poison)
                .env("SENTINEL", &sentinel)
                .env("RUNNER_TEMP", self.root.path())
                .env("GITHUB_ENV", &github_env)
                .env("VELNOR_FIXED_SCRIPT_DIR", &directory)
                .output()?;
            assert!(output.status.success(), "{}", output_text(&output));
            let env = fs::read_to_string(&github_env)?;
            if let Some(value) = env
                .lines()
                .find_map(|line| line.strip_prefix("VELNOR_FIXED_SCRIPT_DIR="))
            {
                directory = PathBuf::from(value);
            }
        }
        assert_eq!(
            fs::read(directory.join("script.sh"))?,
            self.script.as_bytes()
        );
        Ok(directory)
    }
}
