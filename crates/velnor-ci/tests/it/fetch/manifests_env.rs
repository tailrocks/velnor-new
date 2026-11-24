use super::*;

const AWKWARD: &str = "it's a crate/\u{e9}t\u{e9} dir";

fn hex(text: &str) -> String {
    text.bytes()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .filter_map(|nibble| char::from_digit(u32::from(nibble), 16))
        .collect()
}

fn list(entries: &[(&str, &str)]) -> String {
    let items: Vec<String> = entries
        .iter()
        .map(|(path, selected)| format!("[\"{}\",{selected}]", hex(path)))
        .collect();
    format!("[{}]", items.join(","))
}

fn fetch_listed(fixture: &Fixture, api: &Api, entries: &[(&str, &str)]) -> Output {
    let text = list(entries);
    fixture.run_arguments(
        api,
        Some(TOKEN),
        None,
        &["--manifests-env", "VELNOR_FETCH_MANIFESTS"],
        &[("VELNOR_FETCH_MANIFESTS", &text)],
    )
}

#[test]
fn a_listed_manifest_whose_path_is_not_a_plain_word_is_fetched_by_its_exact_path() -> TestResult {
    let fixture = Fixture::new("fetch-listed", 0)?;
    let manifest = format!("{AWKWARD}/Cargo.toml");
    fixture.repo.write_unstaged(&manifest, "");
    let api = serve(204)?;
    let output = fetch_listed(&fixture, &api, &[(&manifest, "true")]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fixture.log("cargo.args").trim(),
        format!(
            "fetch --locked --manifest-path {} --config net.offline=false",
            fixture.manifest(&manifest)?.display()
        )
    );
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}

#[test]
fn a_listed_manifest_marked_false_is_not_fetched_and_the_token_is_still_revoked() -> TestResult {
    let fixture = Fixture::new("fetch-listed-skip", 0)?;
    fixture.repo.write_unstaged("a/Cargo.toml", "");
    fixture.repo.write_unstaged("b/Cargo.toml", "");
    let api = serve(204)?;
    let output = fetch_listed(
        &fixture,
        &api,
        &[("a/Cargo.toml", "true"), ("b/Cargo.toml", "false")],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fixture.log("cargo.args").trim(),
        format!(
            "fetch --locked --manifest-path {} --config net.offline=false",
            fixture.manifest("a/Cargo.toml")?.display()
        )
    );
    let none = Fixture::new("fetch-listed-none", 0)?;
    none.repo.write_unstaged("b/Cargo.toml", "");
    let api = serve(204)?;
    let output = fetch_listed(&none, &api, &[("b/Cargo.toml", "false")]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(none.log("cargo.args"), "");
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}

#[test]
fn a_listed_manifest_that_is_neither_true_nor_false_runs_nothing() -> TestResult {
    let fixture = Fixture::new("fetch-listed-bad", 0)?;
    fixture.repo.write_unstaged("a/Cargo.toml", "");
    let api = serve(204)?;
    let output = fetch_listed(&fixture, &api, &[("a/Cargo.toml", "\"maybe\"")]);
    assert!(!output.status.success());
    assert_eq!(fixture.log("cargo.args"), "");
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}
