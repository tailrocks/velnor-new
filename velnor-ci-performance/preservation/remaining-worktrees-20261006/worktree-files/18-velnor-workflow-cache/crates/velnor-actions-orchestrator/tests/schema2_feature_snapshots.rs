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
      - name: Buildx probe
        run: docker buildx version && printf 'FROM scratch\n' > Dockerfile && docker buildx build --progress=plain -t velnor-g4:probe .
  buildx-scale-set:
    name: Buildx / Velnor Scale Set
    if: inputs.mode == 'features' || inputs.mode == 'buildx'
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 20
    steps:
      - name: Buildx probe
        run: docker buildx version && printf 'FROM scratch\n' > Dockerfile && docker buildx build --progress=plain -t velnor-g4:probe .
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
    steps:
      - name: Intentional failure
        run: echo expected-negative && exit 1
"#,
    include_str!("schema2_class_snapshot.txt"),
);

/// Refresh the generated class-workflow suffix from the canonical fixture.
/// Set `VELNOR_UPDATE_SCHEMA2_CLASS_SNAPSHOT=1` to write the snapshot.
#[test]
fn update_class_snapshot_from_workflow_config() -> super::TestResult {
    if std::env::var("VELNOR_UPDATE_SCHEMA2_CLASS_SNAPSHOT")
        .ok()
        .as_deref()
        != Some("1")
    {
        return Ok(());
    }

    let repo = super::make_repo(&super::workflow_config())?;
    let tree = velnor_actions_orchestrator::render_staged_tree(
        &velnor_actions_orchestrator::prepare(repo.path())?,
    )?;
    let rendered = tree
        .get(".github/workflows/qualification.yml")
        .ok_or("missing generated qualification workflow")?;
    let marker = "  composite-hosted:";
    let start = rendered
        .find(marker)
        .ok_or("generated workflow is missing class snapshot marker")?;
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/schema2_class_snapshot.txt");
    let metadata = std::fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("class snapshot path is not a regular file".into());
    }
    let current = std::fs::read_to_string(&path)?;
    if !current.starts_with(marker) {
        return Err("class snapshot does not start at its generated marker".into());
    }
    std::fs::write(path, &rendered[start..])?;
    Ok(())
}
