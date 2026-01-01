//! Deterministic emitter, quoting, and indent cases.
use velnor_actions_workflow_tree::{Yaml, quote_scalar, render_yaml};

#[test]
fn yaml_renders_nested_documents_exactly() {
    let doc = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("CI")),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "plan".to_owned(),
                Yaml::Map(vec![
                    ("runs-on".to_owned(), Yaml::str("ubuntu-26.04")),
                    (
                        "steps".to_owned(),
                        Yaml::Seq(vec![Yaml::Map(vec![
                            ("name".to_owned(), Yaml::str("Checkout")),
                            ("uses".to_owned(), Yaml::str("actions/checkout@abc")),
                        ])]),
                    ),
                ]),
            )]),
        ),
    ]);
    let expected = "name: CI\njobs:\n  plan:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: Checkout\n        uses: actions/checkout@abc\n";
    assert_eq!(render_yaml(&doc), expected);
}

#[test]
fn yaml_render_is_byte_stable() {
    let doc = Yaml::Map(vec![
        ("b".to_owned(), Yaml::str("second")),
        (
            "a".to_owned(),
            Yaml::Seq(vec![Yaml::str("x"), Yaml::Int(7)]),
        ),
    ]);
    assert_eq!(render_yaml(&doc), render_yaml(&doc));
}

#[test]
fn yaml_preserves_mapping_order() {
    let doc = Yaml::Map(vec![
        ("zebra".to_owned(), Yaml::Bool(true)),
        ("apple".to_owned(), Yaml::Bool(false)),
    ]);
    assert_eq!(render_yaml(&doc), "zebra: true\napple: false\n");
}

#[test]
fn yaml_quotes_only_when_required() {
    let cases = [
        ("plain", "plain"),
        ("plan", "plan"),
        ("ubuntu-26.04", "ubuntu-26.04"),
        ("a: b", "\"a: b\""),
        ("trailing:", "\"trailing:\""),
        ("a # b", "\"a # b\""),
        ("line\nbreak", "\"line\\nbreak\""),
        ("say \"hi\"", "\"say \\\"hi\\\"\""),
        ("", "\"\""),
        (" leading", "\" leading\""),
        ("trailing ", "\"trailing \""),
        ("true", "\"true\""),
        ("True", "\"True\""),
        ("yes", "\"yes\""),
        ("off", "\"off\""),
        ("on", "\"on\""),
        ("null", "\"null\""),
        ("~", "\"~\""),
        ("8080", "\"8080\""),
        ("1.2", "\"1.2\""),
        ("-3", "\"-3\""),
        ("2026-09-28", "\"2026-09-28\""),
        ("- dash", "\"- dash\""),
        ("{flow}", "\"{flow}\""),
        ("[seq]", "\"[seq]\""),
        ("*alias", "\"*alias\""),
        ("héllo wörld", "héllo wörld"),
        (
            "velnor-${{ github.workflow }}",
            "velnor-${{ github.workflow }}",
        ),
        (
            "${{ github.event_name == 'pull_request' }}",
            "${{ github.event_name == 'pull_request' }}",
        ),
        ("tab\there", "\"tab\\there\""),
        ("C:\\path", "C:\\path"),
    ];
    for (input, expected) in cases {
        assert_eq!(quote_scalar(input), expected, "input: {input:?}");
    }
}

#[test]
fn yaml_quotes_reserved_keys() {
    let doc = Yaml::Map(vec![("on".to_owned(), Yaml::str("x"))]);
    assert_eq!(render_yaml(&doc), "\"on\": x\n");
}

#[test]
fn yaml_null_renders_bare_key() {
    let doc = Yaml::Map(vec![
        ("merge_group".to_owned(), Yaml::Null),
        ("empty_seq".to_owned(), Yaml::Seq(Vec::new())),
        ("empty_map".to_owned(), Yaml::Map(Vec::new())),
    ]);
    assert_eq!(
        render_yaml(&doc),
        "merge_group:\nempty_seq: []\nempty_map: {}\n"
    );
}

#[test]
fn yaml_emits_no_anchors_or_aliases() {
    let doc = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![
            ("a".to_owned(), Yaml::str("same value")),
            ("b".to_owned(), Yaml::str("same value")),
        ]),
    )]);
    let text = render_yaml(&doc);
    assert!(!text.contains('&'));
    assert!(!text.contains('*'));
    assert_eq!(text, "jobs:\n  a: same value\n  b: same value\n");
}
