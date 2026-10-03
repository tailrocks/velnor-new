use serde::Serialize;

const VALUE_OPTIONS: &[&str] = &[
    "--config",
    "--manifest-path",
    "--target",
    "--package",
    "-p",
    "--features",
    "-F",
    "--jobs",
    "-j",
    "--profile",
    "--target-dir",
    "--exclude",
    "--bin",
    "--example",
    "--test",
    "--bench",
    "--color",
];
const FLAGS: &[&str] = &[
    "--workspace",
    "--all",
    "--all-features",
    "--no-default-features",
    "--all-targets",
    "--lib",
    "--bins",
    "--examples",
    "--tests",
    "--benches",
    "--release",
    "-r",
    "--locked",
    "--frozen",
    "--offline",
    "--verbose",
    "-v",
    "-vv",
    "--quiet",
    "-q",
    "--keep-going",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CargoFrontend {
    Build,
    Check,
    Clippy,
    TestPreparation,
    Unsupported,
}

/// Classify actual native argv only; values cannot impersonate selector flags.
pub fn cargo_frontend(arguments: &[std::ffi::OsString]) -> CargoFrontend {
    let words: Option<Vec<&str>> = arguments.iter().map(|word| word.to_str()).collect();
    let Some(words) = words else {
        return CargoFrontend::Unsupported;
    };
    let Some(command) = words.first() else {
        return CargoFrontend::Unsupported;
    };
    let mut options = words[1..].iter();
    let mut json = false;
    let mut no_run = false;
    while let Some(option) = options.next() {
        if *option == "--" {
            break;
        }
        let format = if *option == "--message-format" {
            options.next().copied()
        } else {
            option.strip_prefix("--message-format=")
        };
        if let Some(format) = format {
            if json || !valid_json_format(format) {
                return CargoFrontend::Unsupported;
            }
            json = true;
        } else if VALUE_OPTIONS.contains(option) {
            if options.next().is_none() {
                return CargoFrontend::Unsupported;
            }
        } else if *option == "--no-run" {
            no_run = true;
        } else if inline_value(option) || FLAGS.contains(option) {
            continue;
        } else {
            return CargoFrontend::Unsupported;
        }
    }
    if !json {
        return CargoFrontend::Unsupported;
    }
    match *command {
        "build" => CargoFrontend::Build,
        "check" => CargoFrontend::Check,
        "clippy" => CargoFrontend::Clippy,
        "test" if no_run => CargoFrontend::TestPreparation,
        _ => CargoFrontend::Unsupported,
    }
}

fn inline_value(option: &str) -> bool {
    option
        .split_once('=')
        .is_some_and(|(name, _)| VALUE_OPTIONS.contains(&name))
        || ["-p", "-F", "-j"]
            .iter()
            .any(|prefix| option.starts_with(prefix) && option.len() > prefix.len())
}

fn valid_json_format(format: &str) -> bool {
    format.split(',').any(|value| value == "json")
        && format.split(',').all(|value| {
            matches!(
                value,
                "json"
                    | "json-diagnostic-short"
                    | "json-diagnostic-rendered-ansi"
                    | "json-render-diagnostics"
            )
        })
}
