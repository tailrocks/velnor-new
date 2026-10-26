# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Upgrade patch filter, shared by every upgrades.yml job that builds, checks
# or applies the patch. It validates the patch itself and the index the patch
# produces; it never relies on `git apply` rejecting anything.
#
# Rules: only upgrade-owned paths; no rename or copy; every file is a regular
# non-executable file (mode 100644), so no symlink, gitlink or mode change;
# only generated `.github/**` files may be deleted; no binary change
# (`velnor_upgrade_binary_allowed` names none).
velnor_upgrade_path_owned() {
  case "$1" in
    mise.toml|.mise.toml|mise.lock|.mise.lock|.velnor/release-manifest.json|.github/*) return 0 ;;
    rust-toolchain.toml|*/rust-toolchain.toml|rust-toolchain|*/rust-toolchain) return 0 ;;
    Cargo.toml|Cargo.lock|*/Cargo.toml|*/Cargo.lock) return 0 ;;
    package.json|bun.lock|*/package.json|*/bun.lock) return 0 ;;
    gradle/libs.versions.toml|*/gradle/libs.versions.toml|gradle.lockfile|*/gradle.lockfile) return 0 ;;
    gradle/verification-metadata.xml|*/gradle/verification-metadata.xml) return 0 ;;
  esac
  return 1
}
velnor_upgrade_deletable() { case "$1" in .github/*) return 0 ;; esac; return 1; }
velnor_upgrade_binary_allowed() { return 1; }
# Plain relative path: no empty, `.` or `..` segment, no `.git` segment in
# any case, and only portable characters.
velnor_upgrade_path_sane() {
  case "$1" in ''|/*|*/|*//*|*[!A-Za-z0-9._/@+-]*) return 1 ;; esac
  case "/$1/" in */./*|*/../*) return 1 ;; esac
  case "/$(printf '%s' "$1" | LC_ALL=C tr '[:upper:]' '[:lower:]')/" in */.git/*) return 1 ;; esac
  return 0
}
velnor_upgrade_reject() { printf '::error::%s: %s\n' "$1" "$2"; return 1; }

# Check one patch file's text: headers, file count and per-file paths.
velnor_upgrade_check_patch() {
  local patch=$1 headers files line rec added rest path
  local numstat=$patch.numstat
  test -s "$patch" || { velnor_upgrade_reject upgrade_patch_malformed "empty patch"; return 1; }
  if LC_ALL=C grep -q "$(printf '\r')" "$patch"; then velnor_upgrade_reject upgrade_patch_malformed "carriage return"; return 1; fi
  # Header lines never start with a hunk prefix, so an anchored match sees
  # exactly what `git apply` parses as a header.
  line=$(LC_ALL=C grep -n -m 1 -E '^(rename|copy) (from|to) |^(dis)?similarity index ' "$patch" || true)
  [ -z "$line" ] || { velnor_upgrade_reject upgrade_patch_rename_rejected "$line"; return 1; }
  line=$(LC_ALL=C grep -n -m 1 -E '^GIT binary patch|^Binary files |^(literal|delta) [0-9]' "$patch" || true)
  [ -z "$line" ] || { velnor_upgrade_reject upgrade_patch_binary_rejected "$line"; return 1; }
  line=$(LC_ALL=C grep -n -m 1 -E '^(old|new) mode ' "$patch" || true)
  [ -z "$line" ] || { velnor_upgrade_reject upgrade_patch_mode_rejected "$line"; return 1; }
  line=$(LC_ALL=C awk '/^(new|deleted) file mode / && $NF != "100644" { print NR ": " $0; exit }' "$patch")
  case "$line" in
    '') ;;
    *' 120000') velnor_upgrade_reject upgrade_patch_symlink_rejected "$line"; return 1 ;;
    *) velnor_upgrade_reject upgrade_patch_mode_rejected "$line"; return 1 ;;
  esac
  line=$(LC_ALL=C awk '/^index [0-9a-f]+[.][.][0-9a-f]+ / && $3 != "100644" { print NR ": " $0; exit }' "$patch")
  case "$line" in
    '') ;;
    *' 120000') velnor_upgrade_reject upgrade_patch_symlink_rejected "$line"; return 1 ;;
    *) velnor_upgrade_reject upgrade_patch_mode_rejected "$line"; return 1 ;;
  esac
  # Every file patch is a `diff --git` section: a traditional patch section
  # would appear in the numstat without a matching header.
  headers=$(LC_ALL=C grep -c '^diff --git ' "$patch" || true)
  git apply --numstat -z "$patch" > "$numstat" || { velnor_upgrade_reject upgrade_patch_malformed "git apply --numstat failed"; return 1; }
  files=0
  while IFS= read -r -d '' rec; do
    files=$((files + 1))
    added=${rec%%$'\t'*}
    rest=${rec#*$'\t'}
    path=${rest#*$'\t'}
    if [ -z "$path" ]; then velnor_upgrade_reject upgrade_patch_rename_rejected "file $files"; return 1; fi
    velnor_upgrade_path_sane "$path" || { velnor_upgrade_reject upgrade_patch_path_rejected "$path"; return 1; }
    velnor_upgrade_path_owned "$path" || { velnor_upgrade_reject upgrade_patch_path_rejected "$path"; return 1; }
    if [ "$added" = - ] && ! velnor_upgrade_binary_allowed "$path"; then velnor_upgrade_reject upgrade_patch_binary_rejected "$path"; return 1; fi
  done < "$numstat"
  [ "$files" -gt 0 ] && [ "$files" = "$headers" ] || { velnor_upgrade_reject upgrade_patch_malformed "$files file patches, $headers git headers"; return 1; }
}

# Check what the index holds against HEAD, entry by entry, with rename
# detection off so a move shows as its deletion plus its addition.
# `velnor_upgrade_check_index strict` fails on the first violation;
# `velnor_upgrade_check_index drop` unstages it and prints `<code> <path>`.
velnor_upgrade_check_index() {
  local mode=$1 meta path old new status code rec added rest
  local raw numstat
  raw=$(mktemp) numstat=$(mktemp)
  if ! git diff --cached --raw -z --no-renames --no-ext-diff HEAD -- > "$raw" \
    || ! git diff --cached --numstat -z --no-renames --no-ext-diff HEAD -- > "$numstat"; then
    velnor_upgrade_reject upgrade_patch_malformed "git diff --cached failed"; return 1
  fi
  while IFS= read -r -d '' meta && IFS= read -r -d '' path; do
    old=${meta#:}; old=${old%% *}
    new=${meta#:* }; new=${new%% *}
    status=${meta##* }
    code=
    if ! velnor_upgrade_path_sane "$path" || ! velnor_upgrade_path_owned "$path"; then code=upgrade_patch_path_rejected
    else
      case "$status:$old:$new" in
        M:100644:100644|A:000000:100644) ;;
        D:100644:000000) velnor_upgrade_deletable "$path" || code=upgrade_patch_delete_rejected ;;
        A:000000:120000|M:*:120000|T:*:120000) code=upgrade_patch_symlink_rejected ;;
        D:*) code=upgrade_patch_delete_rejected ;;
        *) code=upgrade_patch_mode_rejected ;;
      esac
    fi
    if [ -n "$code" ]; then
      if [ "$mode" = strict ]; then velnor_upgrade_reject "$code" "$path"; return 1; fi
      git reset --quiet -- "$path" || return 1
      printf '%s %s\n' "$code" "$path"
    fi
  done < "$raw"
  while IFS= read -r -d '' rec; do
    added=${rec%%$'\t'*}
    rest=${rec#*$'\t'}
    path=${rest#*$'\t'}
    if [ "$added" = - ] && ! velnor_upgrade_binary_allowed "$path"; then
      if [ "$mode" = strict ]; then velnor_upgrade_reject upgrade_patch_binary_rejected "$path"; return 1; fi
      git reset --quiet -- "$path" || return 1
      printf '%s %s\n' upgrade_patch_binary_rejected "$path"
    fi
  done < "$numstat"
  rm -f "$raw" "$numstat"
}

# Check a patch against HEAD in a scratch index, never the worktree, so a
# rejected patch writes nothing: text first, then the index it would produce.
velnor_upgrade_check_patch_applies() {
  local patch=$1 index=$2
  velnor_upgrade_check_patch "$patch" || return 1
  rm -f "$index"
  (
    export GIT_INDEX_FILE=$index
    git read-tree HEAD || exit 1
    git -c core.hooksPath=/dev/null apply --cached "$patch" \
      || { velnor_upgrade_reject upgrade_patch_malformed "git apply --cached failed"; exit 1; }
    velnor_upgrade_check_index strict
  )
}
