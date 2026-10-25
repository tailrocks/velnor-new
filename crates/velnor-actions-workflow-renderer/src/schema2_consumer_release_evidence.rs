use super::{ConsumerReleaseSpec, PackageReleasePolicy, Yaml};
use crate::steps::{ATTEST_BUILD_PROVENANCE_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
#[derive(Clone, Copy)]
pub(in crate::schema2) enum Artifact {
    Signed,
    TapCandidate,
    TapPr,
}

pub(in crate::schema2) fn transfer(
    spec: &ConsumerReleaseSpec,
    download: bool,
    artifact: Artifact,
) -> Yaml {
    let (suffix, directory) = match (download, artifact) {
        (true, Artifact::Signed) => ("signed", "release-publish-input"),
        (false, Artifact::Signed) => ("signed", "release-signed"),
        (_, Artifact::TapCandidate) => ("tap-candidate", "tap-candidate"),
        (true, Artifact::TapPr) => ("tap-pr", ""),
        (false, Artifact::TapPr) => ("tap-pr", "tap-pr.json"),
    };
    let mut with = vec![
        (
            "name".to_owned(),
            Yaml::str(format!(
                "velnor-{}-{suffix}-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}",
                super::super::TapInvocationContext::for_channel(spec.policy.channel).prefix()
            )),
        ),
        (
            "path".to_owned(),
            Yaml::str(format!("${{{{ runner.temp }}}}/velnor/{directory}")),
        ),
    ];
    if !download {
        with.extend([
            ("if-no-files-found".to_owned(), Yaml::str("error")),
            ("retention-days".to_owned(), Yaml::Int(30)),
            ("overwrite".to_owned(), Yaml::Bool(false)),
            ("include-hidden-files".to_owned(), Yaml::Bool(false)),
        ]);
    }
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(if download {
                "Download exact release evidence"
            } else {
                "Retain original release evidence"
            }),
        ),
        (
            "uses".to_owned(),
            Yaml::str(if download {
                DOWNLOAD_ARTIFACT_USES
            } else {
                UPLOAD_ARTIFACT_USES
            }),
        ),
        ("with".to_owned(), Yaml::Map(with)),
    ])
}

pub(in crate::schema2) fn attest(plan: &PackageReleasePolicy) -> Yaml {
    let prefix = "${{ runner.temp }}/velnor/release-signed";
    let mut names: Vec<String> = plan
        .products
        .iter()
        .flat_map(|product| {
            product
                .targets
                .iter()
                .map(move |target| format!("{}-{target}.tar.gz", product.id))
        })
        .collect();
    names.extend(
        plan.companions
            .iter()
            .map(|companion| companion.asset_name.clone()),
    );
    names.push("release.json".to_owned());
    names.sort();
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Attest exact native release subjects"),
        ),
        ("uses".to_owned(), Yaml::str(ATTEST_BUILD_PROVENANCE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("create-storage-record".to_owned(), Yaml::Bool(false)),
                (
                    "subject-path".to_owned(),
                    Yaml::str(
                        names
                            .iter()
                            .map(|name| format!("{prefix}/{name}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                ),
            ]),
        ),
    ])
}
