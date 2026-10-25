pub const OPENTOFU_FLOOR: (u32, u32, u32) = (1, 6, 0);

type Triple = (u32, u32, u32);

#[must_use]
pub fn admits_opentofu(constraint: &str) -> bool {
    let mut exacts: Vec<Triple> = Vec::new();
    let mut uppers: Vec<(Triple, bool)> = Vec::new();
    for requirement in constraint.split(',') {
        let (operator, version) = split_operator(requirement.trim());
        let Some(triple) = parse_triple(version) else {
            return true;
        };
        match operator {
            "=" | "==" | "" => exacts.push(triple),
            "<" => uppers.push((triple, true)),
            "<=" => uppers.push((triple, false)),
            "~>" => uppers.push((pessimistic_upper(triple, version), true)),
            "!=" | ">" | ">=" => {}
            _ => return true,
        }
    }
    if exacts.is_empty() && uppers.is_empty() {
        return true;
    }
    if !exacts.is_empty() {
        let first = exacts[0];
        if exacts.iter().any(|exact| *exact != first) {
            return false;
        }
        return first >= OPENTOFU_FLOOR;
    }
    let mut best = uppers[0];
    for upper in uppers.iter().skip(1) {
        if upper.0 < best.0 || (upper.0 == best.0 && upper.1 && !best.1) {
            best = *upper;
        }
    }
    let (version, exclusive) = best;
    if exclusive {
        version > OPENTOFU_FLOOR
    } else {
        version >= OPENTOFU_FLOOR
    }
}

#[must_use]
pub fn is_terraform_only(constraint: &str) -> bool {
    !admits_opentofu(constraint)
}

#[must_use]
pub fn toolchain_triple(text: &str) -> Option<(u32, u32, u32)> {
    parse_triple(text.strip_prefix('v').unwrap_or(text))
}

#[must_use]
pub fn admits_version(constraint: &str, toolchain: (u32, u32, u32)) -> bool {
    for requirement in constraint.split(',') {
        let (operator, version) = split_operator(requirement.trim());
        let Some(triple) = parse_triple(version) else {
            return true;
        };
        if !requirement_holds(operator, version, triple, toolchain) {
            return false;
        }
    }
    true
}

fn requirement_holds(operator: &str, version: &str, triple: Triple, toolchain: Triple) -> bool {
    match operator {
        "=" | "==" | "" => toolchain == triple,
        "<" => toolchain < triple,
        "<=" => toolchain <= triple,
        ">" => toolchain > triple,
        ">=" => toolchain >= triple,
        "~>" => toolchain >= triple && toolchain < pessimistic_upper(triple, version),
        _ => true,
    }
}

fn split_operator(requirement: &str) -> (&str, &str) {
    for operator in ["==", "!=", ">=", "<=", "~>", ">", "<", "="] {
        if let Some(version) = requirement.strip_prefix(operator) {
            return (operator, version.trim());
        }
    }
    ("", requirement)
}

fn parse_triple(version: &str) -> Option<Triple> {
    if version.is_empty() || version.contains(['-', '+']) {
        return None;
    }
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |part| part.parse().ok())?;
    let patch = parts.next().map_or(Some(0), |part| part.parse().ok())?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn pessimistic_upper(triple: Triple, version: &str) -> Triple {
    let width = version.split('.').count();
    if width >= 3 {
        (triple.0, triple.1.saturating_add(1), 0)
    } else {
        (triple.0.saturating_add(1), 0, 0)
    }
}
