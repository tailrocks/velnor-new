//! Inline shell recognition and outer-shell script quoting.

pub(crate) fn inline_script_index(argv: &[String]) -> Option<usize> {
    match argv {
        [shell, flag, _, ..] if matches!(shell.as_str(), "sh" | "bash") && flag == "-c" => Some(2),
        [shell, privilege, flag, _, ..]
            if shell == "/bin/bash" && privilege == "-p" && flag == "-c" =>
        {
            Some(3)
        }
        _ => None,
    }
}

pub use velnor_actions_contract::quote_literal_run_arg;
