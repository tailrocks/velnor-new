use std::error::Error;
use std::process::Command;

use super::{Family, family, test_pins};

/// Regression: a later dispatch on an already-published, unchanged version must be a
/// reported no-op, not a failed release run (run 39429134110 failed on a published
/// v0.1.29 because the release target differed from the later source commit).
#[test]
fn generator_prepare_reports_published_version_at_earlier_commit_as_noop()
-> Result<(), Box<dyn Error>> {
    let generator = family::prepare_script(Family::Generator, &test_pins())?;
    let function = extract_function(&generator, "classify_release_commit() {")?;
    let release = "e7b2f088715379c22be473fab896a43d50cc16fc";
    let source = "5eaabb91e14c65a0f8f5a22329e3df802bd6fc94";
    let classify = |release_commit: &str, fixed_tag: &str, status: &str| {
        let script = format!(
            "fail() {{ printf '%s\\n' \"$1\" >&2; exit 1; }}\nrepository=r/r\nsource_sha={source}\nrelease_commit={release_commit}\nfixed_tag={fixed_tag}\ngh() {{ test \"$1 $2\" = 'api repos/r/r/compare/{release_commit}...{source}' || return 1; printf '%s\\n' {status}; }}\n{function}\nclassify_release_commit\n"
        );
        Command::new("bash")
            .args(["-euo", "pipefail", "-c", &script])
            .output()
    };
    let same = classify(source, "v0.1.29", "identical")?;
    assert_eq!(String::from_utf8_lossy(&same.stdout).trim(), "exact");
    let earlier = classify(release, "v0.1.29", "ahead")?;
    assert!(earlier.status.success());
    assert_eq!(String::from_utf8_lossy(&earlier.stdout).trim(), "earlier");
    for status in ["diverged", "behind"] {
        let refused = classify(release, "v0.1.29", status)?;
        assert!(!refused.status.success(), "{status} must be refused");
        assert!(String::from_utf8_lossy(&refused.stderr).contains("not an ancestor"));
    }
    let sha_tagged = classify(release, "", "ahead")?;
    assert!(!sha_tagged.status.success(), "SHA-keyed tags never float");
    assert!(generator.contains("source %s is NOT released"));
    assert!(generator.contains("action=complete"));

    Ok(())
}

fn extract_function<'a>(script: &'a str, header: &str) -> Result<&'a str, Box<dyn Error>> {
    let start = script.find(header).ok_or("function is missing")?;
    let end = script[start..]
        .find("\n}\n")
        .map(|offset| start + offset + 3)
        .ok_or("function is unterminated")?;
    Ok(&script[start..end])
}
