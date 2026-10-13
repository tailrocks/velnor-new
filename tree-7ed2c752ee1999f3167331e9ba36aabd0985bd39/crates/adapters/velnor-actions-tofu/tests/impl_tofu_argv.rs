//! Tofu payload argv + env shape cases (T13).
use velnor_actions_contract::ContractError;
use velnor_actions_tofu_core::argv::{
    CHDIR_FINDING_TAG, chdir_finding_for_root, tofu_payload_argv,
};
use velnor_actions_tofu_core::env::{
    TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON, TF_INPUT_ENV, TF_INPUT_OFF, tofu_payload_env,
};
use velnor_actions_tofu_core::fmt_scope::fmt_scope_for_root;
use velnor_actions_tofu_core::kinds::TofuTaskKind;
use velnor_actions_tofu_core::propose::{TofuTaskGroup, payload_env_for_kind, propose_task};

use crate::support::{Outcome, TempDir};

/// Payload argv as owned strings for one kind in one root.
fn text(kind: TofuTaskKind, root: &str) -> Result<Vec<String>, ContractError> {
    Ok(tofu_payload_argv(kind, root)?
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect())
}

/// Payload env as owned pairs for one kind.
fn env(kind: TofuTaskKind) -> Vec<(String, String)> {
    tofu_payload_env(kind)
        .into_iter()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

/// All three kinds in fixed obligation order.
const KINDS: [TofuTaskKind; 3] = [
    TofuTaskKind::Fmt,
    TofuTaskKind::InitForValidate,
    TofuTaskKind::Validate,
];

/// Fmt at the repo root is byte-exact (T03 baseline order).
#[test]
fn fmt_root_shape_is_exact() -> Result<(), ContractError> {
    assert_eq!(
        text(TofuTaskKind::Fmt, "")?,
        ["fmt", "-check", "-recursive", "-no-color"]
    );
    Ok(())
}

/// Init at the repo root is byte-exact (readonly lock, no color).
#[test]
fn init_root_shape_is_exact() -> Result<(), ContractError> {
    assert_eq!(
        text(TofuTaskKind::InitForValidate, "")?,
        [
            "init",
            "-backend=false",
            "-input=false",
            "-lockfile=readonly",
            "-no-color"
        ]
    );
    Ok(())
}

/// Validate at the repo root is byte-exact (no color).
#[test]
fn validate_root_shape_is_exact() -> Result<(), ContractError> {
    assert_eq!(text(TofuTaskKind::Validate, "")?, ["validate", "-no-color"]);
    Ok(())
}

/// Subdir roots prefix `-chdir <root>` FIRST; the tail matches the root shape.
#[test]
fn subdir_roots_prefix_chdir_first() -> Result<(), ContractError> {
    for kind in KINDS {
        let root_shape = text(kind, "")?;
        for root in ["stacks/a", "nested/deep/root"] {
            let argv = text(kind, root)?;
            assert_eq!(&argv[..2], ["-chdir", root], "{} {root}", kind.as_str());
            assert_eq!(
                &argv[2..],
                root_shape.as_slice(),
                "{} {root}",
                kind.as_str()
            );
        }
    }
    Ok(())
}

/// Init carries `-lockfile=readonly` exactly once on every root; no
/// other kind emits a lockfile flag, and every kind ends in `-no-color`.
#[test]
fn payloads_pin_readonly_and_no_color() -> Result<(), ContractError> {
    for kind in KINDS {
        for root in ["", "stacks/a"] {
            let argv = text(kind, root)?;
            let readonly = argv
                .iter()
                .filter(|arg| arg.as_str() == "-lockfile=readonly")
                .count();
            assert_eq!(
                readonly,
                usize::from(kind == TofuTaskKind::InitForValidate),
                "{} {root}: {argv:?}",
                kind.as_str()
            );
            assert!(
                !argv
                    .iter()
                    .any(|arg| arg.contains("lockfile") && arg.as_str() != "-lockfile=readonly"),
                "{} {root}: {argv:?}",
                kind.as_str()
            );
            assert_eq!(
                argv.last().map(String::as_str),
                Some("-no-color"),
                "{} {root}: {argv:?}",
                kind.as_str()
            );
        }
    }
    Ok(())
}

/// Argv stays batched tokens: the root rides as one element, never shell-joined.
#[test]
fn payload_tokens_stay_separate() -> Result<(), ContractError> {
    for kind in KINDS {
        let argv = text(kind, "stacks/a")?;
        assert_eq!(argv[1], "stacks/a", "root is one token: {argv:?}");
        assert!(
            !argv.iter().any(|arg| arg.contains(' ')),
            "no joined tokens: {argv:?}"
        );
        assert_eq!(
            tofu_payload_argv(kind, "stacks/a")?.len(),
            argv.len(),
            "typed batch, not a string: {argv:?}"
        );
    }
    Ok(())
}

/// Leading-dash roots fail closed at both the ctor and the proposal layer.
#[test]
fn leading_dash_roots_fail_closed() {
    for kind in KINDS {
        for root in ["-evil", "-evil.tf", "-chdir", "--help"] {
            let err = tofu_payload_argv(kind, root).expect_err("must fail closed");
            assert!(
                err.to_string().contains("leading_dash_root"),
                "{} {root}: {err}",
                kind.as_str()
            );
            let group = TofuTaskGroup {
                root: root.to_owned(),
                kind,
                configuration: "default".to_owned(),
                no_targets: false,
            };
            let err = propose_task(&group).expect_err("proposal must fail closed");
            assert!(
                err.to_string().contains("leading_dash_root"),
                "{} {root}: {err}",
                kind.as_str()
            );
        }
    }
}

/// H6: a `-evil.tf` in scope never enters argv (recursive form, filenames stay out).
#[test]
fn evil_tf_fixture_never_enters_argv() -> Outcome {
    let root = TempDir::create("tofu-argv-evil")?;
    root.write("main.tf", "variable \"x\" {}\n")?;
    root.write("-evil.tf", "variable \"evil\" {}\n")?;
    let paths = vec!["main.tf".to_owned(), "-evil.tf".to_owned()];
    let scope = fmt_scope_for_root(&paths, "");
    assert!(
        scope.contains(&"-evil.tf".to_owned()),
        "fixture must be a live fmt file: {scope:?}"
    );
    for kind in KINDS {
        let argv = text(kind, "")?;
        assert!(
            !argv.iter().any(|arg| arg.contains("evil")),
            "scope filenames stay out of argv: {argv:?}"
        );
    }
    Ok(())
}

/// Every kind carries exactly the T03 automation pair, nothing else.
#[test]
fn env_carries_automation_pair_all_kinds() {
    assert_eq!(TF_IN_AUTOMATION_ENV, "TF_IN_AUTOMATION");
    assert_eq!(TF_IN_AUTOMATION_ON, "1");
    assert_eq!(TF_INPUT_ENV, "TF_INPUT");
    assert_eq!(TF_INPUT_OFF, "0");
    for kind in KINDS {
        assert_eq!(
            env(kind),
            vec![
                ("TF_IN_AUTOMATION".to_owned(), "1".to_owned()),
                ("TF_INPUT".to_owned(), "0".to_owned()),
            ],
            "{}",
            kind.as_str()
        );
    }
}

/// Kind-spelling env seam maps all three kinds; bogus spellings map empty.
#[test]
fn payload_env_for_kind_maps_all_spellings() {
    for spelling in ["fmt", "init", "validate"] {
        assert_eq!(payload_env_for_kind(spelling).len(), 2, "{spelling}");
    }
    assert!(payload_env_for_kind("bogus").is_empty());
    assert!(payload_env_for_kind("plan").is_empty());
}

/// Proposals attach the direct payload bytes plus the automation-pair env.
#[test]
fn propose_attaches_direct_bytes_and_env() -> Result<(), ContractError> {
    for kind in KINDS {
        for root in ["", "stacks/a"] {
            let group = TofuTaskGroup {
                root: root.to_owned(),
                kind,
                configuration: "default".to_owned(),
                no_targets: false,
            };
            let task = propose_task(&group)?;
            assert_eq!(
                task.payload,
                tofu_payload_argv(kind, root)?,
                "payload drift: {} {root}",
                kind.as_str()
            );
            assert_eq!(
                task.identity
                    .environment
                    .get("TF_IN_AUTOMATION")
                    .map(String::as_str),
                Some("1"),
                "env drift: {} {root}",
                kind.as_str()
            );
            assert_eq!(
                task.identity
                    .environment
                    .get("TF_INPUT")
                    .map(String::as_str),
                Some("0"),
                "env drift: {} {root}",
                kind.as_str()
            );
            assert!(
                task.validate().is_ok(),
                "{} {root} validates",
                kind.as_str()
            );
        }
    }
    Ok(())
}

/// The repo root needs no finding: no `-chdir`, identity carries `.`.
#[test]
fn root_uses_identity_instead_of_chdir() -> Result<(), ContractError> {
    for kind in KINDS {
        let argv = text(kind, "")?;
        assert!(
            !argv.contains(&"-chdir".to_owned()),
            "root shape has no chdir: {argv:?}"
        );
    }
    assert_eq!(chdir_finding_for_root(""), None);
    let group = TofuTaskGroup {
        root: String::new(),
        kind: TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = propose_task(&group)?;
    assert_eq!(task.identity.unit_path, ".");
    assert_eq!(task.reads, vec![".".to_owned()]);
    Ok(())
}

/// Subdir roots name the `path.cwd` caveat (S3); the tag spelling is pinned.
#[test]
fn chdir_finding_names_subdir_roots() {
    assert_eq!(CHDIR_FINDING_TAG, "path.cwd");
    assert_eq!(
        chdir_finding_for_root("stacks/a"),
        Some("path.cwd:stacks/a".to_owned())
    );
    assert_eq!(
        chdir_finding_for_root("nested/deep"),
        Some("path.cwd:nested/deep".to_owned())
    );
}

/// `..` segments fail closed for direct callers (proposals qualify
/// first, so this is unreachable via `propose_task`); non-segment
/// dots stay valid.
#[test]
fn argv_rejects_dotdot_segments() {
    for kind in KINDS {
        for root in ["..", "a/../b", "../a"] {
            let err = tofu_payload_argv(kind, root).expect_err("traversal fails");
            assert!(
                err.to_string().contains("traversal_root"),
                "{} {root}: {err}",
                kind.as_str()
            );
        }
        assert!(tofu_payload_argv(kind, "a..b").is_ok());
    }
}
