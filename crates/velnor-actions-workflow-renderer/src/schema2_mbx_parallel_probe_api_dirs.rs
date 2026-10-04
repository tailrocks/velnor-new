use velnor_actions_contract::Step;

use crate::RenderError;
use crate::steps;

const PREPARE_RECEIPT_DIRS_NAME: &str = "Prepare private MBX receipt directories";
pub(super) const PREPARE_RECEIPT_DIRS_SCRIPT: &str = concat!(
    "set -euo pipefail; set -o noclobber; umask 077; ",
    "case \"$RUNNER_TEMP\" in /*) ;; *) echo 'invalid runner temp path' >&2; exit 1 ;; esac; ",
    "[ -d \"$RUNNER_TEMP\" ] && [ ! -L \"$RUNNER_TEMP\" ] && [ -O \"$RUNNER_TEMP\" ]; ",
    "identity=\"$RUNNER_TEMP/.mbx-parallel-dir-identity-$$\"; ",
    "[ ! -e \"$identity\" ] && [ ! -L \"$identity\" ]; ",
    "cd -- \"$RUNNER_TEMP\"; pwd -P > \"$identity\"; ",
    "IFS= read -r runner_temp_real < \"$identity\"; rm -f -- \"$identity\"; ",
    "[ \"$runner_temp_real\" = \"$RUNNER_TEMP\" ]; ",
    "input=\"$runner_temp_real/mbx-parallel-input\"; ",
    "for directory in \"$input\" \"$input/seed\" \"$input/reader-a\" ",
    "\"$input/reader-b\" \"$input/new-key-writer\"; do ",
    "if [ -e \"$directory\" ] || [ -L \"$directory\" ]; then ",
    "echo 'MBX receipt directory already exists' >&2; exit 1; fi; ",
    "mkdir -m 700 -- \"$directory\"; ",
    "[ -d \"$directory\" ] && [ ! -L \"$directory\" ] && [ -O \"$directory\" ]; ",
    "cd -- \"$directory\"; pwd -P > \"$identity\"; ",
    "IFS= read -r directory_real < \"$identity\"; rm -f -- \"$identity\"; ",
    "cd -- \"$runner_temp_real\"; [ \"$directory_real\" = \"$directory\" ]; ",
    "stat -c '%a' -- \"$directory\" > \"$identity\"; ",
    "IFS= read -r directory_mode < \"$identity\"; rm -f -- \"$identity\"; ",
    "[ \"$directory_mode\" = 700 ]; done"
);

pub(super) fn prepare_receipt_dirs_step() -> Result<Step, RenderError> {
    steps::shell_step(
        PREPARE_RECEIPT_DIRS_NAME,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            PREPARE_RECEIPT_DIRS_SCRIPT.to_owned(),
        ],
        std::collections::BTreeMap::new(),
    )
}
