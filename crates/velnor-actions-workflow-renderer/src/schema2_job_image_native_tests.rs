#![cfg(unix)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use crate::schema2::git_fixture;
use crate::yaml::Yaml;

use std::os::unix::fs::PermissionsExt;

#[test]
fn attestation_download_uses_only_uploaded_id_outside_the_checkout() {
    let workflow =
        super::super::render(&super::request()).expect("Render the job image candidate workflow.");
    let build = super::field(super::field(&workflow, "jobs"), "build-image-candidate");
    let Yaml::Seq(build_steps) = super::field(build, "steps") else {
        panic!("The build steps are necessary.");
    };
    let upload = super::step_named(build_steps, "Upload candidate archive and record");
    assert_eq!(
        super::string_field(super::field(upload, "with"), "path"),
        format!(
            "${{{{ runner.temp }}}}/job-image.oci.tar\n${{{{ runner.temp }}}}/job-image-candidate.json\n${{{{ runner.temp }}}}/{}",
            super::super::native::CLOSURE
        )
    );

    let attest = super::field(super::field(&workflow, "jobs"), "attest-image-candidate");
    let Yaml::Seq(steps) = super::field(attest, "steps") else {
        panic!("The attestation steps are necessary.");
    };
    let download = steps
        .iter()
        .find(|step| {
            super::optional_field(step, "uses")
                == Some(&Yaml::str(crate::steps::DOWNLOAD_ARTIFACT_USES))
        })
        .expect("The artifact download step is not available.");
    let inputs = super::field(download, "with");
    assert_eq!(
        super::field(inputs, "artifact-ids"),
        &Yaml::str("${{ needs.build-image-candidate.outputs.artifact_id }}")
    );
    assert!(super::optional_field(inputs, "name").is_none());
    assert_eq!(
        super::field(inputs, "path"),
        &Yaml::str("${{ runner.temp }}/privatecandidate")
    );
    let validate = super::string_field(
        super::step_named(steps, "Do a check of source and archive identity"),
        "run",
    );
    assert!(validate.contains("image-maintenance validate-job-candidate"));
    assert!(validate.contains("$RUNNER_TEMP/privatecandidate/job-image.oci.tar"));
    assert!(validate.contains("$RUNNER_TEMP/privatecandidate/job-image-candidate.json"));
    assert!(!validate.contains("$GITHUB_WORKSPACE/candidate"));
    let attest_step = steps
        .iter()
        .find(|step| {
            super::optional_field(step, "uses")
                == Some(&Yaml::str(crate::steps::ATTEST_BUILD_PROVENANCE_USES))
        })
        .expect("The provenance attestation is necessary.");
    assert_eq!(
        super::field(super::field(attest_step, "with"), "subject-path"),
        &Yaml::str(format!(
            "${{{{ runner.temp }}}}/privatecandidate/job-image.oci.tar\n${{{{ runner.temp }}}}/privatecandidate/job-image-candidate.json\n${{{{ runner.temp }}}}/privatecandidate/{}",
            super::super::native::CLOSURE
        ))
    );
}

#[test]
fn clean_source_build_keeps_downloaded_attestation_files_outside_checkout() {
    let fixture = Fixture::new();
    let downloaded = fixture.runner_temp.join("privatecandidate");
    fs::create_dir(&downloaded).expect("create temporary artifact directory");
    for name in [
        "job-image.oci.tar",
        "job-image-candidate.json",
        super::super::native::CLOSURE,
    ] {
        fs::write(downloaded.join(name), b"test artifact").expect("write test artifact");
    }

    let marker = fixture.runner_temp.join("compile-ran");
    let output = fixture.run_source_script(&marker);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(marker.is_file(), "clean source reaches the mocked build");

    let first_binary = fixture.binary_from_env_file();
    assert!(first_binary.starts_with(&fixture.runner_temp));
    assert!(!first_binary.starts_with(&fixture.root));
    assert_eq!(
        fs::read(&first_binary).expect("read copied native executable"),
        b"native image cli"
    );
    assert_eq!(
        fs::metadata(first_binary.parent().expect("private binary directory"))
            .expect("private directory metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&first_binary)
            .expect("private binary metadata")
            .permissions()
            .mode()
            & 0o777,
        0o500
    );

    let second_marker = fixture.runner_temp.join("compile-ran-again");
    let second = fixture.run_source_script(&second_marker);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_binary = fixture.binary_from_env_file();
    assert_ne!(first_binary, second_binary);
    assert_eq!(
        fs::read(&first_binary).expect("first private executable remains intact"),
        b"native image cli"
    );
}

#[test]
fn tracked_changes_fail_before_the_mocked_compiler_runs() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("tracked.txt"), b"changed").expect("modify tracked file");
    fixture.assert_dirty_source_stops_build();
}

#[test]
fn staged_index_changes_fail_before_the_mocked_compiler_runs() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("tracked.txt"), b"staged").expect("modify tracked file");
    git(&fixture.root, ["add", "tracked.txt"]);
    fixture.assert_dirty_source_stops_build();
}

#[test]
fn newline_untracked_path_fails_before_the_mocked_compiler_runs() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("untracked\nname"), b"untracked").expect("create newline path");
    fixture.assert_dirty_source_stops_build();
}

#[test]
fn injected_git_worktree_config_cannot_hide_dirty_source_before_build() {
    let fixture = Fixture::new();
    let mirror = fixture.runner_temp.join("clean-mirror");
    fs::create_dir(&mirror).expect("create clean mirror directory");
    fs::write(mirror.join("tracked.txt"), b"clean").expect("create mirror input");
    git(
        &fixture.root,
        [
            "config",
            "core.worktree",
            mirror.to_str().expect("UTF-8 path"),
        ],
    );
    fs::write(fixture.root.join("tracked.txt"), b"dirty source").expect("modify checkout source");

    let marker = fixture.runner_temp.join("compile-must-not-run");
    let output = fixture
        .source_command(&marker)
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.worktree")
        .env("GIT_CONFIG_VALUE_0", &mirror)
        .output()
        .expect("run with hostile Git configuration");
    assert!(!output.status.success());
    assert!(
        !marker.exists(),
        "hostile Git configuration must not reach compilation"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("source checkout is not clean"));
    assert!(!fixture.github_env.exists());
}

#[test]
fn binary_copy_refuses_a_preexisting_path_without_overwriting_it() {
    let fixture = Fixture::new();
    let mock_bin = fixture.runner_temp.join("mock-bin");
    let image_dir = fixture.runner_temp.join("forced-image-dir");
    fs::create_dir(&mock_bin).expect("create command shim directory");
    fs::create_dir(&image_dir).expect("create forced image directory");
    let binary = image_dir.join("velnor-native-image-actions");
    fs::write(&binary, b"foreign content").expect("create prior binary");
    let mktemp = mock_bin.join("mktemp");
    fs::write(
        &mktemp,
        "#!/bin/bash\nif [[ \"$1\" == \"-d\" ]]; then printf '%s\\n' \"$FORCED_IMAGE_DIR\"; else exec /usr/bin/mktemp \"$@\"; fi\n",
    )
    .expect("write mktemp command shim");
    fs::set_permissions(&mktemp, fs::Permissions::from_mode(0o700))
        .expect("make command shim executable");

    let mut search_paths = vec![mock_bin];
    search_paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").expect("PATH"),
    ));
    let path = std::env::join_paths(search_paths).expect("join command search path");
    let marker = fixture.runner_temp.join("compile-ran");
    let output = fixture
        .source_command(&marker)
        .env("PATH", path)
        .env("FORCED_IMAGE_DIR", &image_dir)
        .output()
        .expect("run source script with mktemp collision");
    assert!(!output.status.success());
    assert!(marker.is_file(), "the exact-source build completed first");
    assert_eq!(
        fs::read(binary).expect("preserved binary"),
        b"foreign content"
    );
    assert!(
        !fixture.github_env.exists(),
        "no colliding path is exported"
    );
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    runner_temp: PathBuf,
    github_env: PathBuf,
    sha: String,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("create fixture parent");
        let root = temp.path().join("checkout");
        let runner_temp = temp.path().join("runner-temp");
        fs::create_dir(&root).expect("create checkout");
        fs::create_dir(&runner_temp).expect("create runner temp");
        fs::write(root.join(".gitignore"), "target/\n").expect("ignore build output");
        fs::write(root.join("tracked.txt"), b"clean").expect("create tracked input");
        git(&root, ["init", "--quiet"]);
        git(&root, ["add", ".gitignore", "tracked.txt"]);
        git(
            &root,
            [
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "source",
            ],
        );
        let sha = std::str::from_utf8(&git(&root, ["rev-parse", "HEAD"]).stdout)
            .expect("git SHA is UTF-8")
            .trim()
            .to_owned();
        let github_env = runner_temp.join("github-env");
        Self {
            _temp: temp,
            root,
            runner_temp,
            github_env,
            sha,
        }
    }

    fn run_source_script(&self, compile_marker: &Path) -> Output {
        self.source_command(compile_marker)
            .output()
            .expect("run generated native source script")
    }

    fn source_command(&self, compile_marker: &Path) -> Command {
        let script = mocked_source_script();
        let mut command = Command::new("bash");
        command
            .arg("-c")
            .arg(script)
            .current_dir(&self.root)
            .env("GITHUB_WORKSPACE", &self.root)
            .env("RUNNER_TEMP", &self.runner_temp)
            .env("GITHUB_ENV", &self.github_env)
            .env("GITHUB_SHA", &self.sha)
            .env("GITHUB_WORKFLOW_SHA", &self.sha)
            .env("COMPILE_MARKER", compile_marker);
        command
    }

    fn assert_dirty_source_stops_build(&self) {
        let marker = self.runner_temp.join("compile-must-not-run");
        let output = self.run_source_script(&marker);
        assert!(!output.status.success());
        assert!(
            !marker.exists(),
            "dirty checkout must fail before compilation"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("source checkout is not clean"));
        assert!(
            !self.github_env.exists(),
            "failed admission exports no binary path"
        );
    }

    fn binary_from_env_file(&self) -> PathBuf {
        let contents = fs::read_to_string(&self.github_env).expect("read GitHub environment file");
        let line = contents
            .lines()
            .last()
            .expect("binary variable is appended");
        PathBuf::from(
            line.strip_prefix("VELNOR_NATIVE_IMAGE_ACTIONS=")
                .expect("native binary variable"),
        )
    }
}

fn mocked_source_script() -> String {
    let pins = super::test_pins();
    let build = crate::commands::join_argv_for_run(&pins.build_argv)
        .expect("serialize pinned native build");
    let steps = super::super::native::source_steps(&pins).expect("render native source steps");
    let Yaml::Map(fields) = steps.last().expect("native build step") else {
        panic!("native build step is a map");
    };
    let Yaml::Str(script) = fields
        .iter()
        .find_map(|(key, value)| (key == "run").then_some(value))
        .expect("native build script")
    else {
        panic!("native build script is text");
    };
    assert!(script.contains(&build));
    script.replace(
        &build,
        "touch \"$COMPILE_MARKER\"; mkdir -p target/release; printf 'native image cli' > target/release/velnor-actions",
    )
}

fn git<const N: usize>(root: &Path, args: [&str; N]) -> Output {
    let output = git_fixture::command(root)
        .expect("create explicitly scoped Git fixture command")
        .args(args)
        .output()
        .expect("run git fixture command");
    assert!(
        output.status.success(),
        "git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
