//! Binary tests for normal cache payload operations.

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Output;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

const IDENTITY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    root: PathBuf,
    runner_temp: PathBuf,
    home: PathBuf,
    mise_data_dir: PathBuf,
    cargo_home: PathBuf,
    rustup_home: PathBuf,
    cache_key: String,
}

impl Fixture {
    fn new(parent: &Path, name: &str, state: &str) -> Result<Self, Box<dyn Error>> {
        let root = fs::canonicalize(parent.join(name)).or_else(|_| {
            fs::create_dir_all(parent.join(name))?;
            fs::canonicalize(parent.join(name))
        })?;
        let runner_temp = root.join("runner-temp");
        let home = root.join("home");
        let mise_data_dir = home.join(".local/share/mise");
        let cargo_home = runner_temp.join("velnor/cargo");
        let rustup_home = runner_temp.join("velnor/rustup");
        let fixture = Self {
            root,
            runner_temp,
            home,
            mise_data_dir,
            cargo_home,
            rustup_home,
            cache_key: format!("mise-tools-v2-payload-v3-{IDENTITY}"),
        };
        fixture.write_state(state)?;
        Ok(fixture)
    }

    fn write_state(&self, state: &str) -> Result<(), Box<dyn Error>> {
        fs::create_dir_all(self.cargo_home.join("bin"))?;
        fs::create_dir_all(&self.mise_data_dir)?;
        fs::create_dir_all(&self.rustup_home)?;
        fs::write(self.cargo_home.join("bin/cargo-tool"), state)?;
        fs::write(self.cargo_home.join(".crates2.json"), state)?;
        fs::write(self.cargo_home.join(".crates.toml"), state)?;
        fs::write(self.mise_data_dir.join("tool-state"), state)?;
        fs::write(self.rustup_home.join("rustup-state"), state)?;
        Ok(())
    }

    fn tool_state(&self) -> Result<String, Box<dyn Error>> {
        Ok(fs::read_to_string(self.mise_data_dir.join("tool-state"))?)
    }

    fn stage_path(&self) -> Result<PathBuf, Box<dyn Error>> {
        let output = self.output_path();
        fs::write(&output, "")?;
        let vars = vec![
            (
                "VELNOR_INTERNAL_OP".to_owned(),
                "normal-tools-cache-stage-path-v1".to_owned(),
            ),
            ("RUNNER_TEMP".to_owned(), path_text(&self.runner_temp)?),
            ("GITHUB_RUN_ID".to_owned(), "700".to_owned()),
            ("GITHUB_RUN_ATTEMPT".to_owned(), "1".to_owned()),
            ("VELNOR_NORMAL_CACHE_KEY".to_owned(), self.cache_key.clone()),
            ("GITHUB_OUTPUT".to_owned(), path_text(&output)?),
        ];
        let result = self.spawn(&vars)?;
        assert_eq!(code(&result), 0, "{:?}", result.stderr);
        let body = fs::read_to_string(output)?;
        let value = body
            .strip_prefix("stage_path=")
            .and_then(|line| line.strip_suffix('\n'))
            .ok_or("stage path output is invalid")?;
        Ok(PathBuf::from(value))
    }

    fn export(&self, stage_path: &Path) -> Result<Output, Box<dyn Error>> {
        self.run_transfer(
            "export-normal-tools-cache-payload-v1",
            stage_path,
            None,
            None,
        )
    }

    fn import(
        &self,
        stage_path: &Path,
        hit: bool,
        matched_key: Option<&str>,
    ) -> Result<Output, Box<dyn Error>> {
        self.run_transfer(
            "import-normal-tools-cache-payload-v1",
            stage_path,
            Some(hit),
            matched_key,
        )
    }

    fn run_transfer(
        &self,
        operation: &str,
        stage_path: &Path,
        hit: Option<bool>,
        matched_key: Option<&str>,
    ) -> Result<Output, Box<dyn Error>> {
        let output = self.output_path();
        fs::write(&output, "")?;
        let mut vars = vec![
            ("VELNOR_INTERNAL_OP".to_owned(), operation.to_owned()),
            ("RUNNER_TEMP".to_owned(), path_text(&self.runner_temp)?),
            ("GITHUB_RUN_ID".to_owned(), "700".to_owned()),
            ("GITHUB_RUN_ATTEMPT".to_owned(), "1".to_owned()),
            ("HOME".to_owned(), path_text(&self.home)?),
            ("VELNOR_NORMAL_CACHE_ENABLED".to_owned(), "true".to_owned()),
            (
                "VELNOR_NORMAL_CACHE_IDENTITY".to_owned(),
                IDENTITY.to_owned(),
            ),
            ("VELNOR_NORMAL_CACHE_KEY".to_owned(), self.cache_key.clone()),
            (
                "VELNOR_NORMAL_CACHE_MISE_DATA_DIR".to_owned(),
                path_text(&self.mise_data_dir)?,
            ),
            (
                "VELNOR_NORMAL_CACHE_CARGO_HOME".to_owned(),
                path_text(&self.cargo_home)?,
            ),
            (
                "VELNOR_NORMAL_CACHE_RUSTUP_HOME".to_owned(),
                path_text(&self.rustup_home)?,
            ),
            (
                "VELNOR_NORMAL_CACHE_STAGE_PATH".to_owned(),
                path_text(stage_path)?,
            ),
            ("GITHUB_OUTPUT".to_owned(), path_text(&output)?),
        ];
        if let Some(hit) = hit {
            vars.push((
                "VELNOR_NORMAL_CACHE_HIT".to_owned(),
                if hit { "true" } else { "false" }.to_owned(),
            ));
        }
        if let Some(key) = matched_key {
            vars.push(("VELNOR_NORMAL_CACHE_MATCHED_KEY".to_owned(), key.to_owned()));
        }
        self.spawn(&vars)
    }

    fn spawn(&self, vars: &[(String, String)]) -> Result<Output, Box<dyn Error>> {
        let borrowed = vars
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        spawn_isolated(&[], &borrowed, &self.root)
    }

    fn output_path(&self) -> PathBuf {
        self.root.join("github-output")
    }
}

#[test]
fn cold_cache_absence_keeps_live_tool_state() -> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("normal-cache-cold")?;
    let fixture = Fixture::new(&temp, "consumer", "cold-state")?;
    let stage_path = fixture.stage_path()?;

    let result = fixture.import(&stage_path, false, None)?;

    assert_eq!(code(&result), 0, "{:?}", result.stderr);
    let output = fs::read_to_string(fixture.output_path())?;
    assert!(output.contains("restore_result=miss\n"), "{output}");
    assert!(!output.contains("restore_admitted=true\n"), "{output}");
    assert_eq!(fixture.tool_state()?, "cold-state");
    assert!(!stage_path.exists());
    cleanup(&temp);
    Ok(())
}

#[test]
fn warm_cache_import_restores_the_exported_tool_state() -> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("normal-cache-warm")?;
    let cold = Fixture::new(&temp, "cold", "saved-state")?;
    let cold_stage = cold.stage_path()?;
    let export = cold.export(&cold_stage)?;
    assert_eq!(code(&export), 0, "{:?}", export.stderr);
    let export_output = fs::read_to_string(cold.output_path())?;
    assert!(
        export_output.contains("export_result=staged\n"),
        "{export_output}"
    );
    assert!(
        export_output.contains("snapshot_admitted=true\n"),
        "{export_output}"
    );

    let warm = Fixture::new(&temp, "warm", "stale-state")?;
    let warm_stage = warm.stage_path()?;
    copy_tree(&cold_stage, &warm_stage)?;
    let restore = warm.import(&warm_stage, true, Some(&warm.cache_key))?;

    assert_eq!(code(&restore), 0, "{:?}", restore.stderr);
    let output = fs::read_to_string(warm.output_path())?;
    assert!(output.contains("restore_result=hit\n"), "{output}");
    assert!(output.contains("payload_admitted=true\n"), "{output}");
    assert!(output.contains("restore_admitted=true\n"), "{output}");
    assert_eq!(warm.tool_state()?, "saved-state");
    cleanup(&temp);
    Ok(())
}

#[test]
fn malformed_payload_is_removed_without_changing_live_roots() -> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("normal-cache-malformed")?;
    let cold = Fixture::new(&temp, "cold", "saved-state")?;
    let cold_stage = cold.stage_path()?;
    let export = cold.export(&cold_stage)?;
    assert_eq!(code(&export), 0, "{:?}", export.stderr);

    let warm = Fixture::new(&temp, "warm", "live-state")?;
    let warm_stage = warm.stage_path()?;
    copy_tree(&cold_stage, &warm_stage)?;
    fs::write(warm_stage.join("manifest.json"), b"{\"schema\":2}")?;
    let unrelated = warm_stage
        .parent()
        .ok_or("stage parent missing")?
        .join("keep");
    fs::write(&unrelated, "keep")?;

    let restore = warm.import(&warm_stage, true, Some(&warm.cache_key))?;

    assert_eq!(code(&restore), 1, "{:?}", restore.stdout);
    assert!(String::from_utf8_lossy(&restore.stderr).contains("normal cache import failed"));
    assert_eq!(fs::read_to_string(warm.output_path())?, "");
    assert_eq!(warm.tool_state()?, "live-state");
    assert!(!warm_stage.exists());
    assert_eq!(fs::read_to_string(unrelated)?, "keep");
    cleanup(&temp);
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if file_type.is_symlink() {
            symlink(fs::read_link(source_path)?, destination_path)?;
        } else {
            fs::copy(source_path, destination_path)?;
        }
    }
    fs::set_permissions(destination, fs::metadata(source)?.permissions())?;
    Ok(())
}

fn path_text(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(path.to_str().ok_or("path is not UTF-8")?.to_owned())
}
