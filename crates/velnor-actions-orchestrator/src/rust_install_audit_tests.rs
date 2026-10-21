use velnor_actions_workflow_renderer::render::RenderedFile;

use super::audit;

fn file(path: &str, bytes: &str) -> Vec<RenderedFile> {
    vec![RenderedFile {
        path: path.to_owned(),
        bytes: bytes.to_owned(),
    }]
}

fn problem(files: &[RenderedFile]) -> String {
    match audit(files) {
        Ok(()) => String::new(),
        Err(error) => error.to_string(),
    }
}

const VERIFIED: &str = "jobs:
  build:
    steps:
      - name: Install pinned Rust and MBX
        run: mise --no-config --no-env --no-hooks install 'rust@'\"${VELNOR_TOOL_RUST}\" mr-boxington@1.26.0
      - name: Install x86_64-apple-darwin Rust target
        run: mise exec 'rust@'\"${VELNOR_TOOL_RUST}\" -- rustup target add x86_64-apple-darwin
      - name: Verify Rust channel manifest
        run: sh -c 'echo verified'
      - name: Build
        run: mise exec rust@1.99.1 -- cargo build
";

#[test]
fn an_install_followed_by_the_verification_passes() {
    assert_eq!(
        problem(&file(".github/workflows/release.yml", VERIFIED)),
        ""
    );
}

#[test]
fn an_install_without_the_verification_fails() {
    // The shape of every release, qualification and freshness job before
    // the check covered them: install, then build.
    let text = VERIFIED.replace(
        "      - name: Verify Rust channel manifest\n        run: sh -c 'echo verified'\n",
        "",
    );
    let found = problem(&file(".github/workflows/release.yml", &text));
    assert!(
        found.contains("rust_install_unverified:.github/workflows/release.yml:build:Build:"),
        "{found}"
    );
}

#[test]
fn an_install_that_runs_rustc_in_the_same_step_fails() {
    let text = "jobs:
  q:
    steps:
      - name: Install pinned Rust toolchain
        run: mise install rust@1.99.1 && mise exec rust@1.99.1 -- rustc --print sysroot
      - name: Verify Rust channel manifest
        run: sh -c 'echo verified'
";
    let found = problem(&file(".github/workflows/qualification.yml", text));
    assert!(found.contains("runs the toolchain it installs"), "{found}");
}

#[test]
fn composite_actions_and_step_anchors_are_audited() {
    let action = "runs:
  using: composite
  steps:
    - name: Install pinned qualification tools
      run: mise --no-config --no-env --no-hooks install 'rust@'\"${VELNOR_TOOL_RUST}\" actionlint@1.7.12
    - name: Download built asset archive
      uses: actions/download-artifact@v4
";
    let found = problem(&file(".github/actions/qualify/action.yml", action));
    assert!(
        found.contains("rust_install_unverified:.github/actions/qualify/action.yml:runs:Install pinned qualification tools:"),
        "{found}"
    );
    let anchored = "jobs:
  a:
    steps:
      - &s1
        name: Prepare pinned tools
        run: mise --no-config --no-env --no-hooks install 'rust@'\"${VELNOR_TOOL_RUST}\"
      - name: Verify Rust channel manifest
        run: sh -c 'echo verified'
  b:
    steps:
      - *s1
      - name: Build
        run: cargo build
";
    let found = problem(&file(".github/workflows/ci.yml", anchored));
    assert!(found.contains("ci.yml:b:Prepare pinned tools:"), "{found}");
}

#[test]
fn files_outside_github_yaml_are_ignored() {
    let text = VERIFIED.replace("Verify Rust channel manifest", "Something else");
    assert_eq!(problem(&file("docs/example.yml", &text)), "");
    assert_eq!(problem(&file(".github/README.md", &text)), "");
}
