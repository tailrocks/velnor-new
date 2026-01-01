use super::super::{
    KIND_DISPLAY_WORDS, is_clippy_kind, is_nextest_kind, is_workspace_fmt_task,
    payload_env_for_kind, step_base_name, task_kind_rank, tool_needs,
};

/// Dispatch helpers pin every kind spelling and rank.
#[test]
fn dispatch_helpers_pin_spellings() {
    assert_eq!(task_kind_rank("fmt"), 0);
    assert_eq!(task_kind_rank("nextest"), 3);
    assert_eq!(task_kind_rank("bogus"), u32::MAX);
    assert!(is_clippy_kind("clippy"));
    assert!(!is_clippy_kind("test"));
    assert!(is_nextest_kind("nextest"));
    assert!(is_workspace_fmt_task("fmt", "", ""));
    assert!(!is_workspace_fmt_task("fmt", "a", ""));
    assert_eq!(step_base_name("clippy", "Format"), "Clippy");
    assert_eq!(step_base_name("fmt", "Format"), "Format");
    assert_eq!(step_base_name("bogus", "Format"), "bogus");
    assert_eq!(KIND_DISPLAY_WORDS.len(), 7);
    let needs = tool_needs("mbx", "cargo_nextest");
    assert!(needs.mbx && needs.nextest);
    assert!(!tool_needs("cargo", "cargo_test").mbx);
    assert_eq!(payload_env_for_kind("doc").len(), 1);
    assert!(payload_env_for_kind("test").is_empty());
    assert!(payload_env_for_kind("bogus").is_empty());
}
