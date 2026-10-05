#!/usr/bin/env bash
set -euo pipefail
base=/root/.cache/velnor-pr12-foundation-tmp-20261005/qualified-tools
rust_prefix="$base/prefix-rust"
nextest_prefix="$base/prefix-nextest"

for component_name in cargo rustc rust-std-x86_64-unknown-linux-gnu; do
  found=0
  for package_root in "$base"/rust/*; do
    if [[ -f "$package_root/components" ]] && rg -q -x -- "$component_name" "$package_root/components"; then
      component_root="$package_root/$component_name"
      manifest="$component_root/manifest.in"
      [[ -f "$manifest" ]]
      found=$((found + 1))
      while IFS= read -r record; do
        [[ "$record" == *:* ]]
        kind=${record%%:*}
        relative=${record#*:}
        [[ -n "$relative" && "$relative" != /* && "$relative" != *..* ]]
        destination="$rust_prefix/$relative"
        source="$component_root/$relative"
        case "$kind" in
          dir)
            if [[ -e "$destination" || -L "$destination" ]]; then
              [[ -d "$destination" && ! -L "$destination" ]]
            else
              mkdir -p -- "$destination"
            fi
            ;;
          file)
            [[ ! -e "$destination" && ! -L "$destination" ]]
            mkdir -p -- "${destination%/*}"
            cp -a -- "$source" "$destination"
            ;;
          *)
            exit 41
            ;;
        esac
      done < "$manifest"
    fi
  done
  [[ "$found" -eq 1 ]]
done
cp -a -- "$base/nextest/cargo-nextest" "$nextest_prefix/cargo-nextest"

for tree in "$rust_prefix" "$nextest_prefix"; do
  entries="${tree}.entries.jsonl"
  manifest="${tree}.manifest.json"
  : > "$entries"
  while IFS= read -r -d '' path; do
    relative=${path#"$tree"/}
    if [[ -L "$path" ]]; then
      target=$(readlink -- "$path")
      jq -cnS --arg path "$relative" --arg target "$target" '{path:$path,kind:"symlink",target:$target}' >> "$entries"
    elif [[ -d "$path" ]]; then
      jq -cnS --arg path "$relative" '{path:$path,kind:"directory"}' >> "$entries"
    elif [[ -f "$path" ]]; then
      digest=$(sha256sum -- "$path" | cut -d' ' -f1)
      executable=false
      if [[ -x "$path" ]]; then executable=true; fi
      jq -cnS --arg path "$relative" --arg sha256 "$digest" --argjson executable "$executable" '{path:$path,kind:"file",sha256:$sha256,executable:$executable}' >> "$entries"
    else
      exit 42
    fi
  done < <(find "$tree" -mindepth 1 -print0 | sort -z)
  printf '[' > "$manifest"
  paste -sd, "$entries" >> "$manifest"
  printf ']' >> "$manifest"
  printf '%s ' "$tree"
  sha256sum "$manifest"
done

printf '\n--- rustc ---\n'
env HOME="$base/home" RUSTUP_HOME="$base/rust-home" CARGO_HOME="$base/cargo-home" RUSTUP_TOOLCHAIN="$rust_prefix" PATH="$rust_prefix/bin:/usr/bin:/bin" "$rust_prefix/bin/rustc" -vV
printf '\n--- cargo ---\n'
env HOME="$base/home" RUSTUP_HOME="$base/rust-home" CARGO_HOME="$base/cargo-home" RUSTUP_TOOLCHAIN="$rust_prefix" PATH="$rust_prefix/bin:/usr/bin:/bin" "$rust_prefix/bin/cargo" --version
printf '\n--- nextest ---\n'
"$nextest_prefix/cargo-nextest" --version
printf '\n--- executable digests ---\n'
sha256sum "$rust_prefix/bin/cargo" "$rust_prefix/bin/rustc" "$nextest_prefix/cargo-nextest"
