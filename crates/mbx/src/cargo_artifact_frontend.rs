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

#[derive(Default, PartialEq, Eq)]
enum MessageFormatKind {
    #[default]
    Unset,
    Json,
    Text,
}

#[derive(Default)]
struct CargoMessageFormat {
    kind: MessageFormatKind,
}

impl CargoMessageFormat {
    fn include(&mut self, formats: &str) -> bool {
        formats.split(',').all(|format| self.include_one(format))
    }

    fn include_one(&mut self, format: &str) -> bool {
        match format.to_ascii_lowercase().as_str() {
            "json" if self.kind == MessageFormatKind::Unset => {
                self.kind = MessageFormatKind::Json;
            }
            "human" | "short" if self.kind == MessageFormatKind::Unset => {
                self.kind = MessageFormatKind::Text;
            }
            "json-diagnostic-short"
            | "json-diagnostic-rendered-ansi"
            | "json-render-diagnostics"
                if self.kind != MessageFormatKind::Text =>
            {
                self.kind = MessageFormatKind::Json;
            }
            _ => return false,
        }
        true
    }

    fn is_json(&self) -> bool {
        self.kind == MessageFormatKind::Json
    }
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
    let mut message_format = CargoMessageFormat::default();
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
            if !message_format.include(format) {
                return CargoFrontend::Unsupported;
            }
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
    if !message_format.is_json() {
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

#[cfg(test)]
mod tests {
    use super::{CargoFrontend, cargo_frontend};
    use std::ffi::OsString;

    fn classify(arguments: &[&str]) -> CargoFrontend {
        cargo_frontend(&arguments.iter().map(OsString::from).collect::<Vec<_>>())
    }

    #[test]
    fn accepts_cargo_json_rendering_modifiers_as_json_frontends() {
        assert_eq!(
            classify(&["build", "--message-format=json-render-diagnostics"]),
            CargoFrontend::Build
        );
        assert_eq!(
            classify(&["check", "--message-format=json-diagnostic-short"]),
            CargoFrontend::Check
        );
        assert_eq!(
            classify(&["build", "--message-format=json-diagnostic-rendered-ansi"]),
            CargoFrontend::Build
        );
        assert_eq!(
            classify(&[
                "build",
                "--message-format=json-diagnostic-short,json-diagnostic-rendered-ansi"
            ]),
            CargoFrontend::Build
        );
        assert_eq!(
            classify(&[
                "clippy",
                "--message-format=json",
                "--message-format=json-render-diagnostics"
            ]),
            CargoFrontend::Clippy
        );
        assert_eq!(
            classify(&["build", "--message-format=JSON-RENDER-DIAGNOSTICS"]),
            CargoFrontend::Build
        );
    }

    #[test]
    fn rejects_invalid_or_conflicting_message_formats() {
        for format in [
            "",
            "json,",
            "json,unknown",
            "json-render-diagnostics,human",
            "json,short",
        ] {
            assert_eq!(
                classify(&["build", "--message-format", format]),
                CargoFrontend::Unsupported,
                "format {format:?}"
            );
        }
        assert_eq!(
            classify(&["build", "--message-format=json", "--message-format=json"]),
            CargoFrontend::Unsupported
        );
        assert_eq!(
            classify(&[
                "build",
                "--message-format=json-render-diagnostics",
                "--message-format=json"
            ]),
            CargoFrontend::Unsupported
        );
    }

    #[test]
    fn test_execution_remains_unsupported_but_no_run_is_observable() {
        assert_eq!(
            classify(&["test", "--message-format=json-render-diagnostics"]),
            CargoFrontend::Unsupported
        );
        assert_eq!(
            classify(&[
                "test",
                "--message-format=json-render-diagnostics",
                "--no-run"
            ]),
            CargoFrontend::TestPreparation
        );
    }
}
