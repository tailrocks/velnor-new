//! Static `settings.gradle(.kts)` project model.
//!
//! Planning executes no Gradle code, so the project set must be spelled out
//! in literals. Anything that computes projects (loops, directory listings,
//! interpolated names, applied scripts) fails with `gradle_dynamic_settings`
//! instead of being guessed.

use std::collections::BTreeMap;

use crate::lex::{Tok, tokenize};

/// Finding code for a settings script the static model cannot read.
pub const DYNAMIC_SETTINGS: &str = "gradle_dynamic_settings";

/// Identifiers whose presence means projects may be computed.
const DYNAMIC_IDENTS: [&str; 12] = [
    "for",
    "while",
    "forEach",
    "each",
    "eachDir",
    "eachFile",
    "listFiles",
    "walk",
    "walkTopDown",
    "fileTree",
    "includeFlat",
    "apply",
];

/// A statically known project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Gradle path, for example `:eth-domain`.
    pub path: String,
    /// Directory relative to the settings directory (`/`-separated).
    pub dir: String,
}

/// The static model of one settings script.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsModel {
    /// `rootProject.name`, when literal.
    pub root_name: Option<String>,
    /// Included projects (and implied parents), sorted by path.
    pub projects: Vec<Project>,
    /// Literal `includeBuild(...)` directories, sorted.
    pub included_builds: Vec<String>,
}

/// Why the settings script is not statically readable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicSettings {
    /// Short machine-readable reason.
    pub reason: String,
}

impl DynamicSettings {
    /// Stable finding code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        DYNAMIC_SETTINGS
    }
}

fn dynamic<T>(reason: &str) -> Result<T, DynamicSettings> {
    Err(DynamicSettings {
        reason: reason.to_owned(),
    })
}

/// Parse a settings script.
///
/// # Errors
/// [`DynamicSettings`] when any project, directory or included build is not a
/// plain string literal.
pub fn parse(text: &str) -> Result<SettingsModel, DynamicSettings> {
    let toks = tokenize(text);
    let mut paths: Vec<String> = Vec::new();
    let mut dirs: BTreeMap<String, String> = BTreeMap::new();
    let mut model = SettingsModel::default();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Ident(name) if DYNAMIC_IDENTS.contains(&name.as_str()) => {
                return dynamic(&format!("construct:{name}"));
            }
            Tok::Ident(name) if name == "include" => {
                let (args, next) = call_args(&toks, i + 1)?;
                for arg in args {
                    paths.push(normalize_path(&arg));
                }
                i = next;
                continue;
            }
            Tok::Ident(name) if name == "includeBuild" => {
                let (args, next) = call_args(&toks, i + 1)?;
                model.included_builds.extend(args.into_iter().take(1));
                i = next;
                continue;
            }
            Tok::Ident(name) if name == "project" => {
                if let Some((path, dir, next)) = project_dir(&toks, i)? {
                    dirs.insert(normalize_path(&path), dir);
                    i = next;
                    continue;
                }
            }
            Tok::Ident(name) if name == "rootProject" => {
                if let Some(name) = root_name(&toks, i) {
                    model.root_name = Some(name);
                }
            }
            _ => {}
        }
        i += 1;
    }
    model.included_builds.sort();
    model.included_builds.dedup();
    model.projects = expand(&paths, &dirs)?;
    Ok(model)
}

/// `:a:b` and `a:b` both name the project path `:a:b`.
fn normalize_path(raw: &str) -> String {
    if raw.starts_with(':') {
        raw.to_owned()
    } else {
        format!(":{raw}")
    }
}

/// Parse `( "a", "b" )` or the Groovy command form `'a', 'b'` after a call name.
fn call_args(toks: &[Tok], mut i: usize) -> Result<(Vec<String>, usize), DynamicSettings> {
    let parens = matches!(toks.get(i), Some(Tok::Punct('(')));
    if parens {
        i += 1;
    }
    let mut args = Vec::new();
    loop {
        match toks.get(i) {
            Some(Tok::Newline) => {
                if !parens && !matches!(toks.get(i.wrapping_sub(1)), Some(Tok::Punct(','))) {
                    break;
                }
                i += 1;
            }
            Some(Tok::Str(value, false)) => {
                args.push(value.clone());
                i += 1;
            }
            Some(Tok::Str(_, true)) => return dynamic("interpolated_string"),
            Some(Tok::Punct(',')) => i += 1,
            Some(Tok::Punct(')')) if parens => return Ok((args, i + 1)),
            Some(Tok::Punct(_) | Tok::Ident(_)) if parens => {
                return dynamic("non_literal_argument");
            }
            _ => break,
        }
    }
    if parens {
        return dynamic("unterminated_call");
    }
    Ok((args, i))
}

/// Match `project(":x").projectDir = file("dir")`.
fn project_dir(toks: &[Tok], i: usize) -> Result<Option<(String, String, usize)>, DynamicSettings> {
    let (Some(Tok::Punct('(')), Some(Tok::Str(path, false)), Some(Tok::Punct(')'))) =
        (toks.get(i + 1), toks.get(i + 2), toks.get(i + 3))
    else {
        return Ok(None);
    };
    let (Some(Tok::Punct('.')), Some(Tok::Ident(field))) = (toks.get(i + 4), toks.get(i + 5))
    else {
        return Ok(None);
    };
    if field != "projectDir" {
        return Ok(None);
    }
    let mut j = i + 6;
    if matches!(toks.get(j), Some(Tok::Punct('='))) {
        j += 1;
    }
    match (
        toks.get(j),
        toks.get(j + 1),
        toks.get(j + 2),
        toks.get(j + 3),
    ) {
        (
            Some(Tok::Ident(file)),
            Some(Tok::Punct('(')),
            Some(Tok::Str(dir, false)),
            Some(Tok::Punct(')')),
        ) if file == "file" => Ok(Some((path.clone(), dir.clone(), j + 4))),
        _ => dynamic("project_dir_not_literal"),
    }
}

fn root_name(toks: &[Tok], i: usize) -> Option<String> {
    match (
        toks.get(i + 1),
        toks.get(i + 2),
        toks.get(i + 3),
        toks.get(i + 4),
    ) {
        (
            Some(Tok::Punct('.')),
            Some(Tok::Ident(field)),
            Some(Tok::Punct('=')),
            Some(Tok::Str(name, false)),
        ) if field == "name" => Some(name.clone()),
        _ => None,
    }
}

/// Add implied parents and resolve directories.
fn expand(
    paths: &[String],
    dirs: &BTreeMap<String, String>,
) -> Result<Vec<Project>, DynamicSettings> {
    let mut all: BTreeMap<String, String> = BTreeMap::new();
    for path in paths {
        let mut prefix = String::new();
        for segment in path.split(':').filter(|segment| !segment.is_empty()) {
            if segment.contains(['/', '\\']) || segment == ".." || segment == "." {
                return dynamic("invalid_project_segment");
            }
            prefix.push(':');
            prefix.push_str(segment);
            all.entry(prefix.clone())
                .or_insert_with(|| prefix[1..].replace(':', "/"));
        }
    }
    for (path, dir) in dirs {
        let Some(slot) = all.get_mut(path) else {
            return dynamic("project_dir_for_unincluded_project");
        };
        dir.trim_end_matches('/').clone_into(slot);
    }
    Ok(all
        .into_iter()
        .map(|(path, dir)| Project { path, dir })
        .collect())
}
