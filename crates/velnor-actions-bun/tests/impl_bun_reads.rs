use velnor_actions_bun::outside_reads;

fn file(path: &str, text: &str) -> (String, String) {
    (path.to_owned(), text.to_owned())
}

#[test]
fn tsconfig_escapes_are_reported_and_inside_paths_are_not() {
    let reads = outside_reads(
        "frontend/packages/ui",
        &[
            file(
                "frontend/packages/ui/tsconfig.json",
                r#"{"extends":"../tsconfig.base.json","compilerOptions":{"paths":{"@/*":["./src/*"],"@x/*":["../../shared/*"]}},
                    "references":[{"path":"../contracts"}],"include":["src","./tests"],}"#,
            ),
            file(
                "frontend/packages/ui/tests/tsconfig.json",
                r#"{"extends":"../tsconfig.json"}"#,
            ),
        ],
    );
    assert_eq!(
        reads,
        [
            "frontend/packages/contracts",
            "frontend/packages/tsconfig.base.json",
            "frontend/shared/*",
        ]
    );
}

#[test]
fn bare_extends_and_workspace_package_configs_are_not_reads() {
    let reads = outside_reads(
        "pkg",
        &[file(
            "pkg/tsconfig.json",
            r#"{"extends":"@chainargos/ui/tsconfig","include":["src"]}"#,
        )],
    );
    assert_eq!(reads, Vec::<String>::new());
}

#[test]
fn script_config_literals_with_parent_segments_are_reads() {
    let reads = outside_reads(
        "frontend/docs",
        &[
            file(
                "frontend/docs/vite.config.ts",
                "export default { server: { fs: { allow: ['../packages/*/src/**', './src'] } }, a: \"..\" }",
            ),
            file("frontend/docs/readme.ts", "const x = '../ignored'"),
        ],
    );
    assert_eq!(reads, ["frontend", "frontend/packages/*/src/**"]);
}

#[test]
fn escaping_the_repository_and_unparsable_configs_are_reported() {
    let reads = outside_reads(
        "pkg",
        &[
            file("pkg/tsconfig.json", r#"{"extends":"../../outside"}"#),
            file("pkg/tsconfig.build.json", "{ broken"),
        ],
    );
    assert_eq!(
        reads,
        [
            "<outside-repository>/../../outside",
            "<unparsable>/pkg/tsconfig.build.json"
        ]
    );
}
