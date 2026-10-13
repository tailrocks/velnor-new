/// `qualification.yml` body, without the generator marker.
pub(super) const QUALIFICATION: &str = r##"name: Qualification
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
        uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9
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
        uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9
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
  composite-hosted:
    name: Composite / GitHub hosted
    if: inputs.mode == 'composite'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Local composite
        uses: ./qualification/actions/composite # zizmor: ignore[self-repository]
  composite-scale-set:
    name: Composite / Velnor Scale Set
    if: inputs.mode == 'composite'
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
      - name: Local composite
        uses: ./qualification/actions/composite # zizmor: ignore[self-repository]
  js-pin-hosted:
    name: Pinned JavaScript / GitHub hosted
    if: inputs.mode == 'js-pin'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Pinned script
        uses: actions/github-script@3a2844b7e9c422d3c10d287c895573f7108da1b3
        with:
          script: console.log('js-pin-ok')
  js-pin-scale-set:
    name: Pinned JavaScript / Velnor Scale Set
    if: inputs.mode == 'js-pin'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Pinned script
        uses: actions/github-script@3a2844b7e9c422d3c10d287c895573f7108da1b3
        with:
          script: console.log('js-pin-ok')
  docker-action-hosted:
    name: Docker action / GitHub hosted
    if: inputs.mode == 'docker-action'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Local docker
        uses: ./qualification/actions/docker # zizmor: ignore[self-repository]
  docker-action-scale-set:
    name: Docker action / Velnor Scale Set
    if: inputs.mode == 'docker-action'
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
      - name: Local docker
        uses: ./qualification/actions/docker # zizmor: ignore[self-repository]
  container-hosted:
    name: Container / GitHub hosted
    if: inputs.mode == 'container'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    defaults:
      run:
        shell: sh -e {0}
    container: alpine:3.22
    services:
      redis:
        image: redis:7-alpine
        options: "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12"
        ports:
          - "6379:6379"
    steps:
      - name: Service DNS
        shell: sh
        run: "i=0; while [ \"$i\" -lt 20 ]; do nc -z -w 1 redis 6379 && exit 0; i=$((i+1)); sleep 1; done; exit 1"
  container-scale-set:
    name: Container / Velnor Scale Set
    if: inputs.mode == 'container'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: sh -e {0}
    container: alpine:3.22
    services:
      redis:
        image: redis:7-alpine
        options: "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12"
        ports:
          - "6379:6379"
    steps:
      - name: Service DNS
        shell: sh
        run: "i=0; while [ \"$i\" -lt 20 ]; do nc -z -w 1 redis 6379 && exit 0; i=$((i+1)); sleep 1; done; exit 1"
  outputs-scale-set:
    name: Outputs / Velnor Scale Set
    if: inputs.mode == 'outputs'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    outputs:
      proof: ${{ steps.emit.outputs.proof }}
    steps:
      - name: Write output env and path
        id: emit
        run: "mkdir -p \"$RUNNER_TEMP/g4-bin\" && printf '%s\\n' '#!/bin/sh' 'echo path-ok' > \"$RUNNER_TEMP/g4-bin/g4-path-ok\" && chmod +x \"$RUNNER_TEMP/g4-bin/g4-path-ok\" && echo proof=outputs-ok >> \"$GITHUB_OUTPUT\" && echo G4_ENV=outputs-ok >> \"$GITHUB_ENV\" && echo \"$RUNNER_TEMP/g4-bin\" >> \"$GITHUB_PATH\""
      - name: Check env and path
        run: "test \"$G4_ENV\" = outputs-ok && g4-path-ok | grep -qx path-ok"
  outputs-hosted:
    name: Outputs / GitHub hosted
    if: inputs.mode == 'outputs'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    needs:
      - outputs-scale-set
    steps:
      - name: Check job output
        env:
          PROOF: ${{ needs.outputs-scale-set.outputs.proof }}
        run: "test \"$PROOF\" = outputs-ok"
  mask-hosted:
    name: Mask / GitHub hosted
    if: inputs.mode == 'mask'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Mask canary
        run: "echo \"::add-mask::g4-mask-canary\" && echo g4-mask-canary"
  mask-scale-set:
    name: Mask / Velnor Scale Set
    if: inputs.mode == 'mask'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Mask canary
        run: "echo \"::add-mask::g4-mask-canary\" && echo g4-mask-canary"
  cache-scale-set:
    name: Cache / Velnor Scale Set
    if: inputs.mode == 'cache'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    permissions:
      contents: read
      actions: write
    steps:
      - name: Write cache file
        run: echo cache-ok > g4-cache.txt
      - name: Save cache
        uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9
        with:
          path: g4-cache.txt
          key: g4-cache-${{ github.run_id }}
  cache-hosted:
    name: Cache / GitHub hosted
    if: inputs.mode == 'cache'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    needs:
      - cache-scale-set
    permissions:
      contents: read
      actions: write
    steps:
      - name: Restore cache
        uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9
        with:
          path: g4-cache.txt
          key: g4-cache-${{ github.run_id }}
          fail-on-cache-miss: "true"
      - name: Check restore
        run: grep -qx cache-ok g4-cache.txt
  oidc-hosted:
    name: OIDC / GitHub hosted
    if: inputs.mode == 'oidc'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    permissions:
      id-token: write
      contents: read
    steps:
      - name: Require OIDC request URL
        run: "test -n \"$ACTIONS_ID_TOKEN_REQUEST_URL\""
  oidc-scale-set:
    name: OIDC / Velnor Scale Set
    if: inputs.mode == 'oidc'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    permissions:
      id-token: write
      contents: read
    steps:
      - name: Require OIDC request URL
        run: "test -n \"$ACTIONS_ID_TOKEN_REQUEST_URL\""
  post-fail-hosted:
    name: Post failure / GitHub hosted
    if: inputs.mode == 'post-fail'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Main fails post runs
        uses: ./qualification/actions/post-fail # zizmor: ignore[self-repository]
  post-fail-scale-set:
    name: Post failure / Velnor Scale Set
    if: inputs.mode == 'post-fail'
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
      - name: Main fails post runs
        uses: ./qualification/actions/post-fail # zizmor: ignore[self-repository]
  cancel-hosted:
    name: Cancel / GitHub hosted
    if: inputs.mode == 'cancel'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Sleep until cancelled
        run: sleep 180
  cancel-scale-set:
    name: Cancel / Velnor Scale Set
    if: inputs.mode == 'cancel'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Sleep until cancelled
        run: sleep 180
  compose-hosted:
    name: Compose / GitHub hosted
    if: inputs.mode == 'compose'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Require GitHub-hosted stock Docker
        run: "set -euo pipefail\ntest -z \"${DOCKER_CONTEXT:-}\"\ncase \"${DOCKER_HOST:-}\" in\n  \"\"|unix:///var/run/docker.sock) ;;\n  *) printf '%s\\n' 'hosted Docker endpoint is not the stock socket' >&2; exit 1 ;;\nesac\ntest -S /var/run/docker.sock\ndocker --host unix:///var/run/docker.sock info >/dev/null"
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Start compose
        run: "docker --host unix:///var/run/docker.sock compose --project-name \"velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}\" -f qualification/compose/stack.yml up -d --wait"
      - name: Prove both services
        run: "docker --host unix:///var/run/docker.sock compose --project-name \"velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}\" -f qualification/compose/stack.yml ps --services --status running > \"$RUNNER_TEMP/g4-compose-ps\" && grep -qx api \"$RUNNER_TEMP/g4-compose-ps\" && grep -qx db \"$RUNNER_TEMP/g4-compose-ps\""
      - name: Remove compose
        if: always()
        run: "docker --host unix:///var/run/docker.sock compose --project-name \"velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}\" -f qualification/compose/stack.yml down --volumes"
      - name: Record compose
        run: echo compose-ok
  compose-scale-set:
    name: Compose / Velnor Scale Set
    if: inputs.mode == 'compose'
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
      - name: Start compose
        run: "docker --host unix:///run/docker/docker.sock compose --project-name \"velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}\" -f qualification/compose/stack.yml up -d --wait"
      - name: Prove both services
        run: "docker --host unix:///run/docker/docker.sock compose --project-name \"velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}\" -f qualification/compose/stack.yml ps --services --status running > \"$RUNNER_TEMP/g4-compose-ps\" && grep -qx api \"$RUNNER_TEMP/g4-compose-ps\" && grep -qx db \"$RUNNER_TEMP/g4-compose-ps\""
      - name: Remove compose
        if: always()
        run: "docker --host unix:///run/docker/docker.sock compose --project-name \"velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}\" -f qualification/compose/stack.yml down --volumes"
      - name: Record compose
        run: echo compose-ok
  bind-hosted:
    name: Bind / GitHub hosted
    if: inputs.mode == 'bind'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Bind workspace file
        run: "printf '%s\\n' bind-ok > \"$GITHUB_WORKSPACE/g4-bind.txt\" && docker run --rm -v \"$GITHUB_WORKSPACE/g4-bind.txt:/g4-bind.txt:ro\" alpine:3.22 cat /g4-bind.txt > \"$RUNNER_TEMP/g4-bind-out\" && grep -qx bind-ok \"$RUNNER_TEMP/g4-bind-out\""
  bind-scale-set:
    name: Bind / Velnor Scale Set
    if: inputs.mode == 'bind'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Bind workspace file
        run: "printf '%s\\n' bind-ok > \"$GITHUB_WORKSPACE/g4-bind.txt\" && docker run --rm -v \"$GITHUB_WORKSPACE/g4-bind.txt:/g4-bind.txt:ro\" alpine:3.22 cat /g4-bind.txt > \"$RUNNER_TEMP/g4-bind-out\" && grep -qx bind-ok \"$RUNNER_TEMP/g4-bind-out\""
  cancel-service-hosted:
    name: Cancel service / GitHub hosted
    if: inputs.mode == 'cancel-service'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    services:
      redis:
        image: redis:7-alpine
        options: "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12"
        ports:
          - "6379:6379"
    steps:
      - name: Probe service then sleep
        run: "i=0; while [ \"$i\" -lt 30 ]; do nc -z -w 1 127.0.0.1 6379 && break; i=$((i+1)); sleep 1; done; nc -z -w 1 127.0.0.1 6379 && echo service-up && sleep 900"
  cancel-service-scale-set:
    name: Cancel service / Velnor Scale Set
    if: inputs.mode == 'cancel-service'
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
      - name: Probe service then sleep
        run: "i=0; while [ \"$i\" -lt 30 ]; do nc -z -w 1 127.0.0.1 6379 && break; i=$((i+1)); sleep 1; done; nc -z -w 1 127.0.0.1 6379 && echo service-up && sleep 900"
  testcontainers-hosted:
    name: Testcontainers / GitHub hosted
    if: inputs.mode == 'testcontainers'
    runs-on: ubuntu-26.04
    timeout-minutes: 30
    env:
      DOCKER_HOST: unix:///var/run/docker.sock
      DOCKER_CONTEXT: ""
      TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE: /var/run/docker.sock
      TESTCONTAINERS_HOST_OVERRIDE: localhost
      TESTCONTAINERS_RYUK_DISABLED: "false"
      TESTCONTAINERS_RYUK_TEST_LABEL: "true"
      RYUK_CONTAINER_IMAGE: docker.io/testcontainers/ryuk@sha256:f0456560ea5b4acdbed0da0efc33b5f9dd6bc1e59f2337106826dcb5b0b0e981
      VELNOR_TESTCONTAINERS_REDIS_IMAGE: docker.io/library/redis@sha256:ca0acbb137c1dc3339c8b147a58fd6f42775d4599327b50e7b116c23de501af2
    steps:
      - name: Require GitHub-hosted stock Docker
        run: "set -euo pipefail\ntest -z \"${DOCKER_CONTEXT:-}\"\ncase \"${DOCKER_HOST:-}\" in\n  \"\"|unix:///var/run/docker.sock) ;;\n  *) printf '%s\\n' 'hosted Docker endpoint is not the stock socket' >&2; exit 1 ;;\nesac\ntest -S /var/run/docker.sock\ndocker --host unix:///var/run/docker.sock info >/dev/null"
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Prove Testcontainers and Ryuk lifecycle
        run: node --version && npm ci --engine-strict --ignore-scripts --prefix qualification/testcontainers && npm test --prefix qualification/testcontainers && node qualification/testcontainers/reap.mjs
  testcontainers-scale-set:
    name: Testcontainers / Velnor Scale Set
    if: inputs.mode == 'testcontainers'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 30
    defaults:
      run:
        shell: bash -e {0}
    env:
      DOCKER_HOST: unix:///run/docker/docker.sock
      DOCKER_CONTEXT: ""
      TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE: /run/docker/docker.sock
      TESTCONTAINERS_HOST_OVERRIDE: localhost
      TESTCONTAINERS_RYUK_DISABLED: "false"
      TESTCONTAINERS_RYUK_TEST_LABEL: "true"
      RYUK_CONTAINER_IMAGE: docker.io/testcontainers/ryuk@sha256:f0456560ea5b4acdbed0da0efc33b5f9dd6bc1e59f2337106826dcb5b0b0e981
      VELNOR_TESTCONTAINERS_REDIS_IMAGE: docker.io/library/redis@sha256:ca0acbb137c1dc3339c8b147a58fd6f42775d4599327b50e7b116c23de501af2
    steps:
      - name: Require Velnor private DinD socket
        run: "set -euo pipefail\ntest -z \"${DOCKER_CONTEXT:-}\"\ntest \"${DOCKER_HOST:-}\" = \"unix:///run/docker/docker.sock\"\ntest -S /run/docker/docker.sock\ntest ! -e /var/run/docker.sock\ndocker --host unix:///run/docker/docker.sock info >/dev/null"
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Prove Testcontainers and Ryuk lifecycle
        run: node --version && npm ci --engine-strict --ignore-scripts --prefix qualification/testcontainers && npm test --prefix qualification/testcontainers && node qualification/testcontainers/reap.mjs
  submodule-hosted:
    name: Submodule / GitHub hosted
    if: inputs.mode == 'submodule'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          submodules: recursive
          lfs: "true"
          persist-credentials: "false"
      - name: Prove submodule and LFS
        run: "git rev-parse HEAD > \"$RUNNER_TEMP/g4-head\" && grep -qx \"$GITHUB_SHA\" \"$RUNNER_TEMP/g4-head\" && grep -qx submodule-ok qualification/fixtures/submodule/MARKER && grep -qx lfs-ok qualification/fixtures/lfs-marker.txt"
  submodule-scale-set:
    name: Submodule / Velnor Scale Set
    if: inputs.mode == 'submodule'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          submodules: recursive
          lfs: "true"
          persist-credentials: "false"
      - name: Prove submodule and LFS
        run: "git rev-parse HEAD > \"$RUNNER_TEMP/g4-head\" && grep -qx \"$GITHUB_SHA\" \"$RUNNER_TEMP/g4-head\" && grep -qx submodule-ok qualification/fixtures/submodule/MARKER && grep -qx lfs-ok qualification/fixtures/lfs-marker.txt"
  ports-a-hosted:
    name: Ports A / GitHub hosted
    if: inputs.mode == 'ports'
    runs-on: ubuntu-26.04
    timeout-minutes: 30
    steps:
      - name: Hold host port
        run: "docker run -d --name g4-hold -p 8080:80 alpine:3.22 sleep 120 && i=0 && while [ \"$i\" -lt 30 ]; do docker port g4-hold 80 | grep -q 8080 && break; i=$((i+1)); sleep 1; done && docker port g4-hold 80 | grep -q 8080 && echo port-held && sleep 45 && docker rm -f g4-hold"
  ports-b-hosted:
    name: Ports B / GitHub hosted
    if: inputs.mode == 'ports'
    runs-on: ubuntu-26.04
    timeout-minutes: 30
    steps:
      - name: Hold host port
        run: "docker run -d --name g4-hold -p 8080:80 alpine:3.22 sleep 120 && i=0 && while [ \"$i\" -lt 30 ]; do docker port g4-hold 80 | grep -q 8080 && break; i=$((i+1)); sleep 1; done && docker port g4-hold 80 | grep -q 8080 && echo port-held && sleep 45 && docker rm -f g4-hold"
  ports-a-scale-set:
    name: Ports A / Velnor Scale Set
    if: inputs.mode == 'ports'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 30
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Hold host port
        run: "docker run -d --name g4-hold -p 8080:80 alpine:3.22 sleep 120 && i=0 && while [ \"$i\" -lt 30 ]; do docker port g4-hold 80 | grep -q 8080 && break; i=$((i+1)); sleep 1; done && docker port g4-hold 80 | grep -q 8080 && echo port-held && sleep 45 && docker rm -f g4-hold"
  ports-b-scale-set:
    name: Ports B / Velnor Scale Set
    if: inputs.mode == 'ports'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 30
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Hold host port
        run: "docker run -d --name g4-hold -p 8080:80 alpine:3.22 sleep 120 && i=0 && while [ \"$i\" -lt 30 ]; do docker port g4-hold 80 | grep -q 8080 && break; i=$((i+1)); sleep 1; done && docker port g4-hold 80 | grep -q 8080 && echo port-held && sleep 45 && docker rm -f g4-hold"
  pressure-a-hosted:
    name: Pressure A / GitHub hosted
    if: inputs.mode == 'pressure'
    runs-on: ubuntu-26.04
    timeout-minutes: 30
    steps:
      - name: Pressure sleep
        run: echo pressure-start && sleep 150 && echo pressure-ok
  pressure-b-hosted:
    name: Pressure B / GitHub hosted
    if: inputs.mode == 'pressure'
    runs-on: ubuntu-26.04
    timeout-minutes: 30
    steps:
      - name: Pressure sleep
        run: echo pressure-start && sleep 150 && echo pressure-ok
  pressure-c-hosted:
    name: Pressure C / GitHub hosted
    if: inputs.mode == 'pressure'
    runs-on: ubuntu-26.04
    timeout-minutes: 30
    steps:
      - name: Pressure sleep
        run: echo pressure-start && sleep 150 && echo pressure-ok
  pressure-a-scale-set:
    name: Pressure A / Velnor Scale Set
    if: inputs.mode == 'pressure'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 30
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Pressure sleep
        run: echo pressure-start && sleep 150 && echo pressure-ok
  pressure-b-scale-set:
    name: Pressure B / Velnor Scale Set
    if: inputs.mode == 'pressure'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 30
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Pressure sleep
        run: echo pressure-start && sleep 150 && echo pressure-ok
  pressure-c-scale-set:
    name: Pressure C / Velnor Scale Set
    if: inputs.mode == 'pressure'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 30
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Pressure sleep
        run: echo pressure-start && sleep 150 && echo pressure-ok
  secret-hosted:
    name: Secret / GitHub hosted
    if: inputs.mode == 'secret'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    steps:
      - name: Hold secret canary
        env:
          G3_CANARY: ${{ secrets.G3_CANARY }}
        run: "test -n \"$G3_CANARY\" && sleep 180"
  secret-scale-set:
    name: Secret / Velnor Scale Set
    if: inputs.mode == 'secret'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Hold secret canary
        env:
          G3_CANARY: ${{ secrets.G3_CANARY }}
        run: "test -n \"$G3_CANARY\" && sleep 180"
  mbx-cache-write-hosted:
    name: MBX objects cache / protected-main writer
    if: inputs.mode == 'mbx-cache-roundtrip' && github.ref == 'refs/heads/main' && github.ref_protected == true
    runs-on: ubuntu-26.04
    timeout-minutes: 45
    permissions:
      contents: read
      actions: write
    env:
      MBX_GC_AUTO: "1"
      MBX_SHARE_OUT_DIR: "0"
      ACTIONS_CACHE_MODE: write
      CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo
      MISE_AUTO_INSTALL: "false"
      MISE_CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo
      MISE_EXEC_AUTO_INSTALL: "false"
      MISE_LOCKFILE: "0"
      MISE_NO_CONFIG: "1"
      MISE_NO_ENV: "1"
      MISE_NO_HOOKS: "1"
      MISE_RUSTUP_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/rustup
      RUSTUP_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/rustup
      RUSTUP_TOOLCHAIN: 1.98.1
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Set up Mise
        uses: jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5
        with:
          version: 2026.9.18
          sha256: d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4
          install: "false"
          env: "false"
          cache: "false"
          cache_save: "false"
      - name: Install pinned Rust toolchain
        run: "mise install rust@1.98.1 && printf '%s/bin\\n' \"$(mise exec rust@1.98.1 -- rustc --print sysroot)\" >> \"$GITHUB_PATH\""
      - name: Restore MBX objects
        uses: jdx/mr-boxington-action@d0825fbaf3cc36ca2609aa38e71046265a1f1e37
        id: mbx_cache
        with:
          github-cache-mode: objects
          version: 1.21.1
          toolchain: 1.98.1
          isolate-objects-cache: "true"
          cache-generation: velnor-qualification-mbx-1.21.1-share-out-dir-disabled-v1-action-d0825fbaf3cc36ca2609aa38e71046265a1f1e37-run-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.sha }}
          save-on-workflow-dispatch: "true"
      - name: Verify MBX action identity and write policy
        env:
          MBX_VERSION: ${{ steps.mbx_cache.outputs.mbx-version }}
          CACHE_SAVE_ELIGIBLE: ${{ steps.mbx_cache.outputs.cache-save-eligible }}
          CACHE_SAVE_REASON: ${{ steps.mbx_cache.outputs.cache-save-reason }}
          CACHE_HIT: ${{ steps.mbx_cache.outputs.cache-hit }}
        run: "test \"$MBX_VERSION\" = '1.21.1' && test \"$CACHE_SAVE_ELIGIBLE\" = 'true' && test \"$CACHE_SAVE_REASON\" = 'workflow_dispatch'"
      - name: Compile MBX cache probe
        run: "set -eu\nroot=\"$GITHUB_WORKSPACE/.velnor-mbx-cache-qualification\"\nmkdir -p \"$root/src\"\ncat > \"$root/Cargo.toml\" <<'EOF'\n[package]\nname = \"mbx-cache-qualification\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\nmembers = [\".\"]\nresolver = \"3\"\n\n[lib]\npath = \"src/lib.rs\"\nEOF\ncat > \"$root/src/lib.rs\" <<'EOF'\npub fn cache_probe() -> u64 { 42 }\nEOF\nmbx build --manifest-path \"$root/Cargo.toml\"\n"
      - name: Sample runner disk
        run: "df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\""
  mbx-cache-read-hosted:
    name: MBX objects cache / read-only reuse
    if: inputs.mode == 'mbx-cache-roundtrip' && github.ref == 'refs/heads/main' && github.ref_protected == true
    runs-on: ubuntu-26.04
    timeout-minutes: 45
    needs:
      - mbx-cache-write-hosted
    permissions:
      contents: read
      actions: read
    env:
      MBX_GC_AUTO: "1"
      MBX_SHARE_OUT_DIR: "0"
      ACTIONS_CACHE_MODE: read
      CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo
      MISE_AUTO_INSTALL: "false"
      MISE_CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo
      MISE_EXEC_AUTO_INSTALL: "false"
      MISE_LOCKFILE: "0"
      MISE_NO_CONFIG: "1"
      MISE_NO_ENV: "1"
      MISE_NO_HOOKS: "1"
      MISE_RUSTUP_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/rustup
      RUSTUP_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/rustup
      RUSTUP_TOOLCHAIN: 1.98.1
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Set up Mise
        uses: jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5
        with:
          version: 2026.9.18
          sha256: d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4
          install: "false"
          env: "false"
          cache: "false"
          cache_save: "false"
      - name: Install pinned Rust toolchain
        run: "mise install rust@1.98.1 && printf '%s/bin\\n' \"$(mise exec rust@1.98.1 -- rustc --print sysroot)\" >> \"$GITHUB_PATH\""
      - name: Restore MBX objects
        uses: jdx/mr-boxington-action@d0825fbaf3cc36ca2609aa38e71046265a1f1e37
        id: mbx_cache
        with:
          github-cache-mode: objects
          version: 1.21.1
          toolchain: 1.98.1
          isolate-objects-cache: "true"
          cache-generation: velnor-qualification-mbx-1.21.1-share-out-dir-disabled-v1-action-d0825fbaf3cc36ca2609aa38e71046265a1f1e37-run-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.sha }}
          save-on-workflow-dispatch: "false"
      - name: Verify MBX action identity and write policy
        env:
          MBX_VERSION: ${{ steps.mbx_cache.outputs.mbx-version }}
          CACHE_SAVE_ELIGIBLE: ${{ steps.mbx_cache.outputs.cache-save-eligible }}
          CACHE_SAVE_REASON: ${{ steps.mbx_cache.outputs.cache-save-reason }}
          CACHE_HIT: ${{ steps.mbx_cache.outputs.cache-hit }}
        run: "test \"$MBX_VERSION\" = '1.21.1' && test \"$CACHE_SAVE_ELIGIBLE\" = 'false' && test \"$CACHE_SAVE_REASON\" = 'workflow_dispatch; save-on-workflow-dispatch is off' && test \"$CACHE_HIT\" = 'false'"
      - name: Require imported MBX objects
        run: "set -e -o pipefail; df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\"; mbx cache stats --json | tee \"$RUNNER_TEMP/mbx-object-stats.json\"; jq -e '.objects > 0' \"$RUNNER_TEMP/mbx-object-stats.json\""
      - name: Compile MBX cache probe
        run: "set -eu\nroot=\"$GITHUB_WORKSPACE/.velnor-mbx-cache-qualification\"\nmkdir -p \"$root/src\"\ncat > \"$root/Cargo.toml\" <<'EOF'\n[package]\nname = \"mbx-cache-qualification\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\nmembers = [\".\"]\nresolver = \"3\"\n\n[lib]\npath = \"src/lib.rs\"\nEOF\ncat > \"$root/src/lib.rs\" <<'EOF'\npub fn cache_probe() -> u64 { 42 }\nEOF\nmbx build --manifest-path \"$root/Cargo.toml\"\n"
      - name: Require reused compilation
        run: "set -e -o pipefail; df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\"; mbx stats --json | tee \"$RUNNER_TEMP/mbx-reuse-stats.json\"; jq -e '.savings.cached_compilations > 0' \"$RUNNER_TEMP/mbx-reuse-stats.json\""
  topology-runner-host:
    name: Worker topology / runner host namespace
    if: inputs.mode == 'topology'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    services:
      redis:
        image: docker.io/library/redis@sha256:ca0acbb137c1dc3339c8b147a58fd6f42775d4599327b50e7b116c23de501af2
        options: "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12"
        ports:
          - "49327:6379"
    steps:
      - name: Require Velnor private DinD socket
        run: "set -euo pipefail\ntest -z \"${DOCKER_CONTEXT:-}\"\ntest \"${DOCKER_HOST:-}\" = \"unix:///run/docker/docker.sock\"\ntest -S /run/docker/docker.sock\ntest ! -e /var/run/docker.sock\ndocker --host unix:///run/docker/docker.sock info >/dev/null"
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Check runner workspace and host-mode service
        run: "set -euo pipefail\nmarker=\".velnor-topology.txt\"\nprintf '%s\\n' runner-work-volume-ok > \"$GITHUB_WORKSPACE/$marker\"\ntest -w \"$GITHUB_WORKSPACE\"\nprobe_redis() {\n  local response\n  exec 3<>/dev/tcp/127.0.0.1/49327 || return 1\n  printf 'PING\\r\\n' >&3 || return 1\n  IFS= read -r -t 2 response <&3 || return 1\n  exec 3>&-\n  test \"${response%$'\\r'}\" = \"+PONG\"\n}\nready=0\ndeadline=$((SECONDS + 60))\nwhile [ \"$SECONDS\" -lt \"$deadline\" ]; do\n  if probe_redis; then ready=1; break; fi\n  sleep 1\ndone\ntest \"$ready\" -eq 1\nprintf '%s\\n' 'runner-workspace-ok' 'host-mode-localhost-ok'"
      - name: Check Docker workspace bind
        run: "set -euo pipefail\nalpine='docker.io/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764'\ndocker --host unix:///run/docker/docker.sock pull \"$alpine\" >/dev/null\ndocker --host unix:///run/docker/docker.sock run --rm --pull=never --network=none \\\n  --mount \"type=bind,src=$GITHUB_WORKSPACE,dst=/probe,readonly\" \\\n  --entrypoint /bin/sh \"$alpine\" -ec \\\n  'grep -qx runner-work-volume-ok /probe/.velnor-topology.txt'\nprintf '%s\\n' 'docker-workspace-mount-ok'"
      - name: Check read-only runner externals
        run: "set -euo pipefail\ndocker --host unix:///run/docker/docker.sock run --rm --pull=never --network=none \\\n  --mount type=bind,src=/home/runner/externals,dst=/probe,readonly \\\n  --entrypoint /bin/sh docker.io/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764 -ec \\\n  'test -d /probe && ls -A /probe | grep -q .; if : > /probe/.velnor-external-write-probe 2>/dev/null; then rm -f /probe/.velnor-external-write-probe; exit 72; fi'\nprintf '%s\\n' 'externals-read-only-ok'"
  topology-runner-host-hosted:
    name: Worker topology / runner host namespace / GitHub hosted
    if: inputs.mode == 'topology'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    services:
      redis:
        image: docker.io/library/redis@sha256:ca0acbb137c1dc3339c8b147a58fd6f42775d4599327b50e7b116c23de501af2
        options: "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12"
        ports:
          - "49327:6379"
    steps:
      - name: Require GitHub-hosted stock Docker
        run: "set -euo pipefail\ntest -z \"${DOCKER_CONTEXT:-}\"\ncase \"${DOCKER_HOST:-}\" in\n  \"\"|unix:///var/run/docker.sock) ;;\n  *) printf '%s\\n' 'hosted Docker endpoint is not the stock socket' >&2; exit 1 ;;\nesac\ntest -S /var/run/docker.sock\ndocker --host unix:///var/run/docker.sock info >/dev/null"
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Check runner workspace and host-mode service
        run: "set -euo pipefail\nmarker=\".velnor-topology.txt\"\nprintf '%s\\n' runner-work-volume-ok > \"$GITHUB_WORKSPACE/$marker\"\ntest -w \"$GITHUB_WORKSPACE\"\nprobe_redis() {\n  local response\n  exec 3<>/dev/tcp/127.0.0.1/49327 || return 1\n  printf 'PING\\r\\n' >&3 || return 1\n  IFS= read -r -t 2 response <&3 || return 1\n  exec 3>&-\n  test \"${response%$'\\r'}\" = \"+PONG\"\n}\nready=0\ndeadline=$((SECONDS + 60))\nwhile [ \"$SECONDS\" -lt \"$deadline\" ]; do\n  if probe_redis; then ready=1; break; fi\n  sleep 1\ndone\ntest \"$ready\" -eq 1\nprintf '%s\\n' 'runner-workspace-ok' 'host-mode-localhost-ok'"
      - name: Check Docker workspace bind
        run: "set -euo pipefail\nalpine='docker.io/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764'\ndocker --host unix:///var/run/docker.sock pull \"$alpine\" >/dev/null\ndocker --host unix:///var/run/docker.sock run --rm --pull=never --network=none \\\n  --mount \"type=bind,src=$GITHUB_WORKSPACE,dst=/probe,readonly\" \\\n  --entrypoint /bin/sh \"$alpine\" -ec \\\n  'grep -qx runner-work-volume-ok /probe/.velnor-topology.txt'\nprintf '%s\\n' 'docker-workspace-mount-ok'"
  topology-job-container:
    name: Worker topology / job container and service alias
    if: inputs.mode == 'topology'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: sh -e {0}
    needs:
      - topology-runner-host
    container: docker.io/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764
    services:
      redis:
        image: docker.io/library/redis@sha256:ca0acbb137c1dc3339c8b147a58fd6f42775d4599327b50e7b116c23de501af2
        options: "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12"
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Check job-container workspace and service DNS
        run: "set -eu\nmarker=\"$GITHUB_WORKSPACE/.velnor-container.txt\"\nprintf '%s\\n' job-container-work-volume-ok > \"$marker\"\ntest -w \"$GITHUB_WORKSPACE\"\ntest \"$(cat \"$marker\")\" = job-container-work-volume-ok\nresponse=\"$(printf 'PING\\r\\n' | busybox nc -w 3 redis 6379 | sed -n '1p' | tr -d '\\r')\"\ntest \"$response\" = '+PONG'\nprintf '%s\\n' 'job-container-workspace-ok' 'service-alias-ok'"
  topology-job-container-hosted:
    name: Worker topology / job container and service alias / GitHub hosted
    if: inputs.mode == 'topology'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    defaults:
      run:
        shell: sh -e {0}
    needs:
      - topology-runner-host-hosted
    container: docker.io/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764
    services:
      redis:
        image: docker.io/library/redis@sha256:ca0acbb137c1dc3339c8b147a58fd6f42775d4599327b50e7b116c23de501af2
        options: "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12"
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Check job-container workspace and service DNS
        run: "set -eu\nmarker=\"$GITHUB_WORKSPACE/.velnor-container.txt\"\nprintf '%s\\n' job-container-work-volume-ok > \"$marker\"\ntest -w \"$GITHUB_WORKSPACE\"\ntest \"$(cat \"$marker\")\" = job-container-work-volume-ok\nresponse=\"$(printf 'PING\\r\\n' | busybox nc -w 3 redis 6379 | sed -n '1p' | tr -d '\\r')\"\ntest \"$response\" = '+PONG'\nprintf '%s\\n' 'job-container-workspace-ok' 'service-alias-ok'"
  topology-docker-action:
    name: Worker topology / external and Docker actions
    if: inputs.mode == 'topology'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    defaults:
      run:
        shell: bash -e {0}
    needs:
      - topology-job-container
    steps:
      - name: Require Velnor private DinD socket
        run: "set -euo pipefail\ntest -z \"${DOCKER_CONTEXT:-}\"\ntest \"${DOCKER_HOST:-}\" = \"unix:///run/docker/docker.sock\"\ntest -S /run/docker/docker.sock\ntest ! -e /var/run/docker.sock\ndocker --host unix:///run/docker/docker.sock info >/dev/null"
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Seed Docker-action workspace probe
        run: "printf '%s\\n' docker-action-work-volume-ok > \"$GITHUB_WORKSPACE/.velnor-topology.txt\""
      - name: Local Docker action and workspace mount
        uses: ./qualification/actions/docker # zizmor: ignore[self-repository]
        env:
          VELNOR_TOPOLOGY_MARKER: .velnor-topology.txt
          DOCKER_HOST: unix:///run/docker/docker.sock
          DOCKER_CONTEXT: ""
  topology-docker-action-hosted:
    name: Worker topology / external and Docker actions / GitHub hosted
    if: inputs.mode == 'topology'
    runs-on: ubuntu-26.04
    timeout-minutes: 20
    needs:
      - topology-job-container-hosted
    steps:
      - name: Require GitHub-hosted stock Docker
        run: "set -euo pipefail\ntest -z \"${DOCKER_CONTEXT:-}\"\ncase \"${DOCKER_HOST:-}\" in\n  \"\"|unix:///var/run/docker.sock) ;;\n  *) printf '%s\\n' 'hosted Docker endpoint is not the stock socket' >&2; exit 1 ;;\nesac\ntest -S /var/run/docker.sock\ndocker --host unix:///var/run/docker.sock info >/dev/null"
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: "false"
      - name: Seed Docker-action workspace probe
        run: "printf '%s\\n' docker-action-work-volume-ok > \"$GITHUB_WORKSPACE/.velnor-topology.txt\""
      - name: Local Docker action and workspace mount
        uses: ./qualification/actions/docker # zizmor: ignore[self-repository]
        env:
          VELNOR_TOPOLOGY_MARKER: .velnor-topology.txt
          DOCKER_HOST: unix:///var/run/docker.sock
          DOCKER_CONTEXT: ""
"##;
