// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::super::unit;
use crate::io::Repo;

pub(super) const PIN: &str = "0.1.117";

const BUILD_SCRIPT: &str = "fn main() {\n    let out = std::env::var(\"OUT_DIR\").unwrap();\n    let text = std::fs::read_to_string(\"../../docs/NAME.md\").unwrap();\n    std::fs::write(format!(\"{out}/o.txt\"), text).unwrap();\nBODY}\n";

pub(super) const UNDECLARED: [(&str, &str); 6] = [
    (
        "envdecl",
        "    println!(\"cargo:rerun-if-env-changed=VELNOR_UNUSED\");\n",
    ),
    ("none", ""),
    (
        "cond",
        "    if std::env::var(\"NEVER_SET\").is_ok() {\n        println!(\"cargo:rerun-if-changed=build.rs\");\n    }\n",
    ),
    (
        "tofile",
        "    std::fs::write(format!(\"{out}/log\"), \"cargo:rerun-if-changed=build.rs\").unwrap();\n",
    ),
    (
        "early",
        "    if !out.is_empty() {\n        return;\n    }\n    println!(\"cargo:rerun-if-changed=build.rs\");\n",
    ),
    (
        "shadow",
        "    macro_rules! println { ($($t:tt)*) => {}; }\n    println!(\"cargo:rerun-if-changed=build.rs\");\n",
    ),
];

const OVERRIDE: &str = "fn main() {\n    let docs = std::fs::canonicalize(\"../../docs\").unwrap();\n    println!(\"KEY=CARGO_MANIFEST_DIR={}\", docs.display());\n    println!(\"cargo:rerun-if-changed=build.rs\");\n}\n";

const MEMBERS: [&str; 21] = [
    "helper", "user", "pm", "puser", "gen", "none", "cond", "tofile", "early", "shadow", "envdecl",
    "kept", "ovr", "ovr2", "outenv", "ext", "euser", "muser", "asstd", "root", "nested",
];

fn package(repo: &Repo, name: &str, extra: &str, lib: &str) {
    let manifest = format!("[package]\nname = \"{name}\"\nedition = \"2024\"\n{extra}");
    unit(repo, &format!("crates/{name}/Cargo.toml"), &manifest);
    unit(repo, &format!("crates/{name}/src/lib.rs"), lib);
}

fn out_dir_lib() -> &'static str {
    "pub const O: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/o.txt\"));\n"
}

fn rustc_env(escaped: bool) -> String {
    let key = if escaped {
        "rustc-\\u{65}nv"
    } else {
        "rustc-env"
    };
    OVERRIDE.replace("KEY", &format!("cargo:{key}"))
}

fn dependency(name: &str) -> String {
    format!("\n[dependencies]\n{name} = {{ path = \"../{name}\" }}\n")
}

pub(super) fn fixture() -> Repo {
    let repo = workspace();
    macros(&repo);
    build_scripts(&repo);
    plain_and_shadowed(&repo);
    repo.git(&["add", "--all"]);
    repo
}

pub(super) fn pinned_lock(repo: &Repo) {
    let committed = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.velnor/rust-ci.lock"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let version = committed
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| unreachable!("{committed}"));
    let lock = committed.replacen(version, PIN, 1);
    assert!(lock.starts_with(&format!("version = \"{PIN}\"")), "{lock}");
    unit(repo, ".velnor/rust-ci.lock", &lock);
}

fn workspace() -> Repo {
    let repo = Repo::new("audit-includes");
    unit(&repo, "mise.toml", &crate::toolchain::mise_rust_pin());
    let members: Vec<String> = MEMBERS.iter().map(|m| format!("\"crates/{m}\"")).collect();
    unit(
        &repo,
        "Cargo.toml",
        &format!(
            "[workspace]\nmembers = [{}]\nresolver = \"3\"\n",
            members.join(", ")
        ),
    );
    unit(&repo, "Cargo.lock", "version = 4\n");
    pinned_lock(&repo);
    repo
}

fn macros(repo: &Repo) {
    package(
        repo,
        "helper",
        "",
        "#[macro_export]\nmacro_rules! manifest { () => { include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../docs/macro.md\")) } }\n#[macro_export]\nmacro_rules! near { () => { include_str!(\"../../../docs/near.md\") } }\n#[macro_export]\nmacro_rules! inc { () => { include!(\"inc.rs\"); } }\n",
    );
    package(
        repo,
        "user",
        &dependency("helper"),
        "mod deep;\nmod inc_mod { helper::inc!(); }\npub const M: &str = helper::manifest!();\n",
    );
    unit(
        repo,
        "crates/user/src/deep/mod.rs",
        "pub const N: &str = helper::near!();\n",
    );
    unit(repo, "crates/user/src/inc.rs", "pub const I: u8 = 1;\n");
    package(
        repo,
        "pm",
        "\n[lib]\nproc-macro = true\n",
        "extern crate proc_macro;\n#[proc_macro]\npub fn embed(_: proc_macro::TokenStream) -> proc_macro::TokenStream {\n    \"include_str!(concat!(env!(\\\"CARGO_MANIFEST_DIR\\\"), \\\"/../../docs/pm.md\\\"))\".parse().unwrap()\n}\n",
    );
    package(
        repo,
        "puser",
        &dependency("pm"),
        "pub const P: &str = pm::embed!();\n",
    );
    package(
        repo,
        "gen",
        "build = \"build.rs\"\n",
        "pub const G: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/generated.txt\"));\n",
    );
    unit(
        repo,
        "crates/gen/build.rs",
        "fn main() {\n    let text = std::fs::read_to_string(\"../../docs/gen.md\").unwrap();\n    std::fs::write(\"generated.txt\", text).unwrap();\n    println!(\"cargo:rerun-if-changed=../../docs/gen.md\");\n}\n",
    );
}

fn build_scripts(repo: &Repo) {
    for (name, body) in UNDECLARED {
        package(repo, name, "build = \"build.rs\"\n", out_dir_lib());
        let script = BUILD_SCRIPT.replace("NAME", name).replace("BODY", body);
        unit(repo, &format!("crates/{name}/build.rs"), &script);
        unit(repo, &format!("docs/{name}.md"), "");
    }
    package(repo, "kept", "build = \"build.rs\"\n", out_dir_lib());
    let kept = BUILD_SCRIPT.replace("NAME", "kept").replace(
        "BODY",
        "    println!(\"cargo:rerun-if-changed=../../docs/kept.md\");\n",
    );
    unit(repo, "crates/kept/build.rs", &kept);
    unit(repo, "docs/kept.md", "");
    for (name, escaped) in [("ovr", false), ("ovr2", true)] {
        let lib = "pub const V: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/NAME.md\"));\n";
        package(
            repo,
            name,
            "build = \"build.rs\"\n",
            &lib.replace("NAME", name),
        );
        unit(
            repo,
            &format!("crates/{name}/build.rs"),
            &rustc_env(escaped),
        );
        unit(repo, &format!("docs/{name}.md"), "");
    }
    package(
        repo,
        "outenv",
        "build = \"build.rs\"\n",
        "pub const V: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/outenv.md\"));\n",
    );
    unit(
        repo,
        "crates/outenv/build.rs",
        &rustc_env(false).replace("CARGO_MANIFEST_DIR", "OUT_DIR"),
    );
    unit(repo, "docs/outenv.md", "");
}

fn plain_and_shadowed(repo: &Repo) {
    package(
        repo,
        "ext",
        "",
        "#[macro_export]\nmacro_rules! include_str { ($p:literal) => { ::std::include_str!(\"../../../docs/ext.md\") } }\n",
    );
    package(
        repo,
        "euser",
        &dependency("ext"),
        "pub const E: &str = ext::include_str!(\"x\");\n",
    );
    package(
        repo,
        "muser",
        &dependency("ext"),
        "#[macro_use]\nextern crate ext;\npub const M: &str = include_str!(\"x\");\n",
    );
    package(
        repo,
        "asstd",
        &dependency("ext"),
        "mod shadowed {\n    use ext as std;\n    pub const A: &str = std::include_str!(\"x\");\n}\n",
    );
    for file in ["macro", "near", "pm", "gen", "ext"] {
        unit(repo, &format!("docs/{file}.md"), "");
    }
    unit(repo, "crates/docs/near.md", "");
    package(
        repo,
        "root",
        "",
        "pub const R: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../docs/root.md\"));\n",
    );
    package(
        repo,
        "nested",
        "",
        "pub const N: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/n.md\"));\n",
    );
    unit(repo, "docs/root.md", "");
    unit(repo, "crates/nested/src/n.md", "");
}
