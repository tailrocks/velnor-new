//! Exact SemVer parsing for a Mise configuration's hard minimum.

/// Exact SemVer minimum declared by a Mise configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeMiseVersion {
    core: [u64; 3],
    prerelease: bool,
}

/// Supported hard and soft minimum declarations from Mise config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeMiseMinimum {
    Hard(NativeMiseVersion),
    NoHardMinimum,
}

impl NativeMiseMinimum {
    pub(crate) fn parse(value: &toml::Value) -> Option<Self> {
        if let Some(value) = value.as_str() {
            return NativeMiseVersion::parse(value).map(Self::Hard);
        }
        let table = value.as_table()?;
        if table.is_empty() || table.keys().any(|key| key != "hard" && key != "soft") {
            return None;
        }
        let hard = match table.get("hard") {
            Some(value) => Some(NativeMiseVersion::parse(value.as_str()?)?),
            None => None,
        };
        let soft = match table.get("soft") {
            Some(value) => Some(NativeMiseVersion::parse(value.as_str()?)?),
            None => None,
        };
        if hard.is_none() && soft.is_none() {
            return None;
        }
        Some(hard.map_or(Self::NoHardMinimum, Self::Hard))
    }

    fn hard(self) -> Option<NativeMiseVersion> {
        match self {
            Self::Hard(version) => Some(version),
            Self::NoHardMinimum => None,
        }
    }
}

impl NativeMiseVersion {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let (without_build, build) = match value.split_once('+') {
            Some((version, identifiers)) => (version, Some(identifiers)),
            None => (value, None),
        };
        if build.is_some_and(|identifiers| {
            identifiers.contains('+') || !valid_identifiers(identifiers, true)
        }) {
            return None;
        }
        let (core, prerelease) = match without_build.split_once('-') {
            Some((core, identifiers)) => {
                if !valid_identifiers(identifiers, false) {
                    return None;
                }
                (core, true)
            }
            None => (without_build, false),
        };
        if core.contains('-') || core.contains('+') {
            return None;
        }
        let components = core.split('.').collect::<Vec<_>>();
        let [major, minor, patch] = components.as_slice() else {
            return None;
        };
        let parse_component = |component: &str| {
            if component.is_empty()
                || (component.len() > 1 && component.starts_with('0'))
                || !component.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            component.parse::<u64>().ok()
        };
        Some(Self {
            core: [
                parse_component(major)?,
                parse_component(minor)?,
                parse_component(patch)?,
            ],
            prerelease,
        })
    }

    pub(crate) fn is_not_newer_than(self, other: Self) -> bool {
        match self.core.cmp(&other.core) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Greater => false,
            // The catalog is a stable Mise release. If that ever changes, fail closed
            // for equal-core hard requirements until prerelease precedence is modeled.
            std::cmp::Ordering::Equal => !other.prerelease,
        }
    }
}

fn valid_identifiers(value: &str, allow_leading_zero: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && (allow_leading_zero
                    || !identifier.bytes().all(|byte| byte.is_ascii_digit())
                    || identifier.len() == 1
                    || !identifier.starts_with('0'))
        })
}
