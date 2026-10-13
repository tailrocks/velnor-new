/// `qualification.yml` body, without the generator marker.
pub(super) const QUALIFICATION: &str = concat!(
    r#"name: Qualification
"on":
  workflow_dispatch:
    inputs:
      mode:
        type: string
        required: false
        default: both
permissions:
  contents: read
jobs:
  verify-hosted:
    name: Verify / GitHub hosted / Linux x64
    if: inputs.mode == 'both'
    runs-on: ubuntu-26.04
    timeout-minutes: 30
    steps:
      - name: Qualify hosted lane
        run: echo qualification-hosted
  verify-scale-set:
    name: Verify / Velnor Scale Set / Linux x64
    if: inputs.mode == 'both'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 30
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Qualify scale-set lane
        run: echo qualification-scale-set
  compare:
    name: Compare hosted and Velnor execution
    if: inputs.mode == 'both'
    runs-on: ubuntu-26.04
    timeout-minutes: 10
    needs:
      - verify-hosted
      - verify-scale-set
    steps:
      - name: Compare lanes
        run: echo compare-lanes
  js-hosted:
    name: JavaScript actions / GitHub hosted
    if: inputs.mode == 'features' || inputs.mode == 'js'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Record checkout
        run: git rev-parse HEAD
      - name: Run JavaScript
        run: "node -e 'console.log(\"js-action-ok\")'"
  js-scale-set:
    name: JavaScript actions / Velnor Scale Set
    if: inputs.mode == 'features' || inputs.mode == 'js'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Record checkout
        run: git rev-parse HEAD
      - name: Run JavaScript
        run: "node -e 'console.log(\"js-action-ok\")'"
  services-hosted:
    name: Services / GitHub hosted
    if: inputs.mode == 'features' || inputs.mode == 'services'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    services:
      redis:
        image: redis:7-alpine
        options: "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12"
        ports:
          - "6379:6379"
    steps:
      - name: Localhost port
        run: timeout 20 bash -c 'until echo >/dev/tcp/127.0.0.1/6379; do sleep 1; done'
  services-scale-set:
    name: Services / Velnor Scale Set
    if: inputs.mode == 'features' || inputs.mode == 'services'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    services:
      redis:
        image: redis:7-alpine
        options: "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12"
        ports:
          - "6379:6379"
    steps:
      - name: Localhost port
        run: timeout 20 bash -c 'until echo >/dev/tcp/127.0.0.1/6379; do sleep 1; done'
  artifacts-hosted:
    name: Artifacts / GitHub hosted
    if: inputs.mode == 'features' || inputs.mode == 'artifacts'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    permissions:
      contents: read
      actions: write
    steps:
      - name: Write proof
        run: echo g4-artifact > g4-proof.txt
      - name: Upload proof
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
        with:
          name: g4-proof-hosted
          path: g4-proof.txt
          if-no-files-found: error
  artifacts-scale-set:
    name: Artifacts / Velnor Scale Set
    if: inputs.mode == 'features' || inputs.mode == 'artifacts'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    permissions:
      contents: read
      actions: write
    steps:
      - name: Write proof
        run: echo g4-artifact > g4-proof.txt
      - name: Upload proof
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
        with:
          name: g4-proof-scale-set
          path: g4-proof.txt
          if-no-files-found: error
  buildx-hosted:
    name: Buildx / GitHub hosted
    if: inputs.mode == 'features' || inputs.mode == 'buildx'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Require GitHub-hosted stock Docker
        run: "set -euo pipefail\ntest -z \"${DOCKER_CONTEXT:-}\"\ncase \"${DOCKER_HOST:-}\" in\n  \"\"|unix:///var/run/docker.sock) ;;\n  *) printf '%s\\n' 'hosted Docker endpoint is not the stock socket' >&2; exit 1 ;;\nesac\ntest -S /var/run/docker.sock\ndocker --host unix:///var/run/docker.sock info >/dev/null"
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Buildx pinned context/cache probe
        run: "set -euo pipefail\nendpoint=\"unix:///var/run/docker.sock\"\nbuilder=\"velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}\"\nwork=\"$RUNNER_TEMP/${builder}\"\ncache=\"$work/cache\"\ncontext=\"$GITHUB_WORKSPACE/qualification/buildx-context\"\ncase \"$work\" in \"$RUNNER_TEMP\"/velnor-g4-*) ;; *) exit 1 ;; esac\nmkdir \"$work\"\ntest -f \"$context/Dockerfile\"\ntest -f \"$context/payload.txt\"\nnode \"$context/cache-hit.mjs\" --self-test\ndocker --host \"$endpoint\" buildx create --name \"$builder\" --driver docker-container --driver-opt \"image=docker.io/moby/buildkit@sha256:98cc6a3fc46220d00f8224ae483f3274fc874e9be8d7dd1e2e2c5481209228b5\" \"$endpoint\" >/dev/null\ndocker --host \"$endpoint\" buildx inspect \"$builder\" --bootstrap > \"$work/inspect.txt\"\nawk -v endpoint=\"$endpoint\" '$1 == \"Driver:\" && $2 == \"docker-container\" { driver=1 } $1 == \"Endpoint:\" && $2 == endpoint { route=1 } $1 == \"BuildKit:\" && $2 == \"v0.33.1\" { version=1 } END { exit !(driver && route && version) }' \"$work/inspect.txt\"\nif ! docker --host \"$endpoint\" buildx build --builder \"$builder\" --platform linux/amd64 --progress=plain --cache-to \"type=local,dest=$cache\" --output \"type=local,dest=$work/first-result\" --file \"$context/Dockerfile\" \"$context\" > \"$work/first-build.log\" 2>&1; then\n  cat \"$work/first-build.log\"\n  exit 1\nfi\ntest -s \"$cache/index.json\"\ncmp \"$context/payload.txt\" \"$work/first-result/payload.txt\"\ndocker --host \"$endpoint\" buildx prune --builder \"$builder\" --all --force\nif ! docker --host \"$endpoint\" buildx build --builder \"$builder\" --platform linux/amd64 --progress=plain --cache-from \"type=local,src=$cache\" --output \"type=local,dest=$work/second-result\" --file \"$context/Dockerfile\" \"$context\" > \"$work/second-build.log\" 2>&1; then\n  cat \"$work/second-build.log\"\n  exit 1\nfi\nif ! node \"$context/cache-hit.mjs\" \"$work/second-build.log\"; then\n  cat \"$work/second-build.log\"\n  printf '%s\\n' 'Buildx did not report a hit for the pinned local context' >&2\n  exit 1\nfi\ncmp \"$context/payload.txt\" \"$work/second-result/payload.txt\""
      - name: Remove Buildx builder and local cache
        if: always()
        run: "set -euo pipefail\nendpoint=\"unix:///var/run/docker.sock\"\nbuilder=\"velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}\"\nwork=\"$RUNNER_TEMP/${builder}\"\ncase \"$work\" in \"$RUNNER_TEMP\"/velnor-g4-*) ;; *) exit 1 ;; esac\nmkdir -p \"$work\"\ndocker --host \"$endpoint\" buildx ls --format '{{.Name}}' > \"$work/builders-before-cleanup\"\nif grep -Fxq \"$builder\" \"$work/builders-before-cleanup\"; then\n  docker --host \"$endpoint\" buildx rm \"$builder\"\nfi\ndocker --host \"$endpoint\" buildx ls --format '{{.Name}}' > \"$work/builders-after-cleanup\"\nif grep -Fxq \"$builder\" \"$work/builders-after-cleanup\"; then\n  printf '%s\\n' 'Buildx builder remained after cleanup' >&2\n  exit 1\nfi\nrm -rf \"$work\""
  buildx-scale-set:
    name: Buildx / Velnor Scale Set
    if: inputs.mode == 'features' || inputs.mode == 'buildx'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Require Velnor private DinD socket
        run: "set -euo pipefail\ntest -z \"${DOCKER_CONTEXT:-}\"\ntest \"${DOCKER_HOST:-}\" = \"unix:///run/docker/docker.sock\"\ntest -S /run/docker/docker.sock\ntest ! -e /var/run/docker.sock\ndocker --host unix:///run/docker/docker.sock info >/dev/null"
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Buildx pinned context/cache probe
        run: "set -euo pipefail\nendpoint=\"unix:///run/docker/docker.sock\"\nbuilder=\"velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}\"\nwork=\"$RUNNER_TEMP/${builder}\"\ncache=\"$work/cache\"\ncontext=\"$GITHUB_WORKSPACE/qualification/buildx-context\"\ncase \"$work\" in \"$RUNNER_TEMP\"/velnor-g4-*) ;; *) exit 1 ;; esac\nmkdir \"$work\"\ntest -f \"$context/Dockerfile\"\ntest -f \"$context/payload.txt\"\nnode \"$context/cache-hit.mjs\" --self-test\ndocker --host \"$endpoint\" buildx create --name \"$builder\" --driver docker-container --driver-opt \"image=docker.io/moby/buildkit@sha256:98cc6a3fc46220d00f8224ae483f3274fc874e9be8d7dd1e2e2c5481209228b5\" \"$endpoint\" >/dev/null\ndocker --host \"$endpoint\" buildx inspect \"$builder\" --bootstrap > \"$work/inspect.txt\"\nawk -v endpoint=\"$endpoint\" '$1 == \"Driver:\" && $2 == \"docker-container\" { driver=1 } $1 == \"Endpoint:\" && $2 == endpoint { route=1 } $1 == \"BuildKit:\" && $2 == \"v0.33.1\" { version=1 } END { exit !(driver && route && version) }' \"$work/inspect.txt\"\nif ! docker --host \"$endpoint\" buildx build --builder \"$builder\" --platform linux/amd64 --progress=plain --cache-to \"type=local,dest=$cache\" --output \"type=local,dest=$work/first-result\" --file \"$context/Dockerfile\" \"$context\" > \"$work/first-build.log\" 2>&1; then\n  cat \"$work/first-build.log\"\n  exit 1\nfi\ntest -s \"$cache/index.json\"\ncmp \"$context/payload.txt\" \"$work/first-result/payload.txt\"\ndocker --host \"$endpoint\" buildx prune --builder \"$builder\" --all --force\nif ! docker --host \"$endpoint\" buildx build --builder \"$builder\" --platform linux/amd64 --progress=plain --cache-from \"type=local,src=$cache\" --output \"type=local,dest=$work/second-result\" --file \"$context/Dockerfile\" \"$context\" > \"$work/second-build.log\" 2>&1; then\n  cat \"$work/second-build.log\"\n  exit 1\nfi\nif ! node \"$context/cache-hit.mjs\" \"$work/second-build.log\"; then\n  cat \"$work/second-build.log\"\n  printf '%s\\n' 'Buildx did not report a hit for the pinned local context' >&2\n  exit 1\nfi\ncmp \"$context/payload.txt\" \"$work/second-result/payload.txt\""
      - name: Remove Buildx builder and local cache
        if: always()
        run: "set -euo pipefail\nendpoint=\"unix:///run/docker/docker.sock\"\nbuilder=\"velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}\"\nwork=\"$RUNNER_TEMP/${builder}\"\ncase \"$work\" in \"$RUNNER_TEMP\"/velnor-g4-*) ;; *) exit 1 ;; esac\nmkdir -p \"$work\"\ndocker --host \"$endpoint\" buildx ls --format '{{.Name}}' > \"$work/builders-before-cleanup\"\nif grep -Fxq \"$builder\" \"$work/builders-before-cleanup\"; then\n  docker --host \"$endpoint\" buildx rm \"$builder\"\nfi\ndocker --host \"$endpoint\" buildx ls --format '{{.Name}}' > \"$work/builders-after-cleanup\"\nif grep -Fxq \"$builder\" \"$work/builders-after-cleanup\"; then\n  printf '%s\\n' 'Buildx builder remained after cleanup' >&2\n  exit 1\nfi\nrm -rf \"$work\""
  expect-fail-hosted:
    name: Expected negative / GitHub hosted
    if: inputs.mode == 'negative'
    runs-on: ubuntu-26.04
    timeout-minutes: 10
    steps:
      - name: Intentional failure
        run: echo expected-negative && exit 1
  expect-fail-scale-set:
    name: Expected negative / Velnor Scale Set
    if: inputs.mode == 'negative'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 10
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Intentional failure
        run: echo expected-negative && exit 1
"#,
    include_str!("fixtures/schema2_class_snapshot.txt"),
    include_str!("fixtures/schema2_class_snapshot_continuation.txt"),
    include_str!("fixtures/schema2_topology_snapshot.txt"),
);
