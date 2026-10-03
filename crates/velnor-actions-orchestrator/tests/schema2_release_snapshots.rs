//! Exact schema-2 release workflow bodies, without the generator marker.

pub(super) const IMAGE_RELEASE: &str = r#"name: Image release
"on":
  workflow_dispatch: {}
permissions:
  contents: read
jobs:
  build-images:
    name: Build runner images
    runs-on: ubuntu-26.04
    timeout-minutes: 60
    permissions:
      actions: write
      contents: read
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
      - name: Build images
        run: "set -eu\ndocker build --platform linux/amd64 -t velnor-runner:linux-amd64 images/runner/ubuntu-26.04\ndocker build --platform linux/amd64 -t velnor-dind:linux-amd64 images/dind"
      - name: Verify image architecture
        run: "set -eu\nrunner=\"$(docker image inspect --format '{{.Architecture}}' velnor-runner:linux-amd64)\"\ndind=\"$(docker image inspect --format '{{.Architecture}}' velnor-dind:linux-amd64)\"\ntest \"$runner\" = amd64\ntest \"$dind\" = amd64"
      - name: Save image tars
        run: "set -eu\ndocker save --output velnor-runner-linux-amd64.tar velnor-runner:linux-amd64\ndocker save --output velnor-dind-linux-amd64.tar velnor-dind:linux-amd64\ntest -s velnor-runner-linux-amd64.tar\ntest -s velnor-dind-linux-amd64.tar"
      - name: Checksum built bytes
        run: "set -eu\nsha256sum velnor-runner-linux-amd64.tar velnor-dind-linux-amd64.tar > SHA256SUMS"
      - name: Upload image assets
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
        with:
          if-no-files-found: error
          name: image-assets
          path: "velnor-runner-linux-amd64.tar\nvelnor-dind-linux-amd64.tar\nSHA256SUMS"
          retention-days: 1
  attest-images:
    name: Attest runner images
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    permissions:
      actions: read
      artifact-metadata: write
      attestations: write
      contents: read
      id-token: write
    needs:
      - build-images
    steps:
      - name: Download built assets
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: image-assets
          path: assets
      - name: Attest built artifacts
        uses: actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8
        with:
          subject-path: "assets/velnor-runner-linux-amd64.tar\nassets/velnor-dind-linux-amd64.tar\nassets/SHA256SUMS"
  publish-images:
    name: Publish runner images
    runs-on: ubuntu-26.04
    timeout-minutes: 30
    permissions:
      actions: read
      contents: write
    needs:
      - attest-images
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
      - name: Download built assets
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: image-assets
          path: assets
      - name: Publish GitHub release
        env:
          GH_TOKEN: ${{ github.token }}
        run: "set -eu\ncd assets\ntag=\"runner-${GITHUB_SHA}\"\ngh release create \"$tag\" -R \"${GITHUB_REPOSITORY}\" --target \"$GITHUB_SHA\" --title \"$tag\" --latest=false --notes \"Runner image assets built from ${GITHUB_SHA}.\" velnor-runner-linux-amd64.tar velnor-dind-linux-amd64.tar SHA256SUMS"
"#;

pub(super) const MACOS_RELEASE: &str = r#"name: macOS binary release
"on":
  workflow_dispatch: {}
permissions:
  contents: read
jobs:
  build-binary:
    name: Build velnor-host
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
      - name: Build velnor-host
        run: "set -eu\nmise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo build --locked --manifest-path crates/velnor-runner/Cargo.toml --release -p velnor-runner-cli\ncp crates/velnor-runner/target/release/velnor-host velnor-host\ntest -s velnor-host"
      - name: Verify Mach-O architecture
        run: "set -eu\ndesc=\"$(file -b velnor-host)\"\ncase \"$desc\" in\n  *Mach-O*arm64*) ;;\n  *) echo \"not an arm64 Mach-O: $desc\" >&2; exit 1 ;;\nesac"
      - name: Checksum built bytes
        run: "set -eu\nshasum -a 256 velnor-host > SHA256SUMS"
      - name: Upload binary asset
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
        with:
          if-no-files-found: error
          name: binary-assets
          path: "velnor-host\nSHA256SUMS"
          retention-days: 1
  attest-binary:
    name: Attest velnor-host
    runs-on: macos-15
    timeout-minutes: 20
    permissions:
      actions: read
      artifact-metadata: write
      attestations: write
      contents: read
      id-token: write
    needs:
      - build-binary
    steps:
      - name: Download built assets
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: binary-assets
          path: assets
      - name: Attest built artifacts
        uses: actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8
        with:
          subject-path: "assets/velnor-host\nassets/SHA256SUMS"
  publish-binary:
    name: Publish velnor-host
    runs-on: macos-15
    timeout-minutes: 30
    permissions:
      actions: read
      contents: write
    needs:
      - attest-binary
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
      - name: Download built assets
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: binary-assets
          path: assets
      - name: Publish GitHub release
        env:
          GH_TOKEN: ${{ github.token }}
        run: "set -eu\ncd assets\ntag=\"binary-${GITHUB_SHA}\"\ngh release create \"$tag\" -R \"${GITHUB_REPOSITORY}\" --target \"$GITHUB_SHA\" --title \"$tag\" --latest=false --notes \"velnor-host built from ${GITHUB_SHA}.\" velnor-host SHA256SUMS"
"#;
