pub(super) fn assert_scale_set_shell_and_same_steps(hosted: &str, local: &str) {
    assert!(
        local.contains("defaults:\n      run:\n        shell: bash -e {0}"),
        "{local}"
    );
    assert!(!hosted.contains("defaults:"), "{hosted}");
    assert_eq!(tool_lines(hosted), tool_lines(local));
}

fn tool_lines(body: &str) -> Vec<&str> {
    let mut in_steps = false;
    body.lines()
        .filter(|line| {
            let indent = line.bytes().take_while(|byte| *byte == b' ').count();
            let content = line.trim();
            if indent == 4 {
                in_steps = content == "steps:";
                return false;
            }
            in_steps && indent == 8 && (content.starts_with("run:") || content.starts_with("uses:"))
        })
        .collect()
}

#[test]
fn tool_lines_ignores_defaults_but_detects_step_differences() {
    let hosted = "    steps:\n      - name: run\n        run: cargo test\n";
    let scaled = "    defaults:\n      run:\n        shell: bash -e {0}\n    steps:\n      - name: run\n        run: cargo test\n";
    let changed = scaled.replace("cargo test", "cargo check");
    assert_eq!(tool_lines(hosted), tool_lines(scaled));
    assert_ne!(tool_lines(hosted), tool_lines(&changed));
}
