// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DockerfileFacts {
    pub external_images: BTreeSet<String>,

    pub context_sources: BTreeSet<String>,

    pub claims_whole_context: bool,

    pub named_contexts: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerfileProblem {
    ImageBaseUnpinned(String),

    ImageBaseUnresolved(String),

    SecretMount,

    RemoteAdd(String),

    ContextEscape(String),

    ContextSourceUnresolved(String),

    EscapeDirective,

    SyntaxUnpinned(String),

    UnsupportedRunnerSyntax(String),
}

impl DockerfileProblem {
    #[must_use]
    pub fn code(&self) -> String {
        match self {
            Self::ImageBaseUnpinned(image) => format!("image_base_unpinned:{image}"),
            Self::ImageBaseUnresolved(image) => format!("image_base_unresolved:{image}"),
            Self::SecretMount => "image_secret_mount".to_owned(),
            Self::RemoteAdd(source) => format!("image_add_remote_source:{source}"),
            Self::ContextEscape(source) => format!("image_context_escape:{source}"),
            Self::ContextSourceUnresolved(source) => {
                format!("image_context_source_unresolved:{source}")
            }
            Self::EscapeDirective => "image_escape_directive".to_owned(),
            Self::SyntaxUnpinned(image) => format!("image_syntax_unpinned:{image}"),
            Self::UnsupportedRunnerSyntax(value) => {
                format!("runner_dockerfile_unsupported:{value}")
            }
        }
    }
}

mod runner;
mod syntax;
mod variable;

pub use runner::analyze_runner_dockerfile;
use syntax::{check_directives, instructions};
use variable::{ArgValue, arg_value, is_injected_arg, substitute, valid_name};

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

pub fn analyze_dockerfile(
    text: &str,
    known_contexts: &[&str],
) -> Result<DockerfileFacts, DockerfileProblem> {
    let mut state = State {
        known_contexts: known_contexts
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        ..State::default()
    };
    check_directives(text)?;
    for (keyword, args) in instructions(text) {
        state.instruction(&keyword, &args)?;
    }
    Ok(state.facts)
}

#[derive(Default)]
struct State {
    facts: DockerfileFacts,
    unknown_environment: bool,
    inherited_stage: bool,
    global_args: BTreeMap<String, ArgValue>,
    unknown_global_args: bool,
    stage_args: BTreeMap<String, ArgValue>,
    stages: BTreeMap<String, usize>,
    triggers: Vec<Vec<(String, String)>>,
    known_contexts: BTreeSet<String>,
}

impl State {
    fn instruction(&mut self, keyword: &str, args: &str) -> Result<(), DockerfileProblem> {
        match keyword {
            "ARG" => self.record_arg(args)?,
            "FROM" => self.from(args)?,
            "ENV" => self.unknown_environment = true,
            "COPY" | "ADD" => self.copy(keyword, args)?,
            "RUN" => self.run(args)?,
            "ONBUILD" => {
                let parsed = instructions(args);
                if parsed.is_empty() {
                    self.facts.claims_whole_context = true;
                }
                if let Some(triggers) = self.triggers.last_mut() {
                    triggers.extend(parsed);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn record_arg(&mut self, args: &str) -> Result<(), DockerfileProblem> {
        let token = args.trim();
        let unresolved = || DockerfileProblem::ContextSourceUnresolved(format!("ARG {args}"));
        let (name, value) = match token.split_once('=') {
            Some((name, value)) => {
                let ambiguous_names = value.chars().any(char::is_whitespace)
                    && !(['\'', '"']
                        .into_iter()
                        .any(|quote| value.starts_with(quote) && value.ends_with(quote)));
                let value = arg_value(value).map_or(ArgValue::Unknown, ArgValue::Known);
                if ambiguous_names {
                    if self.triggers.is_empty() {
                        self.unknown_global_args = true;
                    } else {
                        self.unknown_environment = true;
                    }
                }
                (name, value)
            }
            None if self.inherited_stage => (token, ArgValue::Unknown),
            None => (
                token,
                self.stage_args
                    .get(token)
                    .or_else(|| self.global_args.get(token))
                    .cloned()
                    .unwrap_or(if self.unknown_global_args {
                        ArgValue::Unknown
                    } else {
                        ArgValue::Unset
                    }),
            ),
        };
        if !valid_name(name) {
            return Err(unresolved());
        }
        let value = if is_injected_arg(name) {
            ArgValue::Unknown
        } else {
            value
        };
        if self.triggers.is_empty() {
            self.global_args.insert(name.to_owned(), value);
        } else {
            self.stage_args.insert(name.to_owned(), value);
        }
        Ok(())
    }

    fn from(&mut self, args: &str) -> Result<(), DockerfileProblem> {
        let mut tokens = args.split_whitespace().filter(|t| !t.starts_with("--"));
        let Some(raw) = tokens.next() else {
            return Ok(());
        };
        if self.unknown_global_args && raw.contains('$') {
            return Err(DockerfileProblem::ImageBaseUnresolved(raw.to_owned()));
        }
        let reference = substitute(raw, &self.global_args)
            .ok_or_else(|| DockerfileProblem::ImageBaseUnresolved(raw.to_owned()))?;
        self.stage_args.clear();
        let local = self
            .stages
            .get(&reference.to_ascii_lowercase())
            .copied()
            .or_else(|| {
                reference
                    .parse::<usize>()
                    .ok()
                    .filter(|&index| index < self.triggers.len())
            });
        self.inherited_stage = local.is_some();
        self.unknown_environment = reference != "scratch";
        if reference != "scratch" && local.is_none() {
            require_pinned(&reference)?;
            self.facts.external_images.insert(reference);
            self.facts.claims_whole_context = true;
        }
        if let Some(index) = local {
            for (keyword, args) in self.triggers[index].clone() {
                match keyword.as_str() {
                    "COPY" | "ADD" | "ARG" | "ENV" => match self.instruction(&keyword, &args) {
                        Err(
                            DockerfileProblem::ContextSourceUnresolved(_)
                            | DockerfileProblem::ContextEscape(_),
                        ) => self.facts.claims_whole_context = true,
                        result => result?,
                    },
                    _ => self.facts.claims_whole_context = true,
                }
            }
        }
        if tokens.next().is_some_and(|t| t.eq_ignore_ascii_case("AS"))
            && let Some(name) = tokens.next()
        {
            self.stages
                .insert(name.to_ascii_lowercase(), self.triggers.len());
        }
        self.triggers.push(Vec::new());
        Ok(())
    }

    fn copy(&mut self, keyword: &str, args: &str) -> Result<(), DockerfileProblem> {
        let json_form = args.trim_start().starts_with('[');
        if args.contains('\\') || (!json_form && args.contains(['"', '\''])) {
            return Err(DockerfileProblem::ContextSourceUnresolved(args.to_owned()));
        }
        let mut from_other = false;
        let mut operands = Vec::new();
        for token in split_operands(args)
            .ok_or_else(|| DockerfileProblem::ContextSourceUnresolved(args.to_owned()))?
        {
            if json_form {
                operands.push(token);
            } else if let Some(source) = token.strip_prefix("--from=") {
                from_other = true;
                self.check_from_source(source)?;
            } else if !token.starts_with("--") {
                operands.push(token);
            }
        }
        if from_other {
            return Ok(());
        }
        if operands.len() < 2 {
            self.facts.claims_whole_context = true;
            return Ok(());
        }
        operands.pop();
        for source in operands {
            if source.starts_with("<<") {
                continue;
            }
            if source.contains(['\\', '\'', '"']) {
                return Err(DockerfileProblem::ContextSourceUnresolved(source));
            }
            if self.unknown_environment && source.contains('$') {
                self.facts.claims_whole_context = true;
                continue;
            }
            let Some(source) = substitute(&source, &self.stage_args) else {
                self.facts.claims_whole_context = true;
                continue;
            };
            if !json_form && source.chars().any(char::is_whitespace) {
                return Err(DockerfileProblem::ContextSourceUnresolved(source));
            }
            if keyword == "ADD" && is_remote(&source) {
                return Err(DockerfileProblem::RemoteAdd(source));
            }
            if let Some(relative) = normalize_source(&source) {
                self.facts.context_sources.insert(relative);
            } else {
                self.facts.claims_whole_context = true;
            }
        }
        Ok(())
    }

    fn check_from_source(&mut self, source: &str) -> Result<(), DockerfileProblem> {
        let is_stage = source.bytes().all(|b| b.is_ascii_digit())
            || self.stages.contains_key(&source.to_ascii_lowercase());
        if self.known_contexts.contains(source) {
            self.facts.named_contexts.insert(source.to_owned());
        } else if !is_stage {
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

fn normalize_source(source: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in source.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    })
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

fn split_operands(args: &str) -> Option<Vec<String>> {
    let trimmed = args.trim();
    if trimmed.starts_with('[') {
        return serde_json::from_str(trimmed).ok();
    }
    Some(
        trimmed
            .split_whitespace()
            .map(|item| unquote(item).to_owned())
            .collect(),
    )
}
