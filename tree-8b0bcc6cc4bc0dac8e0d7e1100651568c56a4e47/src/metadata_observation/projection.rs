use super::{MAX_ITEMS, MAX_TEXT, ObservationError, OriginalInvocation, Projection};
use sha2::{Digest, Sha256};

pub(super) fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn project(original: &OriginalInvocation) -> Result<Projection, ObservationError> {
    let mut input = original.argv.iter().map(String::as_str).peekable();
    let selector = input.peek().filter(|arg| arg.starts_with('+')).copied();
    if let Some(selector) = selector {
        if selector.len() < 2 || selector.contains(char::is_whitespace) {
            return Err(ObservationError::UnsupportedInvocation);
        }
        input.next();
    }
    let mut globals = Vec::new();
    let mut options = Vec::new();
    let mut command = None;
    while let Some(argument) = input.next() {
        if argument == "--" {
            if !matches!(
                command,
                Some("test" | "bench" | "run" | "rustc" | "rustdoc")
            ) {
                return Err(ObservationError::UnsupportedInvocation);
            }
            break;
        }
        if argument.starts_with('@') || argument.starts_with('+') {
            return Err(ObservationError::UnsupportedInvocation);
        }
        if !argument.starts_with('-') {
            if command.is_some() || !supported_command(argument) {
                return Err(ObservationError::UnsupportedInvocation);
            }
            command = Some(argument);
            continue;
        }
        flag(argument, &mut input, &mut globals, &mut options)?;
    }
    if command.is_none() {
        return Err(ObservationError::UnsupportedInvocation);
    }
    globals.extend(["metadata".into(), "--format-version".into(), "1".into()]);
    globals.extend(options);
    globals.extend(["--locked".into(), "--offline".into()]);
    if globals.len() > MAX_ITEMS || globals.iter().any(|arg| arg.len() > MAX_TEXT) {
        return Err(ObservationError::Bounds);
    }
    let encoded =
        serde_json::to_vec(&(selector, &globals)).map_err(|_| ObservationError::InvalidRequest)?;
    Ok(Projection {
        toolchain_selector: selector.map(str::to_owned),
        arguments: globals,
        arguments_digest: digest(&encoded),
    })
}

fn supported_command(command: &str) -> bool {
    matches!(
        command,
        "build"
            | "check"
            | "test"
            | "bench"
            | "run"
            | "rustc"
            | "rustdoc"
            | "doc"
            | "clippy"
            | "fix"
    )
}

fn flag<'a>(
    argument: &'a str,
    input: &mut impl Iterator<Item = &'a str>,
    globals: &mut Vec<String>,
    options: &mut Vec<String>,
) -> Result<(), ObservationError> {
    let (name, inline) = argument
        .split_once('=')
        .map_or((argument, None), |(name, value)| (name, Some(value)));
    if let Some((name, value)) = attached_short(argument) {
        return valued(name, value, globals, options);
    }
    match name {
        "--config" | "-Z" | "-C" | "--directory" | "--manifest-path" | "--features" | "-F"
        | "--target" | "-p" | "--package" | "-j" | "--jobs" | "--exclude" | "--bin"
        | "--example" | "--test" | "--bench" | "--profile" | "--target-dir"
        | "--message-format" | "--color" => {
            let value = inline
                .or_else(|| input.next())
                .ok_or(ObservationError::UnsupportedInvocation)?;
            valued(name, value, globals, options)
        }
        "--all-features" | "--no-default-features" if inline.is_none() => {
            options.push(name.into());
            Ok(())
        }
        "--locked"
        | "--offline"
        | "--frozen"
        | "--release"
        | "-r"
        | "--workspace"
        | "--all"
        | "--lib"
        | "--bins"
        | "--examples"
        | "--tests"
        | "--benches"
        | "--all-targets"
        | "--no-run"
        | "--no-fail-fast"
        | "--keep-going"
        | "--no-deps"
        | "--open"
        | "--document-private-items"
        | "--future-incompat-report"
        | "--timings"
        | "--verbose"
        | "-v"
        | "-vv"
        | "--quiet"
        | "-q"
        | "--allow-dirty"
        | "--allow-staged"
        | "--allow-no-vcs"
        | "--broken-code"
        | "--edition"
        | "--edition-idioms"
            if inline.is_none() =>
        {
            Ok(())
        }
        _ => Err(ObservationError::UnsupportedInvocation),
    }
}

fn attached_short(argument: &str) -> Option<(&str, &str)> {
    ["-F", "-Z", "-C", "-p", "-j"].into_iter().find_map(|name| {
        argument
            .strip_prefix(name)
            .filter(|value| !value.is_empty())
            .map(|value| (name, value.strip_prefix('=').unwrap_or(value)))
    })
}

fn valued(
    name: &str,
    value: &str,
    globals: &mut Vec<String>,
    options: &mut Vec<String>,
) -> Result<(), ObservationError> {
    if value.is_empty() || value.len() > MAX_TEXT || value.starts_with('@') {
        return Err(ObservationError::UnsupportedInvocation);
    }
    match name {
        "--config" | "-Z" | "-C" | "--directory" => {
            globals.extend([name.into(), value.into()]);
        }
        "--manifest-path" => options.extend([name.into(), value.into()]),
        "--features" | "-F" => options.extend(["--features".into(), value.into()]),
        "--target" => options.extend(["--filter-platform".into(), value.into()]),
        _ => {}
    }
    Ok(())
}
