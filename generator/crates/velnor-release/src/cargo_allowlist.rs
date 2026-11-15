// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use toml::Value;

enum Node {
    Leaf,
    Open,
    Table(&'static [(&'static str, Node)]),
    Named(&'static Node),
}

const PROFILE_SETTINGS: Node = Node::Table(&[
    ("opt-level", Node::Leaf),
    ("debug", Node::Leaf),
    ("debug-assertions", Node::Leaf),
    ("overflow-checks", Node::Leaf),
    ("lto", Node::Leaf),
    ("panic", Node::Leaf),
    ("incremental", Node::Leaf),
    ("codegen-units", Node::Leaf),
    ("rpath", Node::Leaf),
    ("strip", Node::Leaf),
    ("split-debuginfo", Node::Leaf),
    ("trim-paths", Node::Leaf),
]);

const PROFILE: Node = Node::Table(&[
    ("inherits", Node::Leaf),
    ("opt-level", Node::Leaf),
    ("debug", Node::Leaf),
    ("debug-assertions", Node::Leaf),
    ("overflow-checks", Node::Leaf),
    ("lto", Node::Leaf),
    ("panic", Node::Leaf),
    ("incremental", Node::Leaf),
    ("codegen-units", Node::Leaf),
    ("rpath", Node::Leaf),
    ("strip", Node::Leaf),
    ("split-debuginfo", Node::Leaf),
    ("trim-paths", Node::Leaf),
    ("package", Node::Named(&PROFILE_SETTINGS)),
    ("build-override", PROFILE_SETTINGS),
]);

const REGISTRY_ENTRY: Node =
    Node::Table(&[("index", Node::Leaf), ("credential-provider", Node::Leaf)]);

const SOURCE_ENTRY: Node = Node::Table(&[
    ("replace-with", Node::Leaf),
    ("registry", Node::Leaf),
    ("directory", Node::Leaf),
    ("local-registry", Node::Leaf),
    ("git", Node::Leaf),
    ("branch", Node::Leaf),
    ("tag", Node::Leaf),
    ("rev", Node::Leaf),
]);

const ROOT: Node = Node::Table(&[
    (
        "build",
        Node::Table(&[
            ("target-dir", Node::Leaf),
            ("jobs", Node::Leaf),
            ("target", Node::Leaf),
            ("incremental", Node::Leaf),
        ]),
    ),
    (
        "net",
        Node::Table(&[("retry", Node::Leaf), ("offline", Node::Leaf)]),
    ),
    (
        "http",
        Node::Table(&[
            ("timeout", Node::Leaf),
            ("low-speed-limit", Node::Leaf),
            ("multiplexing", Node::Leaf),
            ("user-agent", Node::Leaf),
            ("debug", Node::Leaf),
            ("check-revoke", Node::Leaf),
            ("proxy", Node::Leaf),
            ("cainfo", Node::Leaf),
        ]),
    ),
    (
        "term",
        Node::Table(&[
            ("verbose", Node::Leaf),
            ("quiet", Node::Leaf),
            ("color", Node::Leaf),
            ("hyperlinks", Node::Leaf),
            ("unicode", Node::Leaf),
            (
                "progress",
                Node::Table(&[
                    ("when", Node::Leaf),
                    ("width", Node::Leaf),
                    ("term-integration", Node::Leaf),
                ]),
            ),
        ]),
    ),
    ("profile", Node::Named(&PROFILE)),
    (
        "registry",
        Node::Table(&[("default", Node::Leaf), ("credential-provider", Node::Leaf)]),
    ),
    ("registries", Node::Named(&REGISTRY_ENTRY)),
    ("source", Node::Named(&SOURCE_ENTRY)),
    ("patch", Node::Open),
    (
        "future-incompat-report",
        Node::Table(&[("frequency", Node::Leaf)]),
    ),
    (
        "resolver",
        Node::Table(&[("incompatible-rust-versions", Node::Leaf)]),
    ),
]);

fn walk(node: &Node, value: &Value, path: &mut Vec<String>) -> Option<String> {
    match node {
        Node::Leaf | Node::Open => None,
        Node::Table(keys) => table(value, path, |key| {
            keys.iter()
                .find(|(name, _)| name == &key)
                .map(|(_, inner)| inner)
        }),
        Node::Named(inner) => table(value, path, |_| Some(inner)),
    }
}

fn table<'a>(
    value: &Value,
    path: &mut Vec<String>,
    lookup: impl Fn(&str) -> Option<&'a Node>,
) -> Option<String> {
    let Value::Table(entries) = value else {
        return Some(path.join("."));
    };
    for (key, inner) in entries {
        path.push(key.clone());
        let Some(node) = lookup(key) else {
            return Some(path.join("."));
        };
        if let Some(found) = walk(node, inner, path) {
            return Some(found);
        }
        path.pop();
    }
    None
}

pub(crate) fn unknown_key(document: &Value) -> Option<String> {
    walk(&ROOT, document, &mut Vec::new())
}
