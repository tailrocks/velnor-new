use std::error::Error;

pub(super) fn decode_run_script(step: &str) -> Result<String, Box<dyn Error>> {
    let run_line = step
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .ok_or("source preparation step has no run command")?;
    Ok(serde_json::from_str(run_line)?)
}

pub(super) fn assert_credential_free_exact_source(source: &str) {
    for expected in [
        "GITHUB_REPOSITORY\" == tailrocks/velnor-new",
        "GITHUB_SHA\" =~ ^[0-9a-f]{40}$",
        "rev-parse --verify 'HEAD^{commit}'",
        "== \"$GITHUB_SHA\"",
        "https://github.com/tailrocks/velnor-new.git",
        "--no-recurse-submodules https://github.com/tailrocks/velnor-new.git \"$GITHUB_SHA\"",
        "checkout --quiet --detach FETCH_HEAD",
    ] {
        assert!(source.contains(expected), "missing {expected}: {source}");
    }
    let git_lines = source
        .lines()
        .filter(|line| line.contains("/usr/bin/git"))
        .collect::<Vec<_>>();
    assert_eq!(git_lines.len(), 4, "{source}");
    for git_line in git_lines {
        for guard in [
            "/usr/bin/env -i",
            "PATH=/usr/bin:/bin:/usr/sbin:/sbin",
            "GIT_CONFIG_GLOBAL=/dev/null",
            "GIT_CONFIG_NOSYSTEM=1",
            "GIT_TERMINAL_PROMPT=0",
            "GIT_ALLOW_PROTOCOL=https",
            "credential.helper=",
            "core.hooksPath=/dev/null",
            "protocol.file.allow=never",
            "submodule.recurse=false",
        ] {
            assert!(git_line.contains(guard), "missing {guard}: {git_line}");
        }
    }
}
