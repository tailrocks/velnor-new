use super::super::npm_proof;
use super::source_key;
use crate::workloads::cache_eligibility::{NativeNpmSource, valid_source_candidate};
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use velnor_actions_contract::CompiledSourceHelper;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{Yaml, source_helper};

const RUNNERS: [&str; 2] = ["ubuntu-26.04", "macos-26"];
const FIXTURE: &str = r#"
import base64, gzip, hashlib, io, json, os, sys, tarfile

root = sys.argv[1]
os.makedirs(root, exist_ok=True)
sources = []
for index in range(1024):
    name = f"velnor-fixture-{index:04d}"
    version = "1.0.0"
    package = json.dumps({"name": name, "version": version},
                         separators=(",", ":"), sort_keys=True).encode()
    archive = io.BytesIO()
    with tarfile.open(fileobj=archive, mode="w", format=tarfile.USTAR_FORMAT) as tar:
        entry = tarfile.TarInfo("package/package.json")
        entry.size = len(package)
        entry.mode = 0o644
        entry.mtime = 0
        entry.uid = entry.gid = 0
        entry.uname = entry.gname = ""
        tar.addfile(entry, io.BytesIO(package))
    tarball = gzip.compress(archive.getvalue(), compresslevel=9, mtime=0)
    with open(os.path.join(root, name + ".tgz"), "wb") as output:
        output.write(tarball)
    integrity = base64.b64encode(hashlib.sha512(tarball).digest()).decode()
    sources.append({
        "name": name,
        "version": version,
        "resolved": f"https://registry.npmjs.org/{name}/-/{name}-{version}.tgz",
        "integrity": "sha512-" + integrity,
    })
print(json.dumps(sources, separators=(",", ":"), sort_keys=True))
"#;

fn fixture_sources() -> Vec<NativeNpmSource> {
    let root = tempfile::tempdir().expect("fixture root");
    let output = Command::new("/usr/bin/python3")
        .env_clear()
        .args([
            "-I",
            "-c",
            FIXTURE,
            root.path().to_str().expect("fixture path"),
        ])
        .output()
        .expect("python fixture");
    assert!(
        output.status.success(),
        "fixture failed: {:?}",
        output.stderr
    );
    assert_eq!(
        std::fs::read_dir(root.path())
            .expect("fixture archives")
            .count(),
        1024
    );
    let sources: Vec<NativeNpmSource> =
        serde_json::from_slice(&output.stdout).expect("fixture descriptors");
    assert_eq!(sources.len(), 1024);
    assert!(sources.iter().all(valid_source_candidate));
    sources
}

fn yaml_fields(document: Yaml) -> (String, BTreeMap<String, String>) {
    let Yaml::Map(entries) = document else {
        panic!("helper yaml map")
    };
    let run = entries
        .iter()
        .find(|(key, _)| key == "run")
        .and_then(|(_, value)| match value {
            Yaml::Str(value) => Some(value.clone()),
            _ => None,
        })
        .expect("helper run");
    let env = entries
        .iter()
        .find(|(key, _)| key == "env")
        .and_then(|(_, value)| match value {
            Yaml::Map(values) => Some(
                values
                    .iter()
                    .filter_map(|(key, value)| match value {
                        Yaml::Str(value) => Some((key.clone(), value.clone())),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("helper environment");
    (run, env)
}

fn decode_python(encoded: &str) -> Vec<u8> {
    let mut child = Command::new("/usr/bin/python3")
        .env_clear()
        .args([
            "-I",
            "-c",
            "import base64,sys; sys.stdout.buffer.write(base64.b64decode(sys.stdin.buffer.read(), validate=True))",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("base64 decoder");
    child
        .stdin
        .as_mut()
        .expect("decoder stdin")
        .write_all(encoded.as_bytes())
        .expect("decoder input");
    let output = child.wait_with_output().expect("decoder output");
    assert!(output.status.success(), "base64 decoder failed");
    output.stdout
}

fn argument_chunks(environment: &BTreeMap<String, String>) -> Vec<u8> {
    let prefix = "VELNOR_COMPILED_HELPER_ARGUMENTS_";
    let count = environment["VELNOR_COMPILED_HELPER_ARGUMENTS_COUNT"]
        .parse::<usize>()
        .expect("argument chunk count");
    let encoded = (0..count)
        .map(|index| {
            let key = format!("{prefix}{index:04}");
            let value = &environment[&key];
            assert!(value.is_ascii());
            assert!(value.len() <= 8192);
            value.as_str()
        })
        .collect::<String>();
    assert_eq!(
        environment
            .keys()
            .filter(|key| key.starts_with(prefix) && !key.ends_with("COUNT"))
            .count(),
        count
    );
    decode_python(&encoded)
}

fn render_fixture(
    sources: &[NativeNpmSource],
    runner: &str,
) -> (CompiledSourceHelper, String, BTreeMap<String, String>) {
    let catalog = ToolCatalog::pinned();
    let key = source_key(sources, &catalog, runner).expect("source key");
    let host = crate::workloads::host_for_runner(runner).expect("runner host");
    let node = super::node_binary(&catalog, host).expect("qualified node binary");
    let record = npm_proof::source_record(
        sources,
        &catalog,
        host,
        &node,
        &key,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("npm source record");
    source_helper::validate_registry(std::slice::from_ref(&record), env!("CARGO_PKG_VERSION"))
        .expect("registered source record");
    let step = source_helper::source_helper_step(
        "Populate proven npm public source cache",
        &record,
        record.environment().clone(),
    )
    .expect("qualified source step");
    let yaml = source_helper::source_helper_step_to_yaml(
        &step,
        std::slice::from_ref(&record),
        env!("CARGO_PKG_VERSION"),
        runner,
    )
    .expect("rendered source step");
    let (run, environment) = yaml_fields(yaml);
    (record, run, environment)
}

#[test]
fn real_1024_npm_sources_round_trip_through_qualified_transport() {
    let sources = fixture_sources();
    let catalog = ToolCatalog::pinned();
    for runner in RUNNERS {
        let (record, run, environment) = render_fixture(&sources, runner);
        let args = record.invocation().args();
        assert_eq!(args.len(), 1028);
        let host = crate::workloads::host_for_runner(runner).expect("runner host");
        assert_eq!(
            args[0],
            super::node_binary(&catalog, host).expect("qualified node binary")
        );
        assert_eq!(
            args[1],
            source_key(&sources, &catalog, runner).expect("source key")
        );
        assert_eq!(args[2], record.invocation().descriptor().source_sha256());
        assert_eq!(
            args[3],
            serde_json::to_string(&npm_proof::owner_expectation(&catalog)).expect("owner json")
        );
        let mut canonical = sources.clone();
        canonical.sort();
        canonical.dedup();
        let expected_descriptors = canonical
            .iter()
            .map(|source| serde_json::to_string(source).expect("descriptor json"))
            .collect::<Vec<_>>();
        assert_eq!(&args[4..], expected_descriptors.as_slice());
        assert!(run.chars().count() < 21 * 1024);
        assert!(!run.contains(&args[4]));
        assert_eq!(environment["VELNOR_SOURCE_IDENTITY"], args[1]);
        for (key, value) in record.environment() {
            assert_eq!(environment.get(key), Some(value), "authority env {key}");
        }
        let encoded = serde_json::to_vec(args).expect("argument envelope");
        let decoded = argument_chunks(&environment);
        assert_eq!(decoded, encoded);
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(&decoded).expect("decoded args"),
            args
        );
    }
}
