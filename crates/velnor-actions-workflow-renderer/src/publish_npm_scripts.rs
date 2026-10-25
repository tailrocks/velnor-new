pub const NPM_DIR: &str = "$RUNNER_TEMP/npm";
pub const NPM_REGISTRY: &str = "https://npm.pkg.github.com";

#[must_use]
pub fn tarball_stem(member: &str) -> String {
    member.trim_start_matches('@').replace('/', "-")
}

#[must_use]
pub fn pack_script(run_pack: &str, tarballs: &str, member: &str) -> String {
    let stem = tarball_stem(member);
    let dir = NPM_DIR;
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; out=\"{dir}\"; mkdir -p \"$out\"; \
         cd -P -- \"$GITHUB_WORKSPACE\"; {run_pack}; \
         (cd -- '{tarballs}' && sha256sum -- *.tgz) > \"$out/first.sha256\"; {run_pack}; \
         (cd -- '{tarballs}' && sha256sum -- *.tgz) > \"$out/second.sha256\"; \
         cmp -s -- \"$out/first.sha256\" \"$out/second.sha256\" \
         || {{ printf 'velnor: tarballs differ between two packs\\n' >&2; exit 1; }}; \
         file='{tarballs}'/'{stem}'-\"$ver\".tgz; test -f \"$file\"; \
         tar -xOf \"$file\" package/package.json | jq -e --arg n '{member}' --arg v \"$ver\" \
         '.name == $n and .version == $v and (.private != true) and .publishConfig.registry == \"{NPM_REGISTRY}\"' \
         > /dev/null; cp -- \"$file\" \"$out/\"; rm -f -- \"$out/first.sha256\" \"$out/second.sha256\"; \
         cd -- \"$out\"; sha256sum -- '{stem}'-\"$ver\".tgz > '{stem}'-\"$ver\".tgz.sha256"
    )
}

#[must_use]
pub fn verify_script(member: &str) -> String {
    let stem = tarball_stem(member);
    let dir = NPM_DIR;
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; cd -- \"{dir}\"; \
         ls | wc -l | grep -Eqx ' *2'; sha256sum -c -- '{stem}'-\"$ver\".tgz.sha256"
    )
}

#[must_use]
pub fn publish_script(member: &str) -> String {
    let stem = tarball_stem(member);
    let dir = NPM_DIR;
    let document = r#"{_id: $name, name: $name, "dist-tags": {latest: $ver}, versions: {($ver): ($manifest[0] + {_id: ($name + "@" + $ver), name: $name, version: $ver, dist: {integrity: $integrity, shasum: $sha1, tarball: ("https://npm.pkg.github.com/download/" + $name + "/" + $ver + "/" + $sha1)}})}, _attachments: {($tgz): {content_type: "application/octet-stream", data: $data, length: $len}}}"#;
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; cd -- \"{dir}\"; tgz='{stem}'-\"$ver\".tgz; \
         sha1sum -- \"$tgz\" > sha1.txt; read -r sha1 rest < sha1.txt; \
         openssl dgst -sha512 -binary \"$tgz\" | openssl base64 -A > sha512.b64; \
         read -r b64 < sha512.b64; wc -c < \"$tgz\" > size.txt; read -r len < size.txt; \
         base64 -w0 \"$tgz\" > data.b64; tar -xOf \"$tgz\" package/package.json > manifest.json; \
         jq -n --arg name '{member}' --arg ver \"$ver\" --arg tgz \"$tgz\" --arg sha1 \"$sha1\" \
         --arg integrity \"sha512-$b64\" --argjson len \"$len\" --rawfile data data.b64 \
         --slurpfile manifest manifest.json '{document}' > document.json; \
         route='{route}'; \
         printf 'header = \"Authorization: Bearer %s\"\\n' \"$GH_TOKEN\" \
         | curl --silent --show-error --fail-with-body --config - --request PUT \
         --header 'Content-Type: application/json' --data-binary @document.json \
         \"{NPM_REGISTRY}/$route\"",
        route = route_of(member),
    )
}

#[must_use]
pub fn route_of(member: &str) -> String {
    member.replacen('/', "%2F", 1)
}
