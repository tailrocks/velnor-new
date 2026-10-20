//! Static Dockerfile analysis for `[[images]]`: pinned bases and context reads.
//!
//! Pure text parsing, no filesystem or network access. The generated build
//! never passes secrets to `BuildKit`, so a Dockerfile that asks for a secret
//! or SSH mount is rejected here instead of failing late in the build.

use std::collections::{BTreeMap, BTreeSet};

/// Facts extracted from one Dockerfile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DockerfileFacts {
    /// External image references after `ARG` substitution, sorted and unique.
    pub external_images: BTreeSet<String>,
    /// Build-context-relative `COPY`/`ADD` sources, sorted and unique.
    pub context_sources: BTreeSet<String>,
}

/// Machine-readable Dockerfile rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerfileProblem {
    /// An external image reference is not pinned by `@sha256:<64 hex>`.
    ImageBaseUnpinned(String),
    /// A `FROM` reference still names an `ARG` with no default.
    ImageBaseUnresolved(String),
    /// `RUN --mount=type=secret|ssh`, which `BuildKit` may never receive.
    SecretMount,
    /// `ADD` of a remote URL or git source, which is not content-addressed.
    RemoteAdd(String),
    /// A context source that leaves the context with `..`.
    ContextEscape(String),
    /// The `# escape=` directive changes parsing and is not supported.
    EscapeDirective,
    /// An unpinned `# syntax=` frontend image.
    SyntaxUnpinned(String),
}

impl DockerfileProblem {
    /// Stable code for config diagnostics.
    #[must_use]
    pub fn code(&self) -> String {
        match self {
            Self::ImageBaseUnpinned(image) => format!("image_base_unpinned:{image}"),
            Self::ImageBaseUnresolved(image) => format!("image_base_unresolved:{image}"),
            Self::SecretMount => "image_secret_mount".to_owned(),
            Self::RemoteAdd(source) => format!("image_add_remote_source:{source}"),
            Self::ContextEscape(source) => format!("image_context_escape:{source}"),
            Self::EscapeDirective => "image_escape_directive".to_owned(),
            Self::SyntaxUnpinned(image) => format!("image_syntax_unpinned:{image}"),
        }
    }
}

/// True for `name[:tag]@sha256:<64 lowercase hex>`.
#[must_use]
pub fn is_digest_pinned(reference: &str) -> bool {
    reference
        .rsplit_once("@sha256:")
        .is_some_and(|(name, hex)| {
            !name.is_empty()
                && hex.len() == 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

/// Parse a Dockerfile and enforce the pinning and no-secret rules.
/// # Errors
pub fn analyze_dockerfile(text: &str) -> Result<DockerfileFacts, DockerfileProblem> {
    let mut state = State::default();
    check_directives(text)?;
    for (keyword, args) in instructions(text) {
        match keyword.as_str() {
            "ARG" if !state.in_stage => state.record_global_arg(&args),
            "FROM" => state.from(&args)?,
            "COPY" | "ADD" => state.copy(&keyword, &args)?,
            "RUN" => state.run(&args)?,
            _ => {}
        }
    }
    Ok(state.facts)
}

#[derive(Default)]
struct State {
    facts: DockerfileFacts,
    in_stage: bool,
    args: BTreeMap<String, Option<String>>,
    stages: BTreeSet<String>,
    stage_count: usize,
}

impl State {
    fn record_global_arg(&mut self, args: &str) {
        for token in args.split_whitespace() {
            match token.split_once('=') {
                Some((name, value)) => {
                    self.args
                        .insert(name.to_owned(), Some(unquote(value).to_owned()));
                }
                None => {
                    self.args.insert(token.to_owned(), None);
                }
            }
        }
    }

    fn from(&mut self, args: &str) -> Result<(), DockerfileProblem> {
        self.in_stage = true;
        let mut tokens = args.split_whitespace().filter(|t| !t.starts_with("--"));
        let Some(raw) = tokens.next() else {
            return Ok(());
        };
        let reference = self.substitute(raw)?;
        let is_stage = self.stages.contains(&reference.to_ascii_lowercase());
        if reference != "scratch" && !is_stage {
            require_pinned(&reference)?;
            self.facts.external_images.insert(reference);
        }
        if tokens.next().is_some_and(|t| t.eq_ignore_ascii_case("AS"))
            && let Some(name) = tokens.next()
        {
            self.stages.insert(name.to_ascii_lowercase());
        }
        self.stage_count += 1;
        Ok(())
    }

    fn substitute(&self, raw: &str) -> Result<String, DockerfileProblem> {
        let mut out = String::new();
        let mut rest = raw;
        while let Some(at) = rest.find('$') {
            out.push_str(&rest[..at]);
            let tail = &rest[at + 1..];
            let (name, default, consumed) = parse_variable(tail);
            let value = match self.args.get(name) {
                Some(Some(value)) => value.clone(),
                _ => default
                    .map(str::to_owned)
                    .ok_or_else(|| DockerfileProblem::ImageBaseUnresolved(raw.to_owned()))?,
            };
            out.push_str(&value);
            rest = &tail[consumed..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn copy(&mut self, keyword: &str, args: &str) -> Result<(), DockerfileProblem> {
        let mut from_other = false;
        let mut operands = Vec::new();
        for token in split_operands(args) {
            if let Some(source) = token.strip_prefix("--from=") {
                from_other = true;
                self.check_from_source(source)?;
            } else if !token.starts_with("--") {
                operands.push(token);
            }
        }
        if from_other || operands.len() < 2 {
            return Ok(());
        }
        operands.pop();
        for source in operands {
            if source.starts_with("<<") {
                continue;
            }
            if keyword == "ADD" && is_remote(&source) {
                return Err(DockerfileProblem::RemoteAdd(source));
            }
            let relative = source.trim_start_matches("./").trim_start_matches('/');
            if relative.split('/').any(|part| part == "..") {
                return Err(DockerfileProblem::ContextEscape(source));
            }
            self.facts.context_sources.insert(relative.to_owned());
        }
        Ok(())
    }

    fn check_from_source(&mut self, source: &str) -> Result<(), DockerfileProblem> {
        let is_stage = source.bytes().all(|b| b.is_ascii_digit())
            || self.stages.contains(&source.to_ascii_lowercase());
        if !is_stage {
            require_pinned(source)?;
            self.facts.external_images.insert(source.to_owned());
        }
        Ok(())
    }

    fn run(&mut self, args: &str) -> Result<(), DockerfileProblem> {
        for token in args.split_whitespace().take_while(|t| t.starts_with("--")) {
            let Some(mount) = token.strip_prefix("--mount=") else {
                continue;
            };
            for field in mount.split(',') {
                match field.split_once('=') {
                    Some(("type", "secret" | "ssh")) => return Err(DockerfileProblem::SecretMount),
                    Some(("from", source)) => self.check_from_source(source)?,
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn require_pinned(reference: &str) -> Result<(), DockerfileProblem> {
    if is_digest_pinned(reference) {
        Ok(())
    } else {
        Err(DockerfileProblem::ImageBaseUnpinned(reference.to_owned()))
    }
}

fn is_remote(source: &str) -> bool {
    source.contains("://") || source.starts_with("git@")
}

fn unquote(value: &str) -> &str {
    value.trim_matches(|c| c == '"' || c == '\'')
}

/// Split `NAME`, `{NAME}` or `{NAME:-default}` after a `$`.
fn parse_variable(tail: &str) -> (&str, Option<&str>, usize) {
    if let Some(body) = tail.strip_prefix('{')
        && let Some(end) = body.find('}')
    {
        let inner = &body[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (inner, None),
        };
        return (name, default, end + 2);
    }
    let end = tail
        .bytes()
        .position(|b| !(b.is_ascii_alphanumeric() || b == b'_'))
        .unwrap_or(tail.len());
    (&tail[..end], None, end)
}

/// `COPY` operands: JSON-array form or whitespace separated, quotes removed.
fn split_operands(args: &str) -> Vec<String> {
    let trimmed = args.trim();
    if trimmed.starts_with('[')
        && let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
    {
        return inner
            .split(',')
            .map(|item| unquote(item.trim()).to_owned())
            .collect();
    }
    trimmed
        .split_whitespace()
        .map(|item| unquote(item).to_owned())
        .collect()
}

fn check_directives(text: &str) -> Result<(), DockerfileProblem> {
    for line in text.lines() {
        let Some(directive) = line.trim().strip_prefix('#') else {
            break;
        };
        let Some((key, value)) = directive.split_once('=') else {
            break;
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "escape" => return Err(DockerfileProblem::EscapeDirective),
            "syntax" if !is_digest_pinned(value.trim()) => {
                return Err(DockerfileProblem::SyntaxUnpinned(value.trim().to_owned()));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Logical instructions with continuations joined and heredoc bodies skipped.
fn instructions(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let mut logical = line.trim().to_owned();
        if logical.is_empty() || logical.starts_with('#') {
            continue;
        }
        while logical.ends_with('\\') {
            logical.pop();
            let Some(next) = lines.next() else { break };
            if !next.trim().starts_with('#') {
                logical.push(' ');
                logical.push_str(next.trim());
            }
        }
        for word in heredoc_words(&logical) {
            for body in lines.by_ref() {
                if body.trim() == word {
                    break;
                }
            }
        }
        let (keyword, args) = logical
            .split_once(char::is_whitespace)
            .unwrap_or((logical.as_str(), ""));
        out.push((keyword.to_ascii_uppercase(), args.trim().to_owned()));
    }
    out
}

fn heredoc_words(logical: &str) -> Vec<String> {
    logical
        .split_whitespace()
        .filter_map(|token| token.strip_prefix("<<"))
        .map(|word| {
            word.trim_start_matches('-')
                .trim_matches(['"', '\''])
                .to_owned()
        })
        .filter(|word| !word.is_empty())
        .collect()
}
