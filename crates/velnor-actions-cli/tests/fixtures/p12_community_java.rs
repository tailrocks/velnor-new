//! Community Java freshness parses the actual gate's provider asset contract.

use std::error::Error;
use std::process::Command;

const SETUP: &str = r"
import json, re, urllib.request
text = open(__import__('sys').argv[1], encoding='utf-8').read()
exec(text.split('FETCH_TIMEOUT = 10\n', 1)[1].split('\nif check_upstream:', 1)[0])
base = 'https://github.com/graalvm/graalvm-ce-builds/releases/'
def release(version='25.0.4.1.1', profile='25i4', tag='graal-25.4.4.1.1'):
    prefix = (profile + '-') if profile else ''
    assets = []
    for target in sorted(COMMUNITY_TARGETS):
        name = f'graalvm-community-jdk-{prefix}{version}_{target}_bin.tar.gz'
        assets.append(dict(name=name, digest='sha256:' + 'a' * 64,
                           browser_download_url=base + 'download/' + tag + '/' + name))
    return dict(tag_name=tag, html_url=base + 'tag/' + tag,
                draft=False, prerelease=False, assets=assets)
def latest(rows, source=COMMUNITY_SOURCE):
    return sniff_latest(source, json.dumps(rows), True)
def rejected(rows, source=COMMUNITY_SOURCE):
    try:
        latest(rows, source)
    except (ValueError, TypeError):
        return
    raise AssertionError('invalid Community cohort accepted')
";

fn check(case: &str) -> Result<(), Box<dyn Error>> {
    let script = super::repo_root().join("scripts/check-freshness.sh");
    let output = Command::new("python3")
        .args(["-c", &format!("{SETUP}\n{case}"), &script.to_string_lossy()])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn official_assets_map_profile_and_legacy_tags_without_tag_math() -> Result<(), Box<dyn Error>> {
    check(
        r"assert latest([release()]) == '25.0.4.1.1'
assert latest([release('25.0.4.1', '25i3', 'graal-25.3.4.1')]) == '25.0.4.1'
assert latest([release('25.0.2', '', 'jdk-25.0.2')]) == '25.0.2'
assert latest([release()], 'file:///fixture/java.json') == '25.0.4.1.1'",
    )
}

#[test]
fn numeric_order_keeps_every_version_component_and_jdk25_cohort() -> Result<(), Box<dyn Error>> {
    check(
        r"rows = [release('26.99.0', ''), release('25.0.4.1'), release()]
assert latest(rows) == '25.0.4.1.1'
assert latest(rows + [release('25.0.10')]) == '25.0.10'",
    )
}

#[test]
fn nonstable_and_unrelated_releases_never_override_stable_assets() -> Result<(), Box<dyn Error>> {
    check(
        r"draft = release('25.99.0'); draft['draft'] = True
preview = release('25.98.0'); preview['prerelease'] = True
unrelated = release(); unrelated['assets'] = []
assert latest([draft, preview, unrelated, release()]) == '25.0.4.1.1'
rejected([draft, preview, unrelated])",
    )
}

#[test]
fn exact_pin_or_oracle_sources_cannot_qualify_freshness() -> Result<(), Box<dyn Error>> {
    check(
        r"rejected([release()], COMMUNITY_SOURCE.split('?')[0] + '/tags/graal-25.3.4.1')
rejected([release()], 'https://www.oracle.com/a/tech/docs/graalvm-downloads.json')
rejected({'normal': {'Title': 'Oracle GraalVM'}}, 'file:///fixture/java.json')
rejected({'tag_name': 'graal-25.3.4.1'}, 'file:///fixture/java.json')",
    )
}

#[test]
fn missing_platform_and_duplicate_platform_fail_closed() -> Result<(), Box<dyn Error>> {
    check(
        r"row = release(); row['assets'].pop(); rejected([row])
row = release(); row['assets'].append(row['assets'][0]); rejected([row])",
    )
}

#[test]
fn mismatched_asset_version_or_profile_fails_closed() -> Result<(), Box<dyn Error>> {
    check(
        r"row = release(); row['assets'][0] = release('25.0.4.1')['assets'][0]; rejected([row])
row = release(); row['assets'][0] = release(profile='25i3')['assets'][0]; rejected([row])",
    )
}

#[test]
fn wrong_provider_asset_url_or_tag_fails_closed() -> Result<(), Box<dyn Error>> {
    check(
        r"row = release(); row['html_url'] = 'https://github.com/oracle/graal/releases/tag/x'; rejected([row])
row = release(); row['assets'][0]['browser_download_url'] += '.sha256'; rejected([row])
row = release(); row['tag_name'] = 'vm-19.3.1'; rejected([row])",
    )
}

#[test]
fn missing_or_malformed_digest_and_stable_flags_fail_closed() -> Result<(), Box<dyn Error>> {
    check(
        r"for digest in [None, 'sha256:no', 'md5:' + 'a' * 64]:
    row = release(); row['assets'][0]['digest'] = digest; rejected([row])
row = release(); del row['draft']; rejected([row])",
    )
}

#[test]
fn malformed_json_assets_and_oversized_pages_fail_closed() -> Result<(), Box<dyn Error>> {
    check(
        r"row = release(); row['assets'][0]['name'] = 'graalvm-community-jdk-25oops_linux-x64_bin.tar.gz'; rejected([row])
rejected([release()] * 11)
row = release(); row['padding'] = 'x' * FETCH_CAP; rejected([row])
try:
    sniff_latest(COMMUNITY_SOURCE, '{garbage', True)
except ValueError:
    pass
else:
    raise AssertionError('malformed JSON accepted')",
    )
}

#[test]
fn bounded_pagination_uses_listing_only_and_never_falls_back_to_pin() -> Result<(), Box<dyn Error>>
{
    check(
        r"calls = []
def fetch_text(url):
    calls.append(url); return json.dumps([release()])
assert latest([release('26.0.0', '')] * 10) == '25.0.4.1.1'
assert calls == [COMMUNITY_SOURCE + '&page=2']
calls.clear()
def fetch_text(url):
    calls.append(url); return json.dumps([release('25.0.10.0.0')])
assert latest([release()] * 10) == '25.0.10.0.0'
assert calls == [COMMUNITY_SOURCE + '&page=2']
calls.clear()
def fetch_text(url):
    calls.append(url); return json.dumps([release('26.0.0', '')] * 10)
rejected([release('26.0.0', '')] * 10)
assert calls == [COMMUNITY_SOURCE + '&page=2', COMMUNITY_SOURCE + '&page=3']",
    )
}
