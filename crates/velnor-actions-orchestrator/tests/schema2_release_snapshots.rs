//! Exact schema-2 release workflow bodies, without the generator marker.

pub(super) const IMAGE_RELEASE: &str = r#"name: Image release
"on":
  workflow_dispatch:
    inputs:
      source_sha:
        description: Exact tested main commit to build and publish
        required: true
        type: string
permissions:
  contents: read
jobs:
  release-eligibility:
    name: Check release source eligibility
    runs-on: ubuntu-26.04
    timeout-minutes: 70
    permissions:
      actions: read
      contents: read
    outputs:
      source_sha: ${{ steps.check.outputs.source_sha }}
      workflow_authority_sha: ${{ steps.check.outputs.workflow_authority_sha }}
      ci_run_id: ${{ steps.check.outputs.ci_run_id }}
      ci_attempt: ${{ steps.check.outputs.ci_attempt }}
    steps:
      - name: Check out exact event source
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
          ref: ${{ github.sha }}
      - name: Set up pinned Mise
        uses: jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5
        with:
          cache: "false"
          env: "false"
          install: "false"
          version: 2026.9.18
      - name: Install pinned GitHub CLI
        run: mise --no-config --no-env --no-hooks install gh@2.102.0
      - name: Verify main and latest Required CI
        id: check
        env:
          GH_TOKEN: ${{ github.token }}
          VELNOR_RELEASE_SOURCE_SHA: ${{ inputs.source_sha }}
        shell: bash
        run: "set -euo pipefail\n\nreadonly repository='tailrocks/velnor-new'\nreadonly workflow_path='.github/workflows/image-release.yml'\nreadonly ci_workflow_path='.github/workflows/ci.yml'\nreadonly source_sha=\"${GITHUB_SHA-}\"\nreadonly authority_sha=\"${GITHUB_WORKFLOW_SHA-}\"\nreadonly requested_sha=\"${VELNOR_RELEASE_SOURCE_SHA-}\"\nreadonly gh_version='2.102.0'\n\nfail() {\n  printf 'release eligibility: %s\\n' \"$1\" >&2\n  exit 1\n}\n\nis_sha() {\n  local candidate=\"$1\"\n  [[ \"${#candidate}\" -eq 40 && \"$candidate\" != *[!0123456789abcdef]* ]]\n}\n\n[[ \"${GITHUB_REPOSITORY-}\" == \"$repository\" ]] || fail 'unexpected repository'\n[[ \"${GITHUB_REF-}\" == 'refs/heads/main' ]] || fail 'source ref is not main'\n[[ \"${GITHUB_WORKFLOW_REF-}\" == \"$repository/$workflow_path@refs/heads/main\" ]] || fail 'unexpected workflow authority path or ref'\n[[ \"${GITHUB_EVENT_NAME-}\" == 'workflow_dispatch' ]] || fail 'event is not an eligible manual dispatch'\nis_sha \"$requested_sha\" || fail 'requested source SHA is malformed'\nis_sha \"$source_sha\" || fail 'source SHA is malformed'\nis_sha \"$authority_sha\" || fail 'workflow authority SHA is malformed'\n[[ \"$requested_sha\" == \"$source_sha\" ]] || fail 'requested source SHA and workflow source differ'\n[[ \"$source_sha\" == \"$authority_sha\" ]] || fail 'workflow authority and source differ'\ncheckout_sha=\"$(git rev-parse HEAD)\" || fail 'cannot read checked-out source SHA'\n[[ \"$checkout_sha\" == \"$source_sha\" ]] || fail 'checked-out source SHA differs'\n[[ -n \"${GH_TOKEN-}\" ]] || fail 'read-only GitHub token is missing'\n[[ -n \"${GITHUB_OUTPUT-}\" ]] || fail 'workflow output path is missing'\n\npoll_limit=\"${VELNOR_RELEASE_CI_POLL_LIMIT:-240}\"\npoll_seconds=\"${VELNOR_RELEASE_CI_POLL_SECONDS:-15}\"\n[[ \"$poll_limit\" =~ ^[1-9][0-9]*$ ]] || fail 'poll limit is invalid'\n[[ \"$poll_seconds\" =~ ^[0-9]+$ ]] || fail 'poll interval is invalid'\n\ngh_api() {\n  mise --no-config --no-env --no-hooks exec \"gh@$gh_version\" -- gh api \"$@\"\n}\n\ncurrent_main_sha() {\n  gh_api \"repos/$repository/commits/main\" | jq -er '.sha'\n}\n\nlatest_ci_run() {\n  gh_api --paginate --slurp -X GET \\\n    \"repos/$repository/actions/workflows/ci.yml/runs?head_sha=$source_sha&branch=main&event=push&per_page=100\" \\\n    | jq -c \\\n        --arg source_sha \"$source_sha\" \\\n        --arg workflow_path \"$ci_workflow_path\" \\\n        --arg repository \"$repository\" \\\n        '\n  [ .[] | (.workflow_runs // [])[] |\n    select(.path == $workflow_path\n      and .event == \"push\"\n      and .head_branch == \"main\"\n      and .head_sha == $source_sha\n      and .head_repository.full_name == $repository\n      and (.id | type) == \"number\"\n      and (.run_number | type) == \"number\"\n      and (.run_attempt | type) == \"number\")\n  ] as $runs\n  | if ($runs | length) == 0 then null\n    else\n      ($runs | sort_by([.run_number, .id])) as $ordered\n      | $ordered[-1] as $latest\n      | [$ordered[] | select(.run_number == $latest.run_number)] as $same_number\n      | if ($same_number | length) != 1\n        then error(\"ambiguous CI run number\")\n        else $latest\n        end\n    end\n'\n}\n\nassert_current_tip() {\n  local current_sha\n  current_sha=\"$(current_main_sha)\"\n  [[ \"$current_sha\" == \"$source_sha\" ]] || fail 'source is no longer the main tip'\n}\n\nassert_required_job() {\n  local run_id=\"$1\"\n  local run_attempt=\"$2\"\n  local jobs required\n  jobs=\"$(gh_api --paginate --slurp -X GET \\\n    \"repos/$repository/actions/runs/$run_id/attempts/$run_attempt/jobs?per_page=100\")\"\n  required=\"$(printf '%s\\n' \"$jobs\" | jq -c \\\n    --argjson run_id \"$run_id\" \\\n    --argjson run_attempt \"$run_attempt\" \\\n    --arg source_sha \"$source_sha\" \\\n    '\n  [ .[].jobs[]? | select(.name == \"Required\") ] as $required\n  | if ($required | length) != 1 then error(\"expected one Required job\")\n    else $required[0]\n      | if .run_id == $run_id\n          and .run_attempt == $run_attempt\n          and .head_sha == $source_sha\n          and .head_branch == \"main\"\n        then .\n        else error(\"Required job identity mismatch\")\n        end\n    end\n')\"\n  [[ \"$(jq -r '.status' <<<\"$required\")\" == 'completed' ]] || fail 'Required job is not complete'\n  [[ \"$(jq -r '.conclusion' <<<\"$required\")\" == 'success' ]] || fail 'Required job did not succeed'\n  [[ \"$(jq -r '.run_id' <<<\"$required\")\" == \"$run_id\" ]] || fail 'Required job has a different run ID'\n  [[ \"$(jq -r '.head_sha' <<<\"$required\")\" == \"$source_sha\" ]] || fail 'Required job has a different source SHA'\n  [[ \"$(jq -r '.head_branch' <<<\"$required\")\" == 'main' ]] || fail 'Required job is not on main'\n}\n\nattempt=0\nwhile [[ \"$attempt\" -lt \"$poll_limit\" ]]; do\n  assert_current_tip\n  run=\"$(latest_ci_run)\"\n  if [[ \"$run\" != 'null' ]]; then\n    status=\"$(jq -r '.status' <<<\"$run\")\"\n    case \"$status\" in\n      completed)\n        [[ \"$(jq -r '.conclusion' <<<\"$run\")\" == 'success' ]] || fail 'latest exact-source CI run did not succeed'\n        run_id=\"$(jq -r '.id' <<<\"$run\")\"\n        run_attempt=\"$(jq -r '.run_attempt' <<<\"$run\")\"\n        assert_required_job \"$run_id\" \"$run_attempt\"\n        latest_again=\"$(latest_ci_run)\"\n        [[ \"$(jq -r '.id' <<<\"$latest_again\")\" == \"$run_id\" ]] || fail 'latest CI run changed during eligibility check'\n        [[ \"$(jq -r '.run_attempt' <<<\"$latest_again\")\" == \"$run_attempt\" ]] || fail 'latest CI attempt changed during eligibility check'\n        [[ \"$(jq -r '.status' <<<\"$latest_again\")\" == 'completed' ]] || fail 'latest CI run restarted during eligibility check'\n        [[ \"$(jq -r '.conclusion' <<<\"$latest_again\")\" == 'success' ]] || fail 'latest CI run changed during eligibility check'\n        assert_current_tip\n        printf 'source_sha=%s\\n' \"$source_sha\" >> \"$GITHUB_OUTPUT\"\n        printf 'workflow_authority_sha=%s\\n' \"$authority_sha\" >> \"$GITHUB_OUTPUT\"\n        printf 'ci_run_id=%s\\n' \"$run_id\" >> \"$GITHUB_OUTPUT\"\n        printf 'ci_attempt=%s\\n' \"$run_attempt\" >> \"$GITHUB_OUTPUT\"\n        exit 0\n        ;;\n      queued|in_progress|pending|waiting|requested) ;;\n      *) fail 'latest exact-source CI run has an unknown status' ;;\n    esac\n  fi\n  attempt=$((attempt + 1))\n  if [[ \"$attempt\" -lt \"$poll_limit\" ]]; then\n    sleep \"$poll_seconds\"\n  fi\ndone\nfail 'latest exact-source CI run did not become successful before timeout'\n"
  build-images:
    name: Build runner images
    runs-on: ubuntu-26.04
    timeout-minutes: 60
    permissions:
      actions: write
      contents: read
    needs:
      - release-eligibility
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
          ref: ${{ needs['release-eligibility'].outputs.source_sha }}
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
      - release-eligibility
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
          ref: ${{ needs['release-eligibility'].outputs.source_sha }}
      - name: Set up pinned Mise
        uses: jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5
        with:
          cache: "false"
          env: "false"
          install: "false"
          version: 2026.9.18
      - name: Install pinned GitHub CLI
        run: mise --no-config --no-env --no-hooks install gh@2.102.0
      - name: Download built assets
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: image-assets
          path: assets
      - name: Verify main and latest Required CI
        id: check
        env:
          GH_TOKEN: ${{ github.token }}
          VELNOR_RELEASE_SOURCE_SHA: ${{ inputs.source_sha }}
        shell: bash
        run: "set -euo pipefail\n\nreadonly repository='tailrocks/velnor-new'\nreadonly workflow_path='.github/workflows/image-release.yml'\nreadonly ci_workflow_path='.github/workflows/ci.yml'\nreadonly source_sha=\"${GITHUB_SHA-}\"\nreadonly authority_sha=\"${GITHUB_WORKFLOW_SHA-}\"\nreadonly requested_sha=\"${VELNOR_RELEASE_SOURCE_SHA-}\"\nreadonly gh_version='2.102.0'\n\nfail() {\n  printf 'release eligibility: %s\\n' \"$1\" >&2\n  exit 1\n}\n\nis_sha() {\n  local candidate=\"$1\"\n  [[ \"${#candidate}\" -eq 40 && \"$candidate\" != *[!0123456789abcdef]* ]]\n}\n\n[[ \"${GITHUB_REPOSITORY-}\" == \"$repository\" ]] || fail 'unexpected repository'\n[[ \"${GITHUB_REF-}\" == 'refs/heads/main' ]] || fail 'source ref is not main'\n[[ \"${GITHUB_WORKFLOW_REF-}\" == \"$repository/$workflow_path@refs/heads/main\" ]] || fail 'unexpected workflow authority path or ref'\n[[ \"${GITHUB_EVENT_NAME-}\" == 'workflow_dispatch' ]] || fail 'event is not an eligible manual dispatch'\nis_sha \"$requested_sha\" || fail 'requested source SHA is malformed'\nis_sha \"$source_sha\" || fail 'source SHA is malformed'\nis_sha \"$authority_sha\" || fail 'workflow authority SHA is malformed'\n[[ \"$requested_sha\" == \"$source_sha\" ]] || fail 'requested source SHA and workflow source differ'\n[[ \"$source_sha\" == \"$authority_sha\" ]] || fail 'workflow authority and source differ'\ncheckout_sha=\"$(git rev-parse HEAD)\" || fail 'cannot read checked-out source SHA'\n[[ \"$checkout_sha\" == \"$source_sha\" ]] || fail 'checked-out source SHA differs'\n[[ -n \"${GH_TOKEN-}\" ]] || fail 'read-only GitHub token is missing'\n[[ -n \"${GITHUB_OUTPUT-}\" ]] || fail 'workflow output path is missing'\n\npoll_limit=\"${VELNOR_RELEASE_CI_POLL_LIMIT:-240}\"\npoll_seconds=\"${VELNOR_RELEASE_CI_POLL_SECONDS:-15}\"\n[[ \"$poll_limit\" =~ ^[1-9][0-9]*$ ]] || fail 'poll limit is invalid'\n[[ \"$poll_seconds\" =~ ^[0-9]+$ ]] || fail 'poll interval is invalid'\n\ngh_api() {\n  mise --no-config --no-env --no-hooks exec \"gh@$gh_version\" -- gh api \"$@\"\n}\n\ncurrent_main_sha() {\n  gh_api \"repos/$repository/commits/main\" | jq -er '.sha'\n}\n\nlatest_ci_run() {\n  gh_api --paginate --slurp -X GET \\\n    \"repos/$repository/actions/workflows/ci.yml/runs?head_sha=$source_sha&branch=main&event=push&per_page=100\" \\\n    | jq -c \\\n        --arg source_sha \"$source_sha\" \\\n        --arg workflow_path \"$ci_workflow_path\" \\\n        --arg repository \"$repository\" \\\n        '\n  [ .[] | (.workflow_runs // [])[] |\n    select(.path == $workflow_path\n      and .event == \"push\"\n      and .head_branch == \"main\"\n      and .head_sha == $source_sha\n      and .head_repository.full_name == $repository\n      and (.id | type) == \"number\"\n      and (.run_number | type) == \"number\"\n      and (.run_attempt | type) == \"number\")\n  ] as $runs\n  | if ($runs | length) == 0 then null\n    else\n      ($runs | sort_by([.run_number, .id])) as $ordered\n      | $ordered[-1] as $latest\n      | [$ordered[] | select(.run_number == $latest.run_number)] as $same_number\n      | if ($same_number | length) != 1\n        then error(\"ambiguous CI run number\")\n        else $latest\n        end\n    end\n'\n}\n\nassert_current_tip() {\n  local current_sha\n  current_sha=\"$(current_main_sha)\"\n  [[ \"$current_sha\" == \"$source_sha\" ]] || fail 'source is no longer the main tip'\n}\n\nassert_required_job() {\n  local run_id=\"$1\"\n  local run_attempt=\"$2\"\n  local jobs required\n  jobs=\"$(gh_api --paginate --slurp -X GET \\\n    \"repos/$repository/actions/runs/$run_id/attempts/$run_attempt/jobs?per_page=100\")\"\n  required=\"$(printf '%s\\n' \"$jobs\" | jq -c \\\n    --argjson run_id \"$run_id\" \\\n    --argjson run_attempt \"$run_attempt\" \\\n    --arg source_sha \"$source_sha\" \\\n    '\n  [ .[].jobs[]? | select(.name == \"Required\") ] as $required\n  | if ($required | length) != 1 then error(\"expected one Required job\")\n    else $required[0]\n      | if .run_id == $run_id\n          and .run_attempt == $run_attempt\n          and .head_sha == $source_sha\n          and .head_branch == \"main\"\n        then .\n        else error(\"Required job identity mismatch\")\n        end\n    end\n')\"\n  [[ \"$(jq -r '.status' <<<\"$required\")\" == 'completed' ]] || fail 'Required job is not complete'\n  [[ \"$(jq -r '.conclusion' <<<\"$required\")\" == 'success' ]] || fail 'Required job did not succeed'\n  [[ \"$(jq -r '.run_id' <<<\"$required\")\" == \"$run_id\" ]] || fail 'Required job has a different run ID'\n  [[ \"$(jq -r '.head_sha' <<<\"$required\")\" == \"$source_sha\" ]] || fail 'Required job has a different source SHA'\n  [[ \"$(jq -r '.head_branch' <<<\"$required\")\" == 'main' ]] || fail 'Required job is not on main'\n}\n\nattempt=0\nwhile [[ \"$attempt\" -lt \"$poll_limit\" ]]; do\n  assert_current_tip\n  run=\"$(latest_ci_run)\"\n  if [[ \"$run\" != 'null' ]]; then\n    status=\"$(jq -r '.status' <<<\"$run\")\"\n    case \"$status\" in\n      completed)\n        [[ \"$(jq -r '.conclusion' <<<\"$run\")\" == 'success' ]] || fail 'latest exact-source CI run did not succeed'\n        run_id=\"$(jq -r '.id' <<<\"$run\")\"\n        run_attempt=\"$(jq -r '.run_attempt' <<<\"$run\")\"\n        assert_required_job \"$run_id\" \"$run_attempt\"\n        latest_again=\"$(latest_ci_run)\"\n        [[ \"$(jq -r '.id' <<<\"$latest_again\")\" == \"$run_id\" ]] || fail 'latest CI run changed during eligibility check'\n        [[ \"$(jq -r '.run_attempt' <<<\"$latest_again\")\" == \"$run_attempt\" ]] || fail 'latest CI attempt changed during eligibility check'\n        [[ \"$(jq -r '.status' <<<\"$latest_again\")\" == 'completed' ]] || fail 'latest CI run restarted during eligibility check'\n        [[ \"$(jq -r '.conclusion' <<<\"$latest_again\")\" == 'success' ]] || fail 'latest CI run changed during eligibility check'\n        assert_current_tip\n        printf 'source_sha=%s\\n' \"$source_sha\" >> \"$GITHUB_OUTPUT\"\n        printf 'workflow_authority_sha=%s\\n' \"$authority_sha\" >> \"$GITHUB_OUTPUT\"\n        printf 'ci_run_id=%s\\n' \"$run_id\" >> \"$GITHUB_OUTPUT\"\n        printf 'ci_attempt=%s\\n' \"$run_attempt\" >> \"$GITHUB_OUTPUT\"\n        exit 0\n        ;;\n      queued|in_progress|pending|waiting|requested) ;;\n      *) fail 'latest exact-source CI run has an unknown status' ;;\n    esac\n  fi\n  attempt=$((attempt + 1))\n  if [[ \"$attempt\" -lt \"$poll_limit\" ]]; then\n    sleep \"$poll_seconds\"\n  fi\ndone\nfail 'latest exact-source CI run did not become successful before timeout'\n"
      - name: Publish GitHub release
        env:
          GH_TOKEN: ${{ github.token }}
        run: "set -eu\ncd assets\ntag=\"runner-${GITHUB_SHA}\"\ngh release create \"$tag\" -R \"${GITHUB_REPOSITORY}\" --target \"$GITHUB_SHA\" --title \"$tag\" --latest=false --notes \"Runner image assets built from ${GITHUB_SHA}.\" velnor-runner-linux-amd64.tar velnor-dind-linux-amd64.tar SHA256SUMS"
"#;

pub(super) const MACOS_RELEASE: &str = r#"name: macOS binary release
"on":
  workflow_dispatch:
    inputs:
      source_sha:
        description: Exact tested main commit to build and publish
        required: true
        type: string
permissions:
  contents: read
jobs:
  release-eligibility:
    name: Check release source eligibility
    runs-on: macos-15
    timeout-minutes: 70
    permissions:
      actions: read
      contents: read
    outputs:
      source_sha: ${{ steps.check.outputs.source_sha }}
      workflow_authority_sha: ${{ steps.check.outputs.workflow_authority_sha }}
      ci_run_id: ${{ steps.check.outputs.ci_run_id }}
      ci_attempt: ${{ steps.check.outputs.ci_attempt }}
    steps:
      - name: Check out exact event source
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
          ref: ${{ github.sha }}
      - name: Set up pinned Mise
        uses: jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5
        with:
          cache: "false"
          env: "false"
          install: "false"
          version: 2026.9.18
      - name: Install pinned GitHub CLI
        run: mise --no-config --no-env --no-hooks install gh@2.102.0
      - name: Verify main and latest Required CI
        id: check
        env:
          GH_TOKEN: ${{ github.token }}
          VELNOR_RELEASE_SOURCE_SHA: ${{ inputs.source_sha }}
        shell: bash
        run: "set -euo pipefail\n\nreadonly repository='tailrocks/velnor-new'\nreadonly workflow_path='.github/workflows/macos-binary-release.yml'\nreadonly ci_workflow_path='.github/workflows/ci.yml'\nreadonly source_sha=\"${GITHUB_SHA-}\"\nreadonly authority_sha=\"${GITHUB_WORKFLOW_SHA-}\"\nreadonly requested_sha=\"${VELNOR_RELEASE_SOURCE_SHA-}\"\nreadonly gh_version='2.102.0'\n\nfail() {\n  printf 'release eligibility: %s\\n' \"$1\" >&2\n  exit 1\n}\n\nis_sha() {\n  local candidate=\"$1\"\n  [[ \"${#candidate}\" -eq 40 && \"$candidate\" != *[!0123456789abcdef]* ]]\n}\n\n[[ \"${GITHUB_REPOSITORY-}\" == \"$repository\" ]] || fail 'unexpected repository'\n[[ \"${GITHUB_REF-}\" == 'refs/heads/main' ]] || fail 'source ref is not main'\n[[ \"${GITHUB_WORKFLOW_REF-}\" == \"$repository/$workflow_path@refs/heads/main\" ]] || fail 'unexpected workflow authority path or ref'\n[[ \"${GITHUB_EVENT_NAME-}\" == 'workflow_dispatch' ]] || fail 'event is not an eligible manual dispatch'\nis_sha \"$requested_sha\" || fail 'requested source SHA is malformed'\nis_sha \"$source_sha\" || fail 'source SHA is malformed'\nis_sha \"$authority_sha\" || fail 'workflow authority SHA is malformed'\n[[ \"$requested_sha\" == \"$source_sha\" ]] || fail 'requested source SHA and workflow source differ'\n[[ \"$source_sha\" == \"$authority_sha\" ]] || fail 'workflow authority and source differ'\ncheckout_sha=\"$(git rev-parse HEAD)\" || fail 'cannot read checked-out source SHA'\n[[ \"$checkout_sha\" == \"$source_sha\" ]] || fail 'checked-out source SHA differs'\n[[ -n \"${GH_TOKEN-}\" ]] || fail 'read-only GitHub token is missing'\n[[ -n \"${GITHUB_OUTPUT-}\" ]] || fail 'workflow output path is missing'\n\npoll_limit=\"${VELNOR_RELEASE_CI_POLL_LIMIT:-240}\"\npoll_seconds=\"${VELNOR_RELEASE_CI_POLL_SECONDS:-15}\"\n[[ \"$poll_limit\" =~ ^[1-9][0-9]*$ ]] || fail 'poll limit is invalid'\n[[ \"$poll_seconds\" =~ ^[0-9]+$ ]] || fail 'poll interval is invalid'\n\ngh_api() {\n  mise --no-config --no-env --no-hooks exec \"gh@$gh_version\" -- gh api \"$@\"\n}\n\ncurrent_main_sha() {\n  gh_api \"repos/$repository/commits/main\" | jq -er '.sha'\n}\n\nlatest_ci_run() {\n  gh_api --paginate --slurp -X GET \\\n    \"repos/$repository/actions/workflows/ci.yml/runs?head_sha=$source_sha&branch=main&event=push&per_page=100\" \\\n    | jq -c \\\n        --arg source_sha \"$source_sha\" \\\n        --arg workflow_path \"$ci_workflow_path\" \\\n        --arg repository \"$repository\" \\\n        '\n  [ .[] | (.workflow_runs // [])[] |\n    select(.path == $workflow_path\n      and .event == \"push\"\n      and .head_branch == \"main\"\n      and .head_sha == $source_sha\n      and .head_repository.full_name == $repository\n      and (.id | type) == \"number\"\n      and (.run_number | type) == \"number\"\n      and (.run_attempt | type) == \"number\")\n  ] as $runs\n  | if ($runs | length) == 0 then null\n    else\n      ($runs | sort_by([.run_number, .id])) as $ordered\n      | $ordered[-1] as $latest\n      | [$ordered[] | select(.run_number == $latest.run_number)] as $same_number\n      | if ($same_number | length) != 1\n        then error(\"ambiguous CI run number\")\n        else $latest\n        end\n    end\n'\n}\n\nassert_current_tip() {\n  local current_sha\n  current_sha=\"$(current_main_sha)\"\n  [[ \"$current_sha\" == \"$source_sha\" ]] || fail 'source is no longer the main tip'\n}\n\nassert_required_job() {\n  local run_id=\"$1\"\n  local run_attempt=\"$2\"\n  local jobs required\n  jobs=\"$(gh_api --paginate --slurp -X GET \\\n    \"repos/$repository/actions/runs/$run_id/attempts/$run_attempt/jobs?per_page=100\")\"\n  required=\"$(printf '%s\\n' \"$jobs\" | jq -c \\\n    --argjson run_id \"$run_id\" \\\n    --argjson run_attempt \"$run_attempt\" \\\n    --arg source_sha \"$source_sha\" \\\n    '\n  [ .[].jobs[]? | select(.name == \"Required\") ] as $required\n  | if ($required | length) != 1 then error(\"expected one Required job\")\n    else $required[0]\n      | if .run_id == $run_id\n          and .run_attempt == $run_attempt\n          and .head_sha == $source_sha\n          and .head_branch == \"main\"\n        then .\n        else error(\"Required job identity mismatch\")\n        end\n    end\n')\"\n  [[ \"$(jq -r '.status' <<<\"$required\")\" == 'completed' ]] || fail 'Required job is not complete'\n  [[ \"$(jq -r '.conclusion' <<<\"$required\")\" == 'success' ]] || fail 'Required job did not succeed'\n  [[ \"$(jq -r '.run_id' <<<\"$required\")\" == \"$run_id\" ]] || fail 'Required job has a different run ID'\n  [[ \"$(jq -r '.head_sha' <<<\"$required\")\" == \"$source_sha\" ]] || fail 'Required job has a different source SHA'\n  [[ \"$(jq -r '.head_branch' <<<\"$required\")\" == 'main' ]] || fail 'Required job is not on main'\n}\n\nattempt=0\nwhile [[ \"$attempt\" -lt \"$poll_limit\" ]]; do\n  assert_current_tip\n  run=\"$(latest_ci_run)\"\n  if [[ \"$run\" != 'null' ]]; then\n    status=\"$(jq -r '.status' <<<\"$run\")\"\n    case \"$status\" in\n      completed)\n        [[ \"$(jq -r '.conclusion' <<<\"$run\")\" == 'success' ]] || fail 'latest exact-source CI run did not succeed'\n        run_id=\"$(jq -r '.id' <<<\"$run\")\"\n        run_attempt=\"$(jq -r '.run_attempt' <<<\"$run\")\"\n        assert_required_job \"$run_id\" \"$run_attempt\"\n        latest_again=\"$(latest_ci_run)\"\n        [[ \"$(jq -r '.id' <<<\"$latest_again\")\" == \"$run_id\" ]] || fail 'latest CI run changed during eligibility check'\n        [[ \"$(jq -r '.run_attempt' <<<\"$latest_again\")\" == \"$run_attempt\" ]] || fail 'latest CI attempt changed during eligibility check'\n        [[ \"$(jq -r '.status' <<<\"$latest_again\")\" == 'completed' ]] || fail 'latest CI run restarted during eligibility check'\n        [[ \"$(jq -r '.conclusion' <<<\"$latest_again\")\" == 'success' ]] || fail 'latest CI run changed during eligibility check'\n        assert_current_tip\n        printf 'source_sha=%s\\n' \"$source_sha\" >> \"$GITHUB_OUTPUT\"\n        printf 'workflow_authority_sha=%s\\n' \"$authority_sha\" >> \"$GITHUB_OUTPUT\"\n        printf 'ci_run_id=%s\\n' \"$run_id\" >> \"$GITHUB_OUTPUT\"\n        printf 'ci_attempt=%s\\n' \"$run_attempt\" >> \"$GITHUB_OUTPUT\"\n        exit 0\n        ;;\n      queued|in_progress|pending|waiting|requested) ;;\n      *) fail 'latest exact-source CI run has an unknown status' ;;\n    esac\n  fi\n  attempt=$((attempt + 1))\n  if [[ \"$attempt\" -lt \"$poll_limit\" ]]; then\n    sleep \"$poll_seconds\"\n  fi\ndone\nfail 'latest exact-source CI run did not become successful before timeout'\n"
  build-binary:
    name: Build velnor-host
    runs-on: macos-15
    timeout-minutes: 120
    permissions:
      actions: write
      contents: read
    needs:
      - release-eligibility
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
          ref: ${{ needs['release-eligibility'].outputs.source_sha }}
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
      - release-eligibility
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: "1"
          persist-credentials: "false"
          ref: ${{ needs['release-eligibility'].outputs.source_sha }}
      - name: Set up pinned Mise
        uses: jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5
        with:
          cache: "false"
          env: "false"
          install: "false"
          version: 2026.9.18
      - name: Install pinned GitHub CLI
        run: mise --no-config --no-env --no-hooks install gh@2.102.0
      - name: Download built assets
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: binary-assets
          path: assets
      - name: Verify main and latest Required CI
        id: check
        env:
          GH_TOKEN: ${{ github.token }}
          VELNOR_RELEASE_SOURCE_SHA: ${{ inputs.source_sha }}
        shell: bash
        run: "set -euo pipefail\n\nreadonly repository='tailrocks/velnor-new'\nreadonly workflow_path='.github/workflows/macos-binary-release.yml'\nreadonly ci_workflow_path='.github/workflows/ci.yml'\nreadonly source_sha=\"${GITHUB_SHA-}\"\nreadonly authority_sha=\"${GITHUB_WORKFLOW_SHA-}\"\nreadonly requested_sha=\"${VELNOR_RELEASE_SOURCE_SHA-}\"\nreadonly gh_version='2.102.0'\n\nfail() {\n  printf 'release eligibility: %s\\n' \"$1\" >&2\n  exit 1\n}\n\nis_sha() {\n  local candidate=\"$1\"\n  [[ \"${#candidate}\" -eq 40 && \"$candidate\" != *[!0123456789abcdef]* ]]\n}\n\n[[ \"${GITHUB_REPOSITORY-}\" == \"$repository\" ]] || fail 'unexpected repository'\n[[ \"${GITHUB_REF-}\" == 'refs/heads/main' ]] || fail 'source ref is not main'\n[[ \"${GITHUB_WORKFLOW_REF-}\" == \"$repository/$workflow_path@refs/heads/main\" ]] || fail 'unexpected workflow authority path or ref'\n[[ \"${GITHUB_EVENT_NAME-}\" == 'workflow_dispatch' ]] || fail 'event is not an eligible manual dispatch'\nis_sha \"$requested_sha\" || fail 'requested source SHA is malformed'\nis_sha \"$source_sha\" || fail 'source SHA is malformed'\nis_sha \"$authority_sha\" || fail 'workflow authority SHA is malformed'\n[[ \"$requested_sha\" == \"$source_sha\" ]] || fail 'requested source SHA and workflow source differ'\n[[ \"$source_sha\" == \"$authority_sha\" ]] || fail 'workflow authority and source differ'\ncheckout_sha=\"$(git rev-parse HEAD)\" || fail 'cannot read checked-out source SHA'\n[[ \"$checkout_sha\" == \"$source_sha\" ]] || fail 'checked-out source SHA differs'\n[[ -n \"${GH_TOKEN-}\" ]] || fail 'read-only GitHub token is missing'\n[[ -n \"${GITHUB_OUTPUT-}\" ]] || fail 'workflow output path is missing'\n\npoll_limit=\"${VELNOR_RELEASE_CI_POLL_LIMIT:-240}\"\npoll_seconds=\"${VELNOR_RELEASE_CI_POLL_SECONDS:-15}\"\n[[ \"$poll_limit\" =~ ^[1-9][0-9]*$ ]] || fail 'poll limit is invalid'\n[[ \"$poll_seconds\" =~ ^[0-9]+$ ]] || fail 'poll interval is invalid'\n\ngh_api() {\n  mise --no-config --no-env --no-hooks exec \"gh@$gh_version\" -- gh api \"$@\"\n}\n\ncurrent_main_sha() {\n  gh_api \"repos/$repository/commits/main\" | jq -er '.sha'\n}\n\nlatest_ci_run() {\n  gh_api --paginate --slurp -X GET \\\n    \"repos/$repository/actions/workflows/ci.yml/runs?head_sha=$source_sha&branch=main&event=push&per_page=100\" \\\n    | jq -c \\\n        --arg source_sha \"$source_sha\" \\\n        --arg workflow_path \"$ci_workflow_path\" \\\n        --arg repository \"$repository\" \\\n        '\n  [ .[] | (.workflow_runs // [])[] |\n    select(.path == $workflow_path\n      and .event == \"push\"\n      and .head_branch == \"main\"\n      and .head_sha == $source_sha\n      and .head_repository.full_name == $repository\n      and (.id | type) == \"number\"\n      and (.run_number | type) == \"number\"\n      and (.run_attempt | type) == \"number\")\n  ] as $runs\n  | if ($runs | length) == 0 then null\n    else\n      ($runs | sort_by([.run_number, .id])) as $ordered\n      | $ordered[-1] as $latest\n      | [$ordered[] | select(.run_number == $latest.run_number)] as $same_number\n      | if ($same_number | length) != 1\n        then error(\"ambiguous CI run number\")\n        else $latest\n        end\n    end\n'\n}\n\nassert_current_tip() {\n  local current_sha\n  current_sha=\"$(current_main_sha)\"\n  [[ \"$current_sha\" == \"$source_sha\" ]] || fail 'source is no longer the main tip'\n}\n\nassert_required_job() {\n  local run_id=\"$1\"\n  local run_attempt=\"$2\"\n  local jobs required\n  jobs=\"$(gh_api --paginate --slurp -X GET \\\n    \"repos/$repository/actions/runs/$run_id/attempts/$run_attempt/jobs?per_page=100\")\"\n  required=\"$(printf '%s\\n' \"$jobs\" | jq -c \\\n    --argjson run_id \"$run_id\" \\\n    --argjson run_attempt \"$run_attempt\" \\\n    --arg source_sha \"$source_sha\" \\\n    '\n  [ .[].jobs[]? | select(.name == \"Required\") ] as $required\n  | if ($required | length) != 1 then error(\"expected one Required job\")\n    else $required[0]\n      | if .run_id == $run_id\n          and .run_attempt == $run_attempt\n          and .head_sha == $source_sha\n          and .head_branch == \"main\"\n        then .\n        else error(\"Required job identity mismatch\")\n        end\n    end\n')\"\n  [[ \"$(jq -r '.status' <<<\"$required\")\" == 'completed' ]] || fail 'Required job is not complete'\n  [[ \"$(jq -r '.conclusion' <<<\"$required\")\" == 'success' ]] || fail 'Required job did not succeed'\n  [[ \"$(jq -r '.run_id' <<<\"$required\")\" == \"$run_id\" ]] || fail 'Required job has a different run ID'\n  [[ \"$(jq -r '.head_sha' <<<\"$required\")\" == \"$source_sha\" ]] || fail 'Required job has a different source SHA'\n  [[ \"$(jq -r '.head_branch' <<<\"$required\")\" == 'main' ]] || fail 'Required job is not on main'\n}\n\nattempt=0\nwhile [[ \"$attempt\" -lt \"$poll_limit\" ]]; do\n  assert_current_tip\n  run=\"$(latest_ci_run)\"\n  if [[ \"$run\" != 'null' ]]; then\n    status=\"$(jq -r '.status' <<<\"$run\")\"\n    case \"$status\" in\n      completed)\n        [[ \"$(jq -r '.conclusion' <<<\"$run\")\" == 'success' ]] || fail 'latest exact-source CI run did not succeed'\n        run_id=\"$(jq -r '.id' <<<\"$run\")\"\n        run_attempt=\"$(jq -r '.run_attempt' <<<\"$run\")\"\n        assert_required_job \"$run_id\" \"$run_attempt\"\n        latest_again=\"$(latest_ci_run)\"\n        [[ \"$(jq -r '.id' <<<\"$latest_again\")\" == \"$run_id\" ]] || fail 'latest CI run changed during eligibility check'\n        [[ \"$(jq -r '.run_attempt' <<<\"$latest_again\")\" == \"$run_attempt\" ]] || fail 'latest CI attempt changed during eligibility check'\n        [[ \"$(jq -r '.status' <<<\"$latest_again\")\" == 'completed' ]] || fail 'latest CI run restarted during eligibility check'\n        [[ \"$(jq -r '.conclusion' <<<\"$latest_again\")\" == 'success' ]] || fail 'latest CI run changed during eligibility check'\n        assert_current_tip\n        printf 'source_sha=%s\\n' \"$source_sha\" >> \"$GITHUB_OUTPUT\"\n        printf 'workflow_authority_sha=%s\\n' \"$authority_sha\" >> \"$GITHUB_OUTPUT\"\n        printf 'ci_run_id=%s\\n' \"$run_id\" >> \"$GITHUB_OUTPUT\"\n        printf 'ci_attempt=%s\\n' \"$run_attempt\" >> \"$GITHUB_OUTPUT\"\n        exit 0\n        ;;\n      queued|in_progress|pending|waiting|requested) ;;\n      *) fail 'latest exact-source CI run has an unknown status' ;;\n    esac\n  fi\n  attempt=$((attempt + 1))\n  if [[ \"$attempt\" -lt \"$poll_limit\" ]]; then\n    sleep \"$poll_seconds\"\n  fi\ndone\nfail 'latest exact-source CI run did not become successful before timeout'\n"
      - name: Publish GitHub release
        env:
          GH_TOKEN: ${{ github.token }}
        run: "set -eu\ncd assets\ntag=\"binary-${GITHUB_SHA}\"\ngh release create \"$tag\" -R \"${GITHUB_REPOSITORY}\" --target \"$GITHUB_SHA\" --title \"$tag\" --latest=false --notes \"velnor-host built from ${GITHUB_SHA}.\" velnor-host SHA256SUMS"
"#;
