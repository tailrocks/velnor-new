use super::*;

#[test]
fn isolated_literal_preparation_registry_remains_cold_without_source_capabilities() {
    let configuration = serde_json::json!({"gh_distribution": null});
    let modules = preparation_modules(&configuration).expect("closed preparation modules");
    let initializer = body::initializer(&configuration, &modules).expect("literal source encoding");
    let script = format!(
        "{initializer}\n{LOADER}\nimport receipt_owned_preparation as p\nassert p._qualified_runtime() is p._MISS\nseen=[]\np._run_source=lambda stage: seen.append(stage[0])\np.prepare_owned(tuple((name, (), ()) for name in ('clear','bootstrap','install','warm')))\nassert seen == ['clear','bootstrap','install']\n"
    );
    let directory = tempfile::tempdir().expect("isolated source startup");
    for name in [
        "cache_receipt_common.py",
        "cache_receipt_gh.py",
        "receipt_fresh_gh.py",
        "receipt_owned_preparation.py",
        "sitecustomize.py",
    ] {
        std::fs::write(
            directory.path().join(name),
            "raise RuntimeError('counterfeit_preparation_startup')\n",
        )
        .expect("counterfeit source");
    }
    let output = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-c", &script])
        .current_dir(directory.path())
        .env("PYTHONPATH", directory.path())
        .env("VELNOR_RECEIPT_WARM", "true")
        .output()
        .expect("system isolated Python");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn preparation_source_does_not_forward_ambient_environment_or_catch_postgrant_errors() {
    let source = include_str!("receipt_owned_preparation.py");
    assert!(!source.contains("dict(os.environ)"));
    assert!(source.contains("environment = _runner_environment()"));
    let warm = source
        .split("def _warm(")
        .nth(1)
        .expect("fixed warm continuation")
        .split("def prepare_owned(")
        .next()
        .expect("warm boundary");
    assert!(!warm.contains("except "));
    assert!(!warm.contains("_cold("));
    assert!(
        warm.contains(
            "grant.require_current()\n    namespace.require(grant)\n    _run_source(stage)"
        )
    );
}
