use std::collections::BTreeMap;

/// One parsed emitted resolver step, including its trust boundary.
pub(super) struct ResolverStep {
    pub(super) shell: String,
    pub(super) run: String,
    pub(super) env: BTreeMap<String, String>,
}

/// Parse every generated resolver step's shell, env and run fields.
pub(super) fn resolver_steps(yaml: &str) -> Vec<ResolverStep> {
    let lines = yaml.lines().collect::<Vec<_>>();
    let mut steps = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed != "- name: Resolve project tool versions" {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let mut shell = None;
        let mut run = None;
        let mut env = BTreeMap::new();
        let mut in_env = false;
        for next in &lines[index + 1..] {
            let content = next.trim_start();
            let next_indent = next.len() - content.len();
            if !content.is_empty() && next_indent <= indent {
                break;
            }
            if content.is_empty() {
                continue;
            }
            if next_indent == indent + 2 {
                in_env = content == "env:";
                if let Some(value) = content.strip_prefix("shell: ") {
                    shell = Some(yaml_scalar(value));
                } else if let Some(value) = quoted_value(content, "run: \"") {
                    run = Some(value);
                }
            } else if in_env
                && next_indent == indent + 4
                && let Some((key, value)) = content.split_once(": ")
            {
                env.insert(key.to_owned(), yaml_scalar(value));
            }
        }
        if let (Some(shell), Some(run)) = (shell, run) {
            steps.push(ResolverStep { shell, run, env });
        }
    }
    steps
}

/// Decode the quoted scalar forms emitted by the YAML renderer.
fn yaml_scalar(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(value) = quoted_value(raw, "\"") {
        return value;
    }
    raw.split(" # ").next().unwrap_or(raw).to_owned()
}

fn quoted_value(line: &str, prefix: &str) -> Option<String> {
    let raw = line.strip_prefix(prefix)?;
    let bytes = raw.as_bytes();
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'"' && !escaped {
            return Some(unquote(&raw[..index]));
        }
        if *byte == b'\\' {
            escaped = !escaped;
        } else {
            escaped = false;
        }
    }
    None
}

fn unquote(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}
