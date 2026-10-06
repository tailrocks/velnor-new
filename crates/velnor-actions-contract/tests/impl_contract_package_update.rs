//! Generic package updater declarations cannot inject commands or fixture paths.
use velnor_actions_contract::WorkloadConfig;

#[test]
fn package_update_declares_source_and_closed_artifacts() {
    let base = serde_json::json!({"name":"fixtures","kind":"package_update_fixture","package_update":{
        "updater":"scripts/update.sh","repository":"example/packages",
        "formula":"Formula/tool.rb","preview_formula":"Formula/tool-preview.rb","binary":"tool",
        "artifacts":[{"prefix":"tool","target":"x86_64-linux","archive":"tar_gz","output":"formula","preview":true,"executable":true}]
    }});
    let parse = |value| serde_json::from_value::<WorkloadConfig>(value).expect("typed shape");
    assert!(parse(base.clone()).validate("config.toml").is_ok());
    for (field, hostile) in [
        ("updater", "../outside.sh"),
        ("repository", "$(id)/tool"),
        ("binary", "tool;id"),
        ("formula", "Other/tool.rb"),
    ] {
        let mut value = base.clone();
        value["package_update"][field] = hostile.into();
        assert!(parse(value).validate("config.toml").is_err(), "{field}");
    }
    let mut duplicate = base.clone();
    let artifact = duplicate["package_update"]["artifacts"][0].clone();
    duplicate["package_update"]["artifacts"]
        .as_array_mut()
        .expect("artifacts")
        .push(artifact);
    assert!(parse(duplicate).validate("config.toml").is_err());
    let mut shell = base;
    shell["package_update"]["command"] = "id".into();
    assert!(serde_json::from_value::<WorkloadConfig>(shell).is_err());
}
