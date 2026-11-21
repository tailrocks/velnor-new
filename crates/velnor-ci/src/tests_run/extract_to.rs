// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const OPTION: &str = "--extract-to";

#[must_use]
pub(crate) fn extracts(argv: &[String]) -> bool {
    value_index(argv).is_some()
}

pub(crate) fn retarget_extract(argv: &mut [String], destination: &str) {
    let Some(index) = value_index(argv) else {
        return;
    };
    if argv[index] == OPTION {
        destination.clone_into(&mut argv[index + 1]);
    } else {
        argv[index] = format!("{OPTION}={destination}");
    }
}

fn value_index(argv: &[String]) -> Option<usize> {
    let end = argv
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(argv.len());
    argv[..end].iter().enumerate().find_map(|(index, arg)| {
        let spaced = arg == OPTION && index + 1 < end;
        let joined = arg
            .strip_prefix(OPTION)
            .is_some_and(|rest| rest.starts_with('='));
        (spaced || joined).then_some(index)
    })
}

#[cfg(test)]
mod tests {
    use super::{extracts, retarget_extract};

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn replaces_the_value_after_a_separate_option() {
        let mut list = argv(&["nextest", "list", "--extract-to", ".", "-E", "all()"]);
        assert!(extracts(&list));
        retarget_extract(&mut list, "/tmp/x");
        assert_eq!(
            list,
            argv(&["nextest", "list", "--extract-to", "/tmp/x", "-E", "all()"])
        );
    }

    #[test]
    fn replaces_the_value_of_the_joined_form() {
        let mut list = argv(&["nextest", "list", "--extract-to=.", "-E", "all()"]);
        retarget_extract(&mut list, "/tmp/x");
        assert_eq!(
            list,
            argv(&["nextest", "list", "--extract-to=/tmp/x", "-E", "all()"])
        );
    }

    #[test]
    fn leaves_a_command_without_the_option_alone() {
        let mut list = argv(&["nextest", "list", "--archive-file", "a.tar.zst"]);
        assert!(!extracts(&list));
        retarget_extract(&mut list, "/tmp/x");
        assert_eq!(
            list,
            argv(&["nextest", "list", "--archive-file", "a.tar.zst"])
        );
    }

    #[test]
    fn ignores_the_option_after_the_test_binary_separator() {
        let mut list = argv(&["nextest", "list", "--", "--extract-to", "."]);
        assert!(!extracts(&list));
        retarget_extract(&mut list, "/tmp/x");
        assert_eq!(list, argv(&["nextest", "list", "--", "--extract-to", "."]));
    }

    #[test]
    fn ignores_the_option_with_no_value() {
        let mut list = argv(&["nextest", "list", "--extract-to"]);
        assert!(!extracts(&list));
        retarget_extract(&mut list, "/tmp/x");
        assert_eq!(list, argv(&["nextest", "list", "--extract-to"]));
    }
}
