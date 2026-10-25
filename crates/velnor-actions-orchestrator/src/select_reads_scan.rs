use crate::select_reads_lex::{Token, tokenize};

const INCLUDE_MACROS: &[&str] = &["include_str", "include_bytes", "include"];

const FS_CALLS: &[&str] = &[
    "read",
    "read_to_string",
    "read_dir",
    "open",
    "metadata",
    "symlink_metadata",
    "canonicalize",
    "copy",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadTarget {
    Path(String),
    Outside,
    Unbounded(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceRead {
    pub(crate) file: String,
    pub(crate) target: ReadTarget,
    pub(crate) compiled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FileReads {
    pub(crate) reads: Vec<SourceRead>,
    pub(crate) anchored: bool,
    pub(crate) escapes: bool,
}

pub(crate) fn scan_source(file: &str, package_dir: &str, text: &str) -> FileReads {
    let tokens = tokenize(text);
    let file_dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut scan = Scan {
        tokens: &tokens,
        file_dir,
        package_dir,
        targets: Vec::new(),
        anchored: false,
        escapes: false,
    };
    scan.run();
    FileReads {
        reads: scan
            .targets
            .into_iter()
            .map(|(target, compiled)| SourceRead {
                file: file.to_owned(),
                target,
                compiled,
            })
            .collect(),
        anchored: scan.anchored,
        escapes: scan.escapes,
    }
}

enum Concat {
    Read(ReadTarget),
    BuildOutput,
    Unanchored,
}

struct Scan<'a> {
    tokens: &'a [Token],
    file_dir: &'a str,
    package_dir: &'a str,
    targets: Vec<(ReadTarget, bool)>,
    anchored: bool,
    escapes: bool,
}

impl Scan<'_> {
    fn run(&mut self) {
        let mut at = 0;
        let mut attribute_depth = 0usize;
        while let Some(token) = self.tokens.get(at) {
            match token {
                Token::Ident(word)
                    if self.is_macro(at) && INCLUDE_MACROS.contains(&word.as_str()) =>
                {
                    let (args, next) = group(self.tokens, at + 2);
                    if let Some(target) = self.include_target(args) {
                        self.targets.push((target, word == "include"));
                    }
                    at = next;
                    continue;
                }
                Token::Ident(word) if word == "concat" && self.is_macro(at) => {
                    let (args, next) = group(self.tokens, at + 2);
                    if let Concat::Read(target) = self.concat_target(args) {
                        self.anchored = true;
                        self.targets.push((target, false));
                        at = next;
                        continue;
                    }
                }
                Token::Ident(word) if FS_CALLS.contains(&word.as_str()) => self.fs_call(at),
                Token::Ident(word) if word == "path" && attribute_depth > 0 => {
                    self.path_attribute(at);
                }
                Token::Ident(word) if word == "current_dir" => self.anchored = true,
                Token::Ident(word) if word == "parent" || word == "ancestors" => {
                    self.escapes = true;
                }
                Token::Str(body) => self.literal(body),
                Token::Punct('[') if attribute_depth > 0 || self.opens_attribute(at) => {
                    attribute_depth += 1;
                }
                Token::Punct(']') if attribute_depth > 0 => attribute_depth -= 1,
                _ => {}
            }
            at += 1;
        }
    }

    fn is_macro(&self, at: usize) -> bool {
        self.tokens.get(at + 1) == Some(&Token::Punct('!'))
            && self.tokens.get(at + 2) == Some(&Token::Punct('('))
    }

    fn opens_attribute(&self, at: usize) -> bool {
        let before = |back: usize| {
            at.checked_sub(back)
                .and_then(|index| self.tokens.get(index))
        };
        before(1) == Some(&Token::Punct('#'))
            || (before(1) == Some(&Token::Punct('!')) && before(2) == Some(&Token::Punct('#')))
    }

    fn literal(&mut self, body: &str) {
        if body == "CARGO_MANIFEST_DIR" {
            self.anchored = true;
        } else if body.split('/').any(|component| component == "..") {
            self.escapes = true;
        }
    }

    fn include_target(&self, args: &[Token]) -> Option<ReadTarget> {
        match args {
            [Token::Str(body)] => Some(resolve_literal(self.file_dir, body)),
            [
                Token::Ident(word),
                Token::Punct('!'),
                Token::Punct('('),
                inner @ ..,
                Token::Punct(')'),
            ] if word == "concat" => match self.concat_target(inner) {
                Concat::Read(target) => Some(target),
                Concat::BuildOutput => None,
                Concat::Unanchored => Some(ReadTarget::Unbounded("include_argument")),
            },
            _ => Some(ReadTarget::Unbounded("include_argument")),
        }
    }

    fn concat_target(&self, args: &[Token]) -> Concat {
        let parts: Vec<&[Token]> = args.split(|token| *token == Token::Punct(',')).collect();
        let Some((first, rest)) = parts.split_first() else {
            return Concat::Unanchored;
        };
        let base = match env_name(first) {
            Some("CARGO_MANIFEST_DIR") => self.package_dir,
            Some("OUT_DIR") => return Concat::BuildOutput,
            _ => return Concat::Unanchored,
        };
        let mut tail = String::new();
        for part in rest {
            match part {
                [Token::Str(body)] => tail.push_str(body),
                [] => {}
                _ => return Concat::Read(ReadTarget::Unbounded("include_argument")),
            }
        }
        Concat::Read(resolve(base, &tail))
    }

    fn path_attribute(&mut self, at: usize) {
        if self.tokens.get(at + 1) == Some(&Token::Punct('='))
            && let Some(Token::Str(body)) = self.tokens.get(at + 2)
        {
            self.targets
                .push((resolve_literal(self.file_dir, body), true));
        }
    }

    fn fs_call(&mut self, at: usize) {
        if self.tokens.get(at + 1) != Some(&Token::Punct('(')) {
            return;
        }
        if let Some(Token::Str(body)) = self.tokens.get(at + 2)
            && body.split('/').next() == Some("..")
        {
            self.targets.push((resolve(self.package_dir, body), false));
        }
    }
}

fn group(tokens: &[Token], open: usize) -> (&[Token], usize) {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::Punct('(' | '[' | '{') => depth += 1,
            Token::Punct(')' | ']' | '}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return (tokens.get(open + 1..index).unwrap_or_default(), index + 1);
                }
            }
            _ => {}
        }
    }
    (tokens.get(open + 1..).unwrap_or_default(), tokens.len())
}

fn env_name(part: &[Token]) -> Option<&str> {
    match part {
        [
            Token::Ident(word),
            Token::Punct('!'),
            Token::Punct('('),
            Token::Str(name),
            Token::Punct(')'),
        ] if word == "env" => Some(name),
        _ => None,
    }
}

fn resolve_literal(dir: &str, literal: &str) -> ReadTarget {
    if literal.starts_with('/') {
        ReadTarget::Outside
    } else {
        resolve(dir, literal)
    }
}

pub(crate) fn resolve(dir: &str, relative: &str) -> ReadTarget {
    let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for component in relative.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return ReadTarget::Outside;
                }
            }
            other => parts.push(other),
        }
    }
    ReadTarget::Path(parts.join("/"))
}
