pub(super) const MONITORING: &str = r#"name: Scale set monitoring
"on":
  workflow_dispatch: {}
permissions:
  contents: read
jobs:
  scale-set-lane:
    name: Scale set lane
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 5
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Run scale-set lane
        run: echo scale-set-lane
    container:
      image: ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  queue-monitor:
    name: Queue monitor
    runs-on: ubuntu-26.04
    timeout-minutes: 5
    steps:
      - name: Watch admission
        run: echo queue-monitor
"#;
