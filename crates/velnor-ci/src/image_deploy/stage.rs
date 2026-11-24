// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::elf::{ElfError, ElfFacts, read};

const GLIBC_LOADER: &str = "/lib64/ld-linux-x86-64.so.2";
pub const OPENSSL_SONAMES: [&str; 2] = ["libssl.so.3", "libcrypto.so.3"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Openssl {
    None,
    Sysroot,
    Vendored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Glibc(pub u32, pub u32);

impl Glibc {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (major, minor) = text.split_once('.')?;
        Some(Self(major.parse().ok()?, minor.parse().ok()?))
    }
}

impl std::fmt::Display for Glibc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.0, self.1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub ceiling: Option<Glibc>,
    pub needed: Vec<String>,
}

fn is_openssl(library: &str) -> bool {
    library.starts_with("libssl.so") || library.starts_with("libcrypto.so")
}

fn problems(facts: &ElfFacts, ceiling: Glibc, openssl: Openssl) -> Vec<String> {
    let mut problems = Vec::new();
    if !facts.x86_64 {
        problems.push("it is not an x86-64 binary".to_owned());
    }
    if !facts.dynamic {
        problems.push(
            "it is statically linked, so the image would run it without the glibc it was built for"
                .to_owned(),
        );
    }
    if let Some((major, minor)) = facts.glibc
        && Glibc(major, minor) > ceiling
    {
        problems.push(format!(
            "it needs GLIBC_{major}.{minor}, which is newer than the allowed glibc {ceiling}"
        ));
    }
    if facts
        .interpreter
        .as_deref()
        .is_some_and(|interpreter| interpreter != GLIBC_LOADER)
    {
        problems.push(format!(
            "its program interpreter is {}, but the image runs it with {GLIBC_LOADER}",
            facts.interpreter.as_deref().unwrap_or_default()
        ));
    }
    for path in &facts.library_paths {
        if path.is_empty() {
            problems.push(
                "it carries an empty library search path component, which makes the loader search the working directory"
                    .to_owned(),
            );
        } else if !path.starts_with("$ORIGIN") {
            problems.push(format!(
                "it carries the library search path {path}, which names a place on the build machine"
            ));
        }
    }
    for library in facts.needed.iter().filter(|library| library.contains('/')) {
        problems.push(format!(
            "it needs {library}, a path on the build machine, so the loader looks for that exact path in the image"
        ));
    }
    for library in facts.needed.iter().filter(|library| is_openssl(library)) {
        match openssl {
            Openssl::Sysroot if OPENSSL_SONAMES.contains(&library.as_str()) => {}
            Openssl::Sysroot => problems.push(format!(
                "it needs {library}, but the Debian trixie sysroot provides only {} and {}",
                OPENSSL_SONAMES[0], OPENSSL_SONAMES[1]
            )),
            Openssl::None | Openssl::Vendored => problems.push(format!(
                "it needs {library}, but openssl is not \"sysroot\" for this image; vendor OpenSSL or set openssl = \"sysroot\""
            )),
        }
    }
    problems
}

pub fn check(bytes: &[u8], ceiling: Glibc, openssl: Openssl) -> Result<Checked, Vec<String>> {
    let facts = read(bytes).map_err(|error: ElfError| vec![error.to_string()])?;
    let problems = problems(&facts, ceiling, openssl);
    if problems.is_empty() {
        Ok(Checked {
            ceiling: facts.glibc.map(|(major, minor)| Glibc(major, minor)),
            needed: facts.needed,
        })
    } else {
        Err(problems)
    }
}
