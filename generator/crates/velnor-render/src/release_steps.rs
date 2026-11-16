// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperVersion, RELEASE_PLZ_FILE};
use velnor_release::{ReleasePlan, RootPlan};

use crate::rules::{APP_TOKEN_ACTION, RenderedStep, Value};
use crate::steps::{CHECKOUT, MISE, MISE_VERSION};

pub(crate) const TAG_PATTERN_HELPER: (u64, u64, u64) = (0, 1, 80);
pub(crate) const RELEASED_VERSIONS_HELPER: (u64, u64, u64) = (0, 1, 81);
pub(crate) const TAGGED_COMMIT_HELPER: (u64, u64, u64) = (0, 1, 81);
pub(crate) const COMMIT_OUTPUT_SUFFIX: &str = "_commit";
const APP_TOKEN_COMMIT: &str = "64ab729a6294000f5aa4a4bc0b73d1f32bc45510";
const APP_TOKEN_VERSION: &str = "v3.3.0";
pub(crate) const APP_TOKEN_OUTPUT: &str = "${{ steps.app.outputs.token }}";

pub(crate) fn expression(condition: &str) -> String {
    format!("${{{{ {condition} }}}}")
}

pub(crate) fn permissions(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(scope, access)| ((*scope).to_owned(), (*access).to_owned()))
        .collect()
}

pub(crate) fn variables(items: &[(&str, &str)]) -> Vec<(String, Value)> {
    items
        .iter()
        .map(|(key, value)| ((*key).to_owned(), Value::Expr((*value).to_owned())))
        .collect()
}

pub(crate) fn checkout(reference: &str, depth: u64) -> RenderedStep {
    RenderedStep {
        name: "Check out".to_owned(),
        uses: Some(CHECKOUT.to_owned()),
        with: vec![
            ("ref".to_owned(), Value::Expr(reference.to_owned())),
            ("fetch-depth".to_owned(), Value::Int(depth)),
            ("persist-credentials".to_owned(), Value::Bool(false)),
        ],
        ..RenderedStep::default()
    }
}

pub(crate) fn install_tool(tool: &str) -> RenderedStep {
    RenderedStep {
        name: format!("Install {tool}"),
        uses: Some(MISE.to_owned()),
        with: vec![
            ("version".to_owned(), Value::Text(MISE_VERSION.to_owned())),
            ("install_args".to_owned(), Value::Text(tool.to_owned())),
            ("cache".to_owned(), Value::Bool(false)),
        ],
        ..RenderedStep::default()
    }
}

pub(crate) const TOOLCHAIN: &str = "stable";

fn run_step(name: &str, run: String, env: Vec<(String, Value)>) -> RenderedStep {
    RenderedStep {
        name: name.to_owned(),
        env,
        run: Some(run),
        ..RenderedStep::default()
    }
}

pub(crate) const RELEASE_PLZ_BIN: &str = "\"$RUNNER_TEMP/release-plz\"";
const RELEASE_PLZ_ARCHIVE: &str = "\"$RUNNER_TEMP/release-plz.tar.gz\"";

pub(crate) fn install_release_plz(plan: &ReleasePlan) -> Vec<RenderedStep> {
    vec![
        run_step(
            "Download release-plz",
            format!(
                "curl --fail --silent --show-error --location --output {RELEASE_PLZ_ARCHIVE} {}",
                shell_word(&plan.release_plz_url)
            ),
            Vec::new(),
        ),
        run_step(
            "Write the release-plz digest",
            format!(
                "printf '%s  %s/release-plz.tar.gz\\n' {} \"$RUNNER_TEMP\" > \"$RUNNER_TEMP/release-plz.sha256\"",
                plan.release_plz_sha256
            ),
            Vec::new(),
        ),
        run_step(
            "Verify the release-plz digest",
            "sha256sum --check \"$RUNNER_TEMP/release-plz.sha256\"".to_owned(),
            Vec::new(),
        ),
        run_step(
            "Unpack release-plz",
            format!(
                "tar --extract --gzip --file {RELEASE_PLZ_ARCHIVE} --directory \"$RUNNER_TEMP\" release-plz"
            ),
            Vec::new(),
        ),
        run_step(
            "Install the Rust toolchain",
            format!("rustup toolchain install {TOOLCHAIN} --profile minimal --no-self-update"),
            Vec::new(),
        ),
    ]
}

const NO_PROGRAM_VARIABLES: [(&str, &str); 6] = [
    ("RUSTC_WRAPPER", ""),
    ("RUSTC_WORKSPACE_WRAPPER", ""),
    ("CARGO_BUILD_RUSTC_WRAPPER", ""),
    ("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER", ""),
    ("CARGO_BUILD_RUSTC", "rustc"),
    ("RUSTC", "rustc"),
];

pub(crate) fn app_token(scopes: &[(&str, &str)]) -> RenderedStep {
    let mut with = variables(&[
        ("client-id", "${{ secrets.RELEASE_APP_ID }}"),
        ("private-key", "${{ secrets.RELEASE_APP_PRIVATE_KEY }}"),
        ("owner", "${{ github.repository_owner }}"),
        ("repositories", "${{ github.event.repository.name }}"),
    ]);
    with.extend(scopes.iter().map(|(scope, access)| {
        (
            format!("permission-{scope}"),
            Value::Text((*access).to_owned()),
        )
    }));
    RenderedStep {
        id: Some("app".to_owned()),
        name: "Create the release App token".to_owned(),
        uses: Some(format!("{APP_TOKEN_ACTION}@{APP_TOKEN_COMMIT}")),
        uses_comment: Some(APP_TOKEN_VERSION.to_owned()),
        with,
        ..RenderedStep::default()
    }
}

pub(crate) fn refuse_commands(
    roots: &[RootPlan],
    helper: &HelperVersion,
    tags_released_version: bool,
) -> RenderedStep {
    let files: Vec<String> = roots
        .iter()
        .map(|root| shell_word(&join(&root.root, RELEASE_PLZ_FILE)))
        .collect();
    let binds_tags = helper.at_least(TAG_PATTERN_HELPER);
    let tags: Vec<String> = roots
        .iter()
        .filter(|_| binds_tags)
        .flat_map(|root| {
            let file = join(&root.root, RELEASE_PLZ_FILE);
            root.packages
                .iter()
                .map(|(_, pattern)| pattern)
                .chain(&root.workspace_tags)
                .map(move |pattern| {
                    format!(
                        "--tag-pattern {} {}",
                        shell_word(&file),
                        shell_word(pattern.as_str())
                    )
                })
        })
        .collect();
    let (name, env) = if binds_tags {
        let mut env = variables(&[
            ("GITHUB_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
        ]);
        env.push((
            "RUSTUP_TOOLCHAIN".to_owned(),
            Value::Text(TOOLCHAIN.to_owned()),
        ));
        env.extend(
            NO_PROGRAM_VARIABLES
                .iter()
                .map(|(key, value)| ((*key).to_owned(), Value::Text((*value).to_owned()))),
        );
        (
            "Refuse a release-plz config that runs commands, changes the tag patterns or names another registry",
            env,
        )
    } else {
        ("Refuse a release-plz config that runs commands", Vec::new())
    };
    RenderedStep {
        name: name.to_owned(),
        env,
        run: Some(
            std::iter::once(format!(
                "velnor-ci release check-config {}",
                files.join(" ")
            ))
            .chain(tags)
            .chain(binds_tags.then(|| "--repo=\"$REPOSITORY\" --manifests".to_owned()))
            .chain(
                (binds_tags && tags_released_version && helper.at_least(RELEASED_VERSIONS_HELPER))
                    .then(|| "--released-versions".to_owned()),
            )
            .collect::<Vec<_>>()
            .join(" "),
        ),
        ..RenderedStep::default()
    }
}

pub(crate) fn release_plz_step(name: String, run: &str) -> RenderedStep {
    let mut env = variables(&[("GITHUB_TOKEN", APP_TOKEN_OUTPUT)]);
    env.push((
        "RUSTUP_TOOLCHAIN".to_owned(),
        Value::Text(TOOLCHAIN.to_owned()),
    ));
    env.extend(
        NO_PROGRAM_VARIABLES
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Value::Text((*value).to_owned()))),
    );
    RenderedStep {
        name,
        env,
        run: Some(format!(
            "{RELEASE_PLZ_BIN} {}",
            run.strip_prefix("release-plz ").unwrap_or(run)
        )),
        ..RenderedStep::default()
    }
}

pub(crate) fn release_plz_arguments(root: &RootPlan) -> String {
    format!(
        "--forge github --git-token \"$GITHUB_TOKEN\" --manifest-path {} --config {}",
        shell_word(&join(&root.root, "Cargo.toml")),
        shell_word(&join(&root.root, RELEASE_PLZ_FILE))
    )
}

pub(crate) fn label(root: &RootPlan) -> &str {
    if root.root == "." {
        "the repository root"
    } else {
        &root.root
    }
}

pub(crate) fn join(root: &str, file: &str) -> String {
    if root == "." {
        file.to_owned()
    } else {
        format!("{root}/{file}")
    }
}

pub(crate) fn shell_word(word: &str) -> String {
    if word
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}
