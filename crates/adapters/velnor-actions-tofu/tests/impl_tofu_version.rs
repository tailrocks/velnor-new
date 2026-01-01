//! `required_version` floor-evaluation cases.
use velnor_actions_tofu_core::version::{
    admits_opentofu, admits_version, is_terraform_only, toolchain_triple,
};

#[test]
fn exact_pins_split_at_the_floor() {
    assert!(is_terraform_only("= 1.5.7"));
    assert!(is_terraform_only("1.5.0"));
    assert!(!is_terraform_only("= 1.6.0"));
    assert!(!is_terraform_only("== 1.13.1"));
    assert!(!is_terraform_only("1.6.0"));
}

#[test]
fn upper_bounds_split_at_the_floor() {
    assert!(is_terraform_only("< 1.6.0"));
    assert!(is_terraform_only("<= 1.5.9"));
    assert!(!is_terraform_only("< 1.6.1"));
    assert!(!is_terraform_only("<= 1.6.0"));
    assert!(!is_terraform_only("< 2.0.0"));
}

#[test]
fn lower_bounds_always_admit() {
    for constraint in [">= 1.0", "> 0.12", ">= 5.0", "> 1.13.1"] {
        assert!(admits_opentofu(constraint), "{constraint}");
    }
}

#[test]
fn pessimistic_bounds_follow_tofu_semantics() {
    assert!(is_terraform_only("~> 1.5.0"));
    assert!(is_terraform_only("~> 0.15"));
    assert!(!is_terraform_only("~> 1.5"));
    assert!(!is_terraform_only("~> 1.6.0"));
    assert!(!is_terraform_only("~> 1.6"));
}

#[test]
fn not_equal_never_excludes_a_range() {
    assert!(admits_opentofu("!= 1.5.0"));
    assert!(admits_opentofu("!= 1.6.0"));
    assert!(admits_opentofu(">= 1.0, != 1.6.0"));
}

#[test]
fn conjunctions_intersect_upper_bounds() {
    assert!(is_terraform_only(">= 1.0, < 1.6.0"));
    assert!(is_terraform_only("< 2.0, <= 1.5.0"));
    assert!(!is_terraform_only(">= 1.0, < 2.0"));
    assert!(admits_opentofu(">= 1.6.0, < 1.13.2"));
}

#[test]
fn contradicting_pins_admit_nothing() {
    assert!(is_terraform_only("= 1.5.0, = 1.5.1"));
    assert!(!is_terraform_only("= 1.6.0, = 1.6.0"));
}

#[test]
fn unparseable_constraints_abstain_toward_admit() {
    for constraint in ["", "banana", ">= ", "= 1.2.3.4", "~> x", ">= 1.0,"] {
        assert!(admits_opentofu(constraint), "{constraint:?}");
    }
}

#[test]
fn prerelease_and_build_metadata_abstain() {
    assert!(admits_opentofu("= 1.6.0-beta1"));
    assert!(admits_opentofu(">= 1.6.0+build"));
}

#[test]
fn whitespace_and_bare_versions_parse() {
    assert!(admits_opentofu("  >=   1.6.0  "));
    assert!(is_terraform_only("  <1.6.0"));
    assert!(!is_terraform_only("1.13"));
    assert!(is_terraform_only("1.5"));
}

#[test]
fn partial_versions_pad_with_zero() {
    assert!(!is_terraform_only(">= 1.6"));
    assert!(is_terraform_only("<= 1.5"));
    assert!(!is_terraform_only("= 2"));
}

#[test]
fn mixed_case_operators_reject() {
    assert!(admits_opentofu("=> 1.6.0"));
    assert!(admits_opentofu("=< 1.5.0"));
}

#[test]
fn tightest_upper_wins() {
    assert!(is_terraform_only("< 1.6.0, < 2.0.0"));
    assert!(!is_terraform_only("<= 1.6.0, < 2.0.0"));
    assert!(is_terraform_only("<= 1.5.9, <= 1.6.0"));
}

#[test]
fn floor_constant_is_first_opentofu_release() {
    assert_eq!(velnor_actions_tofu_core::version::OPENTOFU_FLOOR, (1, 6, 0));
    assert!(!is_terraform_only(">= 1.6.0"));
}

#[test]
fn toolchain_triples_parse_strictly() {
    assert_eq!(toolchain_triple("1.13.1"), Some((1, 13, 1)));
    assert_eq!(toolchain_triple("v1.13.1"), Some((1, 13, 1)));
    assert_eq!(toolchain_triple("1.13"), Some((1, 13, 0)));
    assert_eq!(toolchain_triple("2"), Some((2, 0, 0)));
    for bad in [
        "",
        "latest",
        "1.13.1-beta1",
        "1.13.1+build",
        "1.2.3.4",
        "x.y.z",
    ] {
        assert_eq!(toolchain_triple(bad), None, "{bad:?}");
    }
}

#[test]
fn admits_version_checks_exact_pins() {
    let toolchain = (1, 13, 1);
    assert!(admits_version("= 1.13.1", toolchain));
    assert!(admits_version("1.13.1", toolchain));
    assert!(!admits_version("= 1.13.0", toolchain));
    assert!(!admits_version("= 1.5.7", toolchain));
    assert!(!admits_version("= 1.13.1, = 1.13.0", toolchain));
    assert!(admits_version("= 1.13.1, = 1.13.1", toolchain));
}

#[test]
fn admits_version_honors_bounds() {
    let toolchain = (1, 13, 1);
    assert!(admits_version(">= 1.7.0", toolchain));
    assert!(admits_version("> 1.13.0", toolchain));
    assert!(admits_version("< 2.0.0", toolchain));
    assert!(admits_version("<= 1.13.1", toolchain));
    assert!(!admits_version(">= 1.14.0", toolchain));
    assert!(!admits_version("> 1.13.1", toolchain));
    assert!(!admits_version("< 1.13.1", toolchain));
    assert!(!admits_version("<= 1.13.0", toolchain));
    assert!(!admits_version(">= 1.0, < 1.13.1", toolchain));
    assert!(admits_version(">= 1.0, < 2.0", toolchain));
}

#[test]
fn admits_version_follows_pessimistic_and_not_equal() {
    let toolchain = (1, 13, 1);
    assert!(admits_version("~> 1.13.0", toolchain));
    assert!(admits_version("~> 1.0", toolchain));
    assert!(!admits_version("~> 1.12.0", toolchain));
    assert!(!admits_version("~> 2.0", toolchain));
    assert!(admits_version("!= 1.13.0", toolchain));
    assert!(admits_version("!= 1.13.1", toolchain));
    assert!(admits_version(">= 1.0, != 1.13.1", toolchain));
}

#[test]
fn admits_version_abstains_on_unparseable() {
    let toolchain = (1, 13, 1);
    for constraint in [
        "",
        "banana",
        ">= ",
        "= 1.2.3.4",
        "~> x",
        ">= 1.0,",
        "=> 1.0",
    ] {
        assert!(admits_version(constraint, toolchain), "{constraint:?}");
    }
    assert!(admits_version("= 1.13.1-beta1", toolchain));
}
