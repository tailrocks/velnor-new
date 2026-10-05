#!/bin/bash
set -euo pipefail
source_root=/var/tmp/velnor-native-c8-frozen-mode000-20261005
copy_root=/root/.cache/velnor-task-targets/20261005/mbx-cli-clippyfix-20261005T1501Z
manifest=/root/.cache/velnor-task-targets/20261005/stock-helper/frozen-c8-manifest/source-v2.manifest
evidence=/root/.cache/velnor-task-targets/20261005/mbx-cli-clippyfix-evidence-20261005T1503Z
awk '$1 ~ /^[[:xdigit:]]+$/ && length($1) == 64 && $2 ~ /^\.\//' "$manifest" | (cd "$source_root" && sha256sum -c --status)
awk '$1 ~ /^[[:xdigit:]]+$/ && length($1) == 64 && $2 ~ /^\.\//' "$manifest" | (cd "$copy_root" && sha256sum -c --status)
find "$source_root" -mindepth 1 -type d -printf 'DIR %m %P\n' | sort > "$evidence/source-post.dirs"
find "$copy_root" -mindepth 1 -type d -printf 'DIR %m %P\n' | sort > "$evidence/copy-post.dirs"
find "$source_root" -type f -printf 'FILE %m %s %P\n' | sort > "$evidence/source-post.files"
find "$copy_root" -type f -printf 'FILE %m %s %P\n' | sort > "$evidence/copy-post.files"
find "$source_root" -type l -printf 'LINK %m %P -> %l\n' | sort > "$evidence/source-post.links"
find "$copy_root" -type l -printf 'LINK %m %P -> %l\n' | sort > "$evidence/copy-post.links"
cmp "$evidence/source-post.dirs" "$evidence/copy-post.dirs"
cmp "$evidence/source-post.files" "$evidence/copy-post.files"
cmp "$evidence/source-post.links" "$evidence/copy-post.links"
printf 'source-v2 manifest SHA-256: %s\n' "$(sha256sum "$manifest" | cut -d ' ' -f1)"
printf 'source/copy SHA rows verified: %s\n' "$(awk '$1 ~ /^[[:xdigit:]]+$/ && length($1) == 64 && $2 ~ /^\.\//' "$manifest" | wc -l)"
printf 'matching directories: %s\n' "$(wc -l < "$evidence/copy-post.dirs")"
printf 'matching file mode/size rows: %s\n' "$(wc -l < "$evidence/copy-post.files")"
printf 'matching symlink mode/target rows: %s\n' "$(wc -l < "$evidence/copy-post.links")"
printf 'source and isolated copy match in content, modes, sizes, and symlink targets\n'
