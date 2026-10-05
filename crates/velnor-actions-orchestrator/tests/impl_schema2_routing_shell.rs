pub(super) fn assert_scale_set_shell_and_same_steps(
    hosted: &str,
    local: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(
        local.contains("defaults:\n      run:\n        shell: bash -e {0}"),
        "{local}"
    );
    assert!(!hosted.contains("defaults:"), "{hosted}");
    assert_eq!(tool_lines(hosted), tool_lines(local));
    let hosted_identity = named_step(hosted, "Identify Mise cache runtime")?;
    let local_identity = named_step(local, "Identify Mise cache runtime")?;
    for body in [hosted, local] {
        let checkout = position(body, "uses: actions/checkout@")?;
        let identity = position(body, "name: Identify Mise cache runtime")?;
        let restore = position(body, "name: Restore Mise tools")?;
        let shared = position(body, "uses: ./.github/actions/rust-demo")?;
        assert!(checkout < identity && identity < restore && restore < shared);
        assert!(named_step(body, "Identify Mise cache runtime")?.contains("VELNOR_CACHE_LANE"));
        assert!(named_step(body, "Restore Mise tools")?.contains("actions/cache/restore@"));
        if let Some(save) = optional_named_step(body, "Save Mise tools") {
            let save_at = position(body, "name: Save Mise tools")?;
            assert!(shared < save_at && save.contains("actions/cache/save@"));
        }
    }
    assert_ne!(hosted_identity, local_identity);
    Ok(())
}

fn tool_lines(body: &str) -> Vec<&str> {
    let mut in_steps = false;
    let mut lane_specific_cache_step = false;
    body.lines()
        .filter(|line| {
            let indent = line.bytes().take_while(|byte| *byte == b' ').count();
            let content = line.trim();
            if indent == 4 {
                in_steps = content == "steps:";
                lane_specific_cache_step = false;
                return false;
            }
            if in_steps && indent == 6 && content.starts_with("- name:") {
                lane_specific_cache_step = matches!(
                    content,
                    "- name: Identify Mise cache runtime"
                        | "- name: Restore Mise tools"
                        | "- name: Save Mise tools"
                );
                return false;
            }
            in_steps
                && !lane_specific_cache_step
                && indent == 8
                && (content.starts_with("run:") || content.starts_with("uses:"))
        })
        .collect()
}

fn named_step<'a>(body: &'a str, name: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    optional_named_step(body, name).ok_or_else(|| format!("missing {name} step").into())
}

fn optional_named_step<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("- name: {name}");
    let start = body.find(&marker)?;
    let after = &body[start..];
    let end = after[marker.len()..]
        .find("\n      - ")
        .map_or(after.len(), |offset| marker.len() + offset);
    Some(&after[..end])
}

fn position(body: &str, marker: &str) -> Result<usize, Box<dyn std::error::Error>> {
    body.find(marker)
        .ok_or_else(|| format!("missing {marker}").into())
}

#[test]
fn tool_lines_ignores_defaults_but_detects_step_differences() {
    let hosted = "    steps:\n      - name: run\n        run: cargo test\n";
    let scaled = "    defaults:\n      run:\n        shell: bash -e {0}\n    steps:\n      - name: run\n        run: cargo test\n";
    let changed = scaled.replace("cargo test", "cargo check");
    assert_eq!(tool_lines(hosted), tool_lines(scaled));
    assert_ne!(tool_lines(hosted), tool_lines(&changed));
}
