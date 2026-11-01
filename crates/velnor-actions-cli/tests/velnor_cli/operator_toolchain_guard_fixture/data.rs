// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const CARGO_TOML: &str =
    "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

pub(super) const CONFIG: &str = r#"schema = 2
[workflow]
name = "CI"
policy = "consumer-v1"
default_branch = "main"
generator_validation = "bootstrap"
max_parallel_jobs = 2
[execution]
default_profile = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
[stacks.rust.binary_release]
manifest_path = "Cargo.toml"
version_package = "probe"
version_env = "PROBE_VERSION_OVERRIDE"
[stacks.rust.binary_release.preview]
enabled = true
[stacks.rust.binary_release.producer]
task = "release:package"
source = { mise_config = "mise.toml", working_directory = "." }
inputs = ["Cargo.toml", "Cargo.lock", "mise.toml"]
tools = ["rust"]
timeout_minutes = 1
output_directory = "dist/release"
manifest = "dist/release/output.json"
[stacks.rust.binary_release.evidence]
profile = "signed-sbom-v1"
[[stacks.rust.binary_release.products]]
id = "probe"
package = "probe"
bins = ["probe"]
targets = ["x86_64-unknown-linux-gnu"]
archive = "tar.gz"
[distribution.homebrew_tap]
enabled = true
repository = "example/homebrew-probe"
environment = "tap-update"
app_id_variable = "TAP_APP_ID"
app_private_key_secret = "TAP_APP_PRIVATE_KEY"
merge = "squash-after-gates"
[distribution.homebrew_tap.destinations.preview]
formula = "Formula/probe.rb"
[distribution.homebrew_tap.updater]
task = "tap:update"
source = { mise_config = "mise.toml", working_directory = "." }
runner = { label = "ubuntu-26.04", platform = "linux_x64", executor = "hosted" }
inputs = ["mise.toml", "Formula/probe.rb"]
tools = ["ruby"]
timeout_minutes = 1
[distribution.homebrew_tap.protection_observer]
app_id_variable = "TAP_OBSERVER_APP_ID"
app_private_key_secret = "TAP_OBSERVER_APP_PRIVATE_KEY"
[distribution.homebrew_tap.review_completion]
check_name = "Review completion"
app_id_variable = "REVIEW_APP_ID"
[[distribution.homebrew_tap.review_completion.expected_reviewers]]
app_id_variable = "CODEX_REVIEWER_APP_ID"
"#;

pub(super) const QUALIFIED_RUST: &str = r#"[[qualified_tools]]
id = "rust"
backend = { kind = "core", tool = "rust" }
version = "1.99.1"
options = { kind = "rust", components = [], targets = [] }
depends_on = []
[[qualified_tools.platforms]]
platform = "linux_x64"
artifacts = [
  { url = "https://static.rust-lang.org/dist/2026-10-15/cargo-1.99.1-x86_64-unknown-linux-gnu.tar.xz", sha256 = "53f3521949404d595efb06a55c12c5658d4ed1c75566f33d3d514eaebe70bd8a" },
  { url = "https://static.rust-lang.org/dist/2026-10-15/rust-std-1.99.1-x86_64-unknown-linux-gnu.tar.xz", sha256 = "109b074225bf5b9ea3820890876bfdf4ed161adbfaf94b2b4b5984c85ee5d958" },
  { url = "https://static.rust-lang.org/dist/2026-10-15/rustc-1.99.1-x86_64-unknown-linux-gnu.tar.xz", sha256 = "2ff05ff9c08a90861e7f6e4065b40a8b87463b4576186fd613871c3e37f2091f" },
]
dependency_artifacts = []
install_tree_sha256 = "baa8089f830b6c49980b5c2274a0863d963d2eae8ef7f8ece56b90dfae8a8625"
[[qualified_tools.platforms.executables]]
name = "cargo"
path = "bin/cargo"
sha256 = "a670e02cfa39ac2e53f77a2d911ca1fc9e2c479b0603ec956625be1daf24e3af"
probe = { kind = "version", expected = "cargo 1.99.1 (5f94df478 2026-08-27)" }
[[qualified_tools.platforms.executables]]
name = "rustc"
path = "bin/rustc"
sha256 = "b53b780f77f952062f8ca8a3b70aef2390b1e45ad91939be94afdc9543026853"
probe = { kind = "rustc_verbose", expected = "rustc 1.99.1 (58cf8f9c3 2026-10-13)\nbinary: rustc\ncommit-hash: 58cf8f9c35b355199727627d07ae34a4e2846d67\ncommit-date: 2026-10-13\nhost: x86_64-unknown-linux-gnu\nrelease: 1.99.1\nLLVM version: 23.1.1" }
"#;
