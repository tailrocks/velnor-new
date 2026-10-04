set -euo pipefail
profile=$(jq -ce '. as $profile | .artifacts |= map(. + {
  stable_name: (.prefix + "-1.2.3-" + .target + "." + (if .archive == "tar_gz" then "tar.gz" else "zip" end)),
  preview_name: (if .preview then .prefix + "-" + .target + "." + (if .archive == "tar_gz" then "tar.gz" else "zip" end) else null end),
  output: (if .output == "formula" then $profile.formula else $profile.cask end)
})' <<< "$1")
root=$(pwd -P)
tmp=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/velnor-package-fixtures.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
cp -R "$root/." "$tmp/repo"
updater=$(jq -er '.updater' <<< "$profile")
repository=$(jq -er '.repository' <<< "$profile")
formula=$(jq -er '.formula' <<< "$profile")
preview_formula=$(jq -er '.preview_formula' <<< "$profile")
binary=$(jq -er '.binary' <<< "$profile")
version=1.2.3
commit=0123456789abcdef0123456789abcdef01234567
preview_version=1.2.3-preview.42+0123456
different_commit=fedcba9876543210fedcba9876543210fedcba98
stable_outputs=()
while IFS= read -r path; do stable_outputs+=("$path"); done < <(jq -r '[.formula,.cask] | map(select(. != null))[]' <<< "$profile")
stable_names=()
while IFS= read -r name; do stable_names+=("$name"); done < <(jq -r '.artifacts[].stable_name' <<< "$profile")
preview_names=()
while IFS= read -r name; do preview_names+=("$name"); done < <(jq -r '.artifacts[] | select(.preview_name != null) | .preview_name' <<< "$profile")
case_id() { printf 'velnor-package-fixture-case: %s\n' "$1"; }
ruby_check() { (cd "$root" && ruby -c "$1") >/dev/null; }
shopt -s nullglob
source_formulae=("$root"/Formula/*.rb)
test "${#source_formulae[@]}" -gt 0
for path in "${source_formulae[@]}"; do ruby_check "$path"; done
shopt -u nullglob
test "$(grep -cF 'releases/download/$VELNOR_PACKAGE_RELEASE_TAG/' "$root/$updater")" -eq "${#preview_names[@]}"
if grep -Fq 'releases/download/preview/' "$root/$updater"; then
  echo 'literal preview release URLs are forbidden' >&2
  exit 1
fi

cat > "$tmp/check-output.rb" <<'RUBY_OUTPUT'
require "ripper"
def literal(node)
  return unless node.is_a?(Array) && node[0] == :string_literal
  content = node[1]
  return unless content[0] == :string_content
  fragments = content.drop(1)
  return unless fragments.all? { |part| part[0] == :@tstring_content }
  fragments.map { |part| part[1] }.join
end
def declaration(node)
  return unless node.is_a?(Array) && node[0] == :command
  name = node[1][1]
  return unless ["url", "sha256"].include?(name)
  args = node[2]
  return unless args[0] == :args_add_block && args[1].is_a?(Array)
  value = literal(args[1][0])
  raise "nonliteral artifact declaration" unless value
  [name, value]
end
def collect_pairs(node, pairs)
  return unless node.is_a?(Array)
  if node[0] == :bodystmt
    declarations = node[1].filter_map { |statement| declaration(statement) }
    raise "unpaired artifact declarations" unless declarations.length.even?
    declarations.each_slice(2) do |first, second|
      pair = [first, second].to_h
      raise "invalid artifact declaration pair" unless pair.keys.sort == ["sha256", "url"]
      raise "duplicate artifact URL" if pairs.key?(pair["url"])
      pairs[pair["url"]] = pair["sha256"]
    end
  end
  node.each { |child| collect_pairs(child, pairs) }
end
tree = Ripper.sexp(File.read(ARGV.fetch(0)))
raise "invalid generated Ruby" unless tree
pairs = {}
collect_pairs(tree, pairs)
raise "artifact URL/checksum mismatch" unless pairs[ARGV.fetch(1)] == ARGV.fetch(2)
RUBY_OUTPUT
output_pair() {
  (cd "$root" && ruby "$tmp/check-output.rb" "$tmp/repo/$1" "$2" "$3")
}

digest() { shasum -a 256 "$1" | awk '{print $1}'; }
asset_record() {
  jq -cn --arg name "$2" --arg sha256 "$(digest "$1/$2")" '{name:$name,sha256:$sha256}'
}
write_identity() {
  jq -Sn --arg source_repository "$repository" --arg source_ref "$2" \
    --arg source_digest "$commit" --slurpfile manifest "$1/release-manifest.json" \
    '{source_repository:$source_repository,source_ref:$source_ref,source_digest:$source_digest,manifest:$manifest[0]}' > "$1/identity.json"
}
run_updater() {
  (
    cd "$tmp/repo"
    unset VELNOR_VERIFIED_PACKAGE_DIR VELNOR_PACKAGE_CHANNEL VELNOR_PACKAGE_RELEASE_TAG
    export VELNOR_VERIFIED_PACKAGE_DIR="$1"
    if test "$2" = preview; then
      export VELNOR_PACKAGE_CHANNEL=preview
      if test "$3" != absent; then export VELNOR_PACKAGE_RELEASE_TAG="$3"; fi
    fi
    bash "$updater"
  )
}
assert_stable_outputs() {
  local path name output checksum count expected
  for path in "${stable_outputs[@]}"; do
    ruby_check "$tmp/repo/$path"
    grep -F "version \"$version\"" "$tmp/repo/$path" >/dev/null
    expected=$(jq -r --arg path "$path" '[.artifacts[] | select(.output == $path)] | length' <<< "$profile")
    count=$(grep -cE 'sha256 "[0-9a-f]{64}"$' "$tmp/repo/$path")
    test "$count" -eq "$expected"
  done
  while IFS=$'\t' read -r name output; do
    checksum=$(jq -er --arg name "$name" '.assets[] | select(.name == $name) | .sha256' "$stable/release-manifest.json")
    output_pair "$output" "https://github.com/$repository/releases/download/v$version/$name" "$checksum"
  done < <(jq -r '.artifacts[] | [.stable_name,.output] | @tsv' <<< "$profile")
}
snapshot_stable() { (cd "$tmp/repo" && shasum -a 256 "${stable_outputs[@]}") > "$tmp/stable.sha"; }
unchanged_stable() { (cd "$tmp/repo" && shasum -a 256 -c "$tmp/stable.sha"); }
snapshot_preview() { (cd "$tmp/repo" && shasum -a 256 "$preview_formula") > "$tmp/preview.sha"; }
unchanged_preview() { (cd "$tmp/repo" && shasum -a 256 -c "$tmp/preview.sha"); }

stable="$tmp/stable"
mkdir "$stable"
: > "$tmp/stable-assets.jsonl"
for name in "${stable_names[@]}"; do
  printf 'fixture-%s\n' "$name" > "$stable/$name"
  asset_record "$stable" "$name" >> "$tmp/stable-assets.jsonl"
done
jq -Sn --arg source_repository "$repository" --arg source_ref "refs/tags/v$version" \
  --arg source_commit "$commit" --arg version "$version" --slurpfile assets "$tmp/stable-assets.jsonl" \
  '{schema:"velnor.package-release.v1",source_repository:$source_repository,source_ref:$source_ref,source_commit:$source_commit,version:$version,assets:$assets}' > "$stable/release-manifest.json"
write_identity "$stable" "refs/tags/v$version"
case_id stable-success
run_updater "$stable" stable absent
assert_stable_outputs
snapshot_stable
case_id stable-idempotence
run_updater "$stable" stable absent
unchanged_stable
