//! Exact schema-2 generator-release workflow body, without the generator marker.

use velnor_actions_workflow_renderer::RenderedTree;

pub(super) const GENERATOR_RELEASE: &str = r#"name: Generator release
"on":
  workflow_dispatch: {}
permissions:
  contents: read
jobs:
  build-linux:
    name: Build Linux velnor-actions
    runs-on: ubuntu-26.04
    timeout-minutes: 120
    permissions:
      actions: write
      contents: read
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
      - name: Setup Mise
        uses: jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5
        with:
          cache: "false"
          env: "false"
          install: "false"
          version: 2026.9.18
      - name: Install pinned Rust
        run: "set -eu\nmise --no-config --no-env --no-hooks install rust@1.98.1"
      - name: Build velnor-actions
        run: "set -eu\nmise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo build --locked --release -p velnor-actions-cli\ncp target/release/velnor-actions velnor-actions-0.1.0-x86_64-unknown-linux-gnu\ntest -s velnor-actions-0.1.0-x86_64-unknown-linux-gnu"
      - name: Verify ELF architecture
        run: "set -eu\ndesc=\"$(file -b velnor-actions-0.1.0-x86_64-unknown-linux-gnu)\"\ncase \"$desc\" in\n  *ELF*x86-64*) ;;\n  *) echo \"not an x86-64 ELF: $desc\" >&2; exit 1 ;;\nesac"
      - name: Checksum built bytes
        run: "set -eu\nsha256sum velnor-actions-0.1.0-x86_64-unknown-linux-gnu > velnor-actions-0.1.0-x86_64-unknown-linux-gnu.sha256"
      - name: Upload Linux assets
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
        with:
          if-no-files-found: error
          name: generator-linux-assets
          path: "velnor-actions-0.1.0-x86_64-unknown-linux-gnu\nvelnor-actions-0.1.0-x86_64-unknown-linux-gnu.sha256"
          retention-days: 1
  attest-linux:
    name: Attest Linux velnor-actions
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    permissions:
      actions: read
      artifact-metadata: write
      attestations: write
      contents: read
      id-token: write
    needs:
      - build-linux
    steps:
      - name: Download built assets
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: generator-linux-assets
          path: assets
      - name: Attest built artifacts
        uses: actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8
        with:
          subject-path: "assets/velnor-actions-0.1.0-x86_64-unknown-linux-gnu\nassets/velnor-actions-0.1.0-x86_64-unknown-linux-gnu.sha256"
  build-macos:
    name: Build macOS velnor-actions
    runs-on: macos-15
    timeout-minutes: 120
    permissions:
      actions: write
      contents: read
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
      - name: Setup Mise
        uses: jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5
        with:
          cache: "false"
          env: "false"
          install: "false"
          version: 2026.9.18
      - name: Install pinned Rust
        run: "set -eu\nmise --no-config --no-env --no-hooks install rust@1.98.1"
      - name: Build velnor-actions
        run: "set -eu\nmise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo build --locked --release -p velnor-actions-cli\ncp target/release/velnor-actions velnor-actions-0.1.0-aarch64-apple-darwin\ntest -s velnor-actions-0.1.0-aarch64-apple-darwin"
      - name: Verify Mach-O architecture
        run: "set -eu\ndesc=\"$(file -b velnor-actions-0.1.0-aarch64-apple-darwin)\"\ncase \"$desc\" in\n  *Mach-O*arm64*) ;;\n  *) echo \"not an arm64 Mach-O: $desc\" >&2; exit 1 ;;\nesac"
      - name: Checksum built bytes
        run: "set -eu\nshasum -a 256 velnor-actions-0.1.0-aarch64-apple-darwin > velnor-actions-0.1.0-aarch64-apple-darwin.sha256"
      - name: Upload macOS assets
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
        with:
          if-no-files-found: error
          name: generator-macos-assets
          path: "velnor-actions-0.1.0-aarch64-apple-darwin\nvelnor-actions-0.1.0-aarch64-apple-darwin.sha256"
          retention-days: 1
  attest-macos:
    name: Attest macOS velnor-actions
    runs-on: macos-15
    timeout-minutes: 20
    permissions:
      actions: read
      artifact-metadata: write
      attestations: write
      contents: read
      id-token: write
    needs:
      - build-macos
    steps:
      - name: Download built assets
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: generator-macos-assets
          path: assets
      - name: Attest built artifacts
        uses: actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8
        with:
          subject-path: "assets/velnor-actions-0.1.0-aarch64-apple-darwin\nassets/velnor-actions-0.1.0-aarch64-apple-darwin.sha256"
  publish-generator:
    name: Publish velnor-actions
    runs-on: ubuntu-26.04
    timeout-minutes: 30
    permissions:
      actions: read
      contents: write
    needs:
      - attest-linux
      - attest-macos
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
      - name: Download Linux assets
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: generator-linux-assets
          path: linux-assets
      - name: Download macOS assets
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: generator-macos-assets
          path: macos-assets
      - name: Publish GitHub release
        env:
          GH_TOKEN: ${{ github.token }}
        run: "set -eu\ntag=\"generator-${GITHUB_SHA}\"\ngh release create \"$tag\" -R \"${GITHUB_REPOSITORY}\" --target \"$GITHUB_SHA\" --title \"$tag\" --latest=false --notes \"velnor-actions 0.1.0 built from ${GITHUB_SHA}.\" linux-assets/velnor-actions-0.1.0-x86_64-unknown-linux-gnu linux-assets/velnor-actions-0.1.0-x86_64-unknown-linux-gnu.sha256 macos-assets/velnor-actions-0.1.0-aarch64-apple-darwin macos-assets/velnor-actions-0.1.0-aarch64-apple-darwin.sha256"
"#;

/// Byte-lock the rendered workflow and check release invariants.
pub(super) fn assert_rendered(tree: &RenderedTree) -> Result<(), Box<dyn std::error::Error>> {
    let body = tree
        .get(".github/workflows/generator-release.yml")
        .ok_or("missing .github/workflows/generator-release.yml")?;
    assert_eq!(body, super::marked(GENERATOR_RELEASE));
    assert_generator(body)?;
    Ok(())
}

/// Committed `generator-release.yml` matches the rendered bytes.
pub(super) fn assert_committed(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = root.join(".github/workflows/generator-release.yml");
    let body = std::fs::read_to_string(&path)?;
    assert_eq!(body, super::marked(GENERATOR_RELEASE), "{}", path.display());
    assert_generator(&body)?;
    Ok(())
}

fn assert_generator(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    assert!(body.contains("name: Generator release\n"), "{body}");
    assert_eq!(
        super::job_ids(body),
        vec![
            "build-linux",
            "attest-linux",
            "build-macos",
            "attest-macos",
            "publish-generator",
        ]
    );
    assert_eq!(body.matches("contents: write").count(), 1, "{body}");
    assert_eq!(body.matches("id-token: write").count(), 2, "{body}");
    assert!(!body.contains("v0.1.0"), "{body}");
    assert!(body.contains("generator-${GITHUB_SHA}"), "{body}");
    assert!(
        body.contains("velnor-actions-0.1.0-x86_64-unknown-linux-gnu"),
        "{body}"
    );
    assert!(
        body.contains("velnor-actions-0.1.0-aarch64-apple-darwin"),
        "{body}"
    );
    assert!(!body.contains("ubuntu-26.04-scale-set"), "{body}");
    assert!(!body.contains("runs-on: [velnor"), "{body}");
    assert!(!body.contains("binary-assets"), "{body}");
    assert!(!body.contains("image-assets"), "{body}");
    assert!(body.contains("workflow_dispatch: {}"), "{body}");
    assert!(!body.contains("inputs:"), "{body}");
    assert_attest(body, "attest-linux", "ubuntu-26.04")?;
    assert_attest(body, "attest-macos", "macos-15")?;
    let linux = super::job_body(body, "build-linux")?;
    let macos = super::job_body(body, "build-macos")?;
    assert!(linux.contains("runs-on: ubuntu-26.04\n"), "{linux}");
    assert!(linux.contains("ELF"), "{linux}");
    assert!(linux.contains("x86-64"), "{linux}");
    assert!(linux.contains("sha256sum"), "{linux}");
    assert!(macos.contains("runs-on: macos-15\n"), "{macos}");
    assert!(macos.contains("*Mach-O*arm64*"), "{macos}");
    assert!(macos.contains("shasum -a 256"), "{macos}");
    assert!(!macos.contains("ubuntu"), "{macos}");
    let publish = super::job_body(body, "publish-generator")?;
    assert!(publish.contains("runs-on: ubuntu-26.04\n"), "{publish}");
    assert!(publish.contains("contents: write"), "{publish}");
    assert!(!publish.contains("id-token:"), "{publish}");
    assert!(
        publish.contains("GH_TOKEN: ${{ github.token }}"),
        "{publish}"
    );
    assert!(publish.contains("actions/checkout@"), "{publish}");
    assert!(publish.contains("- attest-linux"), "{publish}");
    assert!(publish.contains("- attest-macos"), "{publish}");
    Ok(())
}

fn assert_attest(body: &str, id: &str, runs_on: &str) -> Result<(), Box<dyn std::error::Error>> {
    let job = super::job_body(body, id)?;
    assert!(job.contains(&format!("runs-on: {runs_on}\n")), "{job}");
    assert!(job.contains("id-token: write"), "{job}");
    assert!(!job.contains("contents: write"), "{job}");
    Ok(())
}
