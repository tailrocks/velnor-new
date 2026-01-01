//! S6 module grammar, lexical resolution, cycles, identity digests.
use std::collections::BTreeSet;

use velnor_actions_tofu_core::modules::{
    ModuleEdge, ModuleError, ModuleFinding, ModuleRef, ModuleSource, RemoteKind, SourceClass,
    check_acyclic, classify_literal, identities_digest, module_edge_pairs, resolve_local_target,
    resolve_refs,
};
use velnor_actions_tofu_core::parser::{ParseError, parse_json, parse_native};

/// One literal module reference from `file`.
fn literal(file: &str, name: &str, source: &str) -> ModuleRef {
    ModuleRef {
        file: file.to_owned(),
        name: name.to_owned(),
        source: ModuleSource::Literal(source.to_owned()),
    }
}

/// One dynamic module reference from `file`.
fn dynamic(file: &str, name: &str, reason: &str) -> ModuleRef {
    ModuleRef {
        file: file.to_owned(),
        name: name.to_owned(),
        source: ModuleSource::Dynamic {
            reason: reason.to_owned(),
        },
    }
}

/// Source of the module named `name` in `text`, or `None`.
fn source_of(text: &str, name: &str) -> Option<ModuleSource> {
    parse_native(text)
        .ok()?
        .modules
        .into_iter()
        .find(|decl| decl.name == name)
        .map(|decl| decl.source)
}

#[test]
fn local_iff_dot_slash_prefix() {
    for source in ["./a", "../a", "./", "../", "./a/b", "../../a", "./../x"] {
        assert_eq!(classify_literal(source), SourceClass::Local, "{source}");
    }
    // Strict S6: bare `.`/`..` never count (OpenTofu requires the slashes).
    for source in ["a", "a/b", "ns/name/sys", ".", ".."] {
        assert_ne!(classify_literal(source), SourceClass::Local, "{source}");
    }
}

#[test]
fn absolute_paths_are_external() {
    for source in ["/abs/path", "\\abs", "C:/win", "C:\\win", "d:/lower"] {
        assert_eq!(classify_literal(source), SourceClass::External, "{source}");
    }
}

#[test]
fn remote_kinds_split_git_registry_other() {
    assert_eq!(
        classify_literal("git::https://example.com/acme/widgets.git?ref=v1.0.0"),
        SourceClass::Remote(RemoteKind::Git)
    );
    assert_eq!(
        classify_literal("github.com/acme/widgets"),
        SourceClass::Remote(RemoteKind::Git)
    );
    assert_eq!(
        classify_literal("example-ns/widgets/fictional"),
        SourceClass::Remote(RemoteKind::Registry)
    );
    assert_eq!(
        classify_literal("registry.example.com/ns/name/system"),
        SourceClass::Remote(RemoteKind::Registry)
    );
    for source in [
        "https://example.com/module.zip",
        "s3::https://example.com/mod.zip",
        "hg::https://example.com/mod",
    ] {
        assert_eq!(
            classify_literal(source),
            SourceClass::Remote(RemoteKind::Other),
            "{source}"
        );
    }
}

#[test]
fn native_module_sources_captured() {
    let text = "module \"local\" {\n  source = \"./mods/a\"\n}\n\
         module \"tpl\" {\n  source = \"./${var.dir}\"\n}\n\
         module \"ref\" {\n  source = var.dir\n}\n\
         module \"bare\" {\n}\n";
    assert!(matches!(
        source_of(text, "local"),
        Some(ModuleSource::Literal(source)) if source == "./mods/a"
    ));
    assert!(matches!(
        source_of(text, "tpl"),
        Some(ModuleSource::Dynamic { reason }) if reason == "template_source"
    ));
    assert!(matches!(
        source_of(text, "ref"),
        Some(ModuleSource::Dynamic { reason }) if reason == "dynamic_source"
    ));
    assert!(matches!(
        source_of(text, "bare"),
        Some(ModuleSource::Dynamic { reason }) if reason == "missing_source"
    ));
}

#[test]
fn native_duplicate_source_is_malformed() {
    let err = parse_native("module \"dup\" {\n  source = \"./a\"\n  source = \"./b\"\n}\n")
        .expect_err("repeats rejected");
    assert!(matches!(err, ParseError::Syntax { .. }), "{err}");
}

#[test]
fn json_module_sources_captured() {
    let model = parse_json(
        "{\"module\": {\"local\": {\"source\": \"../shared\"}, \
         \"num\": {\"source\": 42}, \"bare\": {}}}",
    )
    .expect("parses");
    assert_eq!(model.modules.len(), 3);
    let by_name = |name: &str| {
        model
            .modules
            .iter()
            .find(|decl| decl.name == name)
            .map(|decl| decl.source.clone())
    };
    assert!(matches!(
        by_name("local"),
        Some(ModuleSource::Literal(source)) if source == "../shared"
    ));
    assert!(matches!(
        by_name("num"),
        Some(ModuleSource::Dynamic { reason }) if reason == "dynamic_source"
    ));
    assert!(matches!(
        by_name("bare"),
        Some(ModuleSource::Dynamic { reason }) if reason == "missing_source"
    ));
}

#[test]
fn lexical_targets_resolve_dot_segments() {
    assert_eq!(
        resolve_local_target("", "./mods/a").as_deref(),
        Some("mods/a")
    );
    assert_eq!(
        resolve_local_target("modules/a", "../shared").as_deref(),
        Some("modules/shared")
    );
    assert_eq!(
        resolve_local_target("modules/a", "./nested").as_deref(),
        Some("modules/a/nested")
    );
    assert_eq!(
        resolve_local_target("a/b", "./c/../d/").as_deref(),
        Some("a/b/d")
    );
    assert_eq!(resolve_local_target("a", ".").as_deref(), Some("a"));
}

#[test]
fn lexical_escape_rejected() {
    assert_eq!(resolve_local_target("", ".."), None);
    assert_eq!(resolve_local_target("", "../../x"), None);
    assert_eq!(resolve_local_target("a", "../../x"), None);
    assert_eq!(resolve_local_target("a/b", "../../../x"), None);
    // Exact climb to the root is still in-repo.
    assert_eq!(resolve_local_target("a/b", "../..").as_deref(), Some(""));
}

#[test]
fn resolve_splits_edges_and_findings() {
    let resolved = resolve_refs(&[
        literal("main.tf", "a", "./mods/a"),
        literal("main.tf", "ext", "/opt/copy"),
        literal("main.tf", "reg", "ns/name/sys"),
        dynamic("main.tf", "dyn", "template_source"),
    ])
    .expect("resolves");
    assert_eq!(resolved.edges.len(), 1);
    assert_eq!(resolved.edges[0].from, "");
    assert_eq!(resolved.edges[0].to, "mods/a");
    assert_eq!(resolved.edges[0].source, "./mods/a");
    assert_eq!(resolved.findings.len(), 3);
    let classes: BTreeSet<String> = resolved
        .findings
        .iter()
        .map(|finding| format!("{:?}", finding.class))
        .collect();
    assert!(classes.contains("External"), "{classes:?}");
    assert!(
        classes.iter().any(|class| class.starts_with("Remote")),
        "{classes:?}"
    );
    assert!(classes.contains("Dynamic"), "{classes:?}");
}

#[test]
fn resolve_errors_on_lexical_escape() {
    let err =
        resolve_refs(&[literal("a/main.tf", "out", "../../outside")]).expect_err("escape errors");
    assert!(matches!(err, ModuleError::Escape { .. }), "{err}");
    assert!(err.to_string().contains("outside"), "{err}");
}

#[test]
fn cycles_error_naming_the_path() {
    let edges = vec![
        ModuleEdge {
            from: "a".to_owned(),
            to: "b".to_owned(),
            source: "./b".to_owned(),
        },
        ModuleEdge {
            from: "b".to_owned(),
            to: "a".to_owned(),
            source: "../a".to_owned(),
        },
    ];
    let err = check_acyclic(&edges).expect_err("cycle errors");
    assert!(matches!(err, ModuleError::Cycle { .. }), "{err}");
    let text = err.to_string();
    assert!(text.contains('a') && text.contains('b'), "{text}");
    let slf = vec![ModuleEdge {
        from: "a".to_owned(),
        to: "a".to_owned(),
        source: ".".to_owned(),
    }];
    assert!(check_acyclic(&slf).is_err(), "self loop is a cycle");
    let chain = vec![ModuleEdge {
        from: "a".to_owned(),
        to: "b".to_owned(),
        source: "./b".to_owned(),
    }];
    assert!(check_acyclic(&chain).is_ok(), "chain is acyclic");
}

#[test]
fn edge_pairs_project_like_rust() {
    let edges = vec![ModuleEdge {
        from: "a".to_owned(),
        to: "b".to_owned(),
        source: "./b".to_owned(),
    }];
    assert_eq!(
        module_edge_pairs(&edges),
        vec![("a".to_owned(), "b".to_owned())]
    );
    assert!(module_edge_pairs(&[]).is_empty());
}

#[test]
fn findings_carry_file_and_name() {
    let resolved =
        resolve_refs(&[dynamic("infra/main.tf", "dyn", "missing_source")]).expect("resolves");
    assert_eq!(resolved.findings.len(), 1);
    let finding: &ModuleFinding = &resolved.findings[0];
    assert_eq!(finding.file, "infra/main.tf");
    assert_eq!(finding.name, "dyn");
    assert_eq!(finding.class, SourceClass::Dynamic);
    assert_eq!(finding.detail, "missing_source");
}

#[test]
fn identities_digest_is_stable_and_sensitive() {
    let first = identities_digest(&[("a.tf", "m", "./x", "x", "content-a")]);
    let again = identities_digest(&[("a.tf", "m", "./x", "x", "content-a")]);
    assert_eq!(first, again);
    let moved = identities_digest(&[("a.tf", "m", "./x", "x", "content-b")]);
    assert_ne!(first, moved);
    let empty = identities_digest(&[]);
    assert_ne!(first, empty);
    assert!(!empty.is_empty());
}
