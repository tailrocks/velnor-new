use std::fmt;

pub const JAVA_VENDORS: [&str; 5] = [
    "temurin",
    "oracle-graalvm",
    "graalvm-community",
    "corretto",
    "zulu",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionError {
    Vendor,

    Core,

    Build,

    Charset,
}

impl VersionError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        "project_tool_version_not_exact"
    }
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let detail = match self {
            Self::Vendor => "vendor",
            Self::Core => "core",
            Self::Build => "build",
            Self::Charset => "charset",
        };
        write!(f, "{}:{detail}", self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaVersion {
    pub vendor: &'static str,

    pub core: String,

    pub build: Option<String>,

    pub lts: bool,
}

impl JavaVersion {
    #[must_use]
    pub fn major(&self) -> u32 {
        self.core
            .split('.')
            .next()
            .and_then(|part| part.parse().ok())
            .unwrap_or(0)
    }
}

impl fmt::Display for JavaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.vendor, self.core)?;
        if let Some(build) = &self.build {
            write!(f, "+{build}")?;
        }
        if self.lts {
            f.write_str(".LTS")?;
        }
        Ok(())
    }
}

pub fn parse_java(text: &str) -> Result<JavaVersion, VersionError> {
    if text.is_empty()
        || !text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
    {
        return Err(VersionError::Charset);
    }
    let vendor = JAVA_VENDORS
        .iter()
        .find(|vendor| {
            text.strip_prefix(**vendor)
                .is_some_and(|rest| rest.starts_with('-'))
        })
        .ok_or(VersionError::Vendor)?;
    let rest = &text[vendor.len() + 1..];
    let (rest, lts) = match rest.strip_suffix(".LTS") {
        Some(rest) => (rest, true),
        None => (rest, false),
    };
    let (core, build) = match rest.split_once('+') {
        Some((core, build)) => (core, Some(build)),
        None => (rest, None),
    };
    if !is_exact_core(core) {
        return Err(VersionError::Core);
    }
    if let Some(build) = build
        && !build.split('.').all(is_number)
    {
        return Err(VersionError::Build);
    }
    Ok(JavaVersion {
        vendor,
        core: core.to_owned(),
        build: build.map(str::to_owned),
        lts,
    })
}

pub fn parse_gradle(text: &str) -> Result<String, VersionError> {
    let parts: Vec<&str> = text.split('.').collect();
    if matches!(parts.len(), 2 | 3) && parts.iter().all(|part| is_number(part)) {
        Ok(text.to_owned())
    } else {
        Err(VersionError::Core)
    }
}

fn is_number(part: &str) -> bool {
    !part.is_empty()
        && part.chars().all(|c| c.is_ascii_digit())
        && (part == "0" || !part.starts_with('0'))
        && part.len() <= 9
}

fn is_exact_core(core: &str) -> bool {
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() >= 3 && parts.iter().all(|part| is_number(part))
}
