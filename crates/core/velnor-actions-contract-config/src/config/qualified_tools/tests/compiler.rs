use super::super::*;
use super::{bun, nextest_tool, rust_toolchain};

#[test]
fn compiler_requirements_follow_probe_facts_across_backends() {
    assert!(!bun().requires_compiler());
    assert!(rust_toolchain().requires_compiler());
    for prebuilt in [false, true] {
        let mut nextest = nextest_tool(prebuilt);
        assert!(nextest.requires_compiler());
        let error = validate_qualified_tools(&[nextest.clone()], "config")
            .expect_err("nextest probe compiler");
        assert!(
            error
                .to_string()
                .contains("requires_one_direct_rust_dependency")
        );
        nextest.depends_on = vec!["rust".to_owned()];
        assert!(validate_qualified_tools(&[nextest.clone(), rust_toolchain()], "config").is_ok());
        let mut other = rust_toolchain();
        other.id = "rust-other".to_owned();
        nextest.depends_on.push("rust-other".to_owned());
        assert!(validate_qualified_tools(&[nextest, rust_toolchain(), other], "config").is_err());
    }
}
