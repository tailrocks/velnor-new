#!/usr/bin/env bash
set -eu
cd "$(git rev-parse --show-toplevel)"
messages="$(mktemp)"
trap 'rm -f "$messages"' EXIT
fixture() {
  package="$1"
  example="$2"
  variable="$3"
  cargo build --locked -p "$package" --example "$example" --message-format=json >"$messages"
  path="$(VELNOR_INTERNAL_OP=repo-policy-v1 \
    VELNOR_REPO_POLICY_ACTION=artifact-executable \
    VELNOR_REPO_POLICY_ROOT="$PWD" \
    VELNOR_REPO_POLICY_MESSAGES="$messages" \
    VELNOR_REPO_POLICY_MANIFEST="crates/$package/Cargo.toml" \
    VELNOR_REPO_POLICY_TARGET="$example" \
    VELNOR_REPO_POLICY_ARTIFACT_KIND=example \
    cargo run --quiet --locked -p velnor-actions-cli --bin velnor-actions)"
  case "$path" in
    /*) test -x "$path" ;;
    *) echo "no absolute $example artifact" >&2; exit 1 ;;
  esac
  printf '%s=%s\n' "$variable" "$path" >> "${GITHUB_ENV:?}"
}
fixture velnor-actions-cli image-publication-fixture VELNOR_IMAGE_PUBLICATION_FIXTURE
fixture velnor-actions-cli qualification-diagnostics VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE
fixture velnor-archive-guard golden_collector_clock VELNOR_QUALIFICATION_CLOCK_DRIVER
