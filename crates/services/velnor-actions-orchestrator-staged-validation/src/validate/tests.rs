use super::*;

#[test]
fn actionlint_argv_has_single_config_file_first() {
    let workflows = vec![".github/workflows/ci.yml".to_owned()];
    let argv: Vec<String> = actionlint_argv(&workflows)
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv[0..4],
        [
            "-no-color",
            "-oneline",
            "-config-file",
            ".github/actionlint.yaml"
        ]
    );
    assert_eq!(argv[4], workflows[0]);
    assert_eq!(
        argv.iter()
            .filter(|arg| arg.as_str() == "-config-file")
            .count(),
        1
    );
}
