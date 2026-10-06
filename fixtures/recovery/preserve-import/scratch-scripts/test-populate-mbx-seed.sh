#!/usr/bin/env bash
# Drive populate-mbx-seed.sh. Do not mount velnor-seed.
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
tool="$root/populate-mbx-seed.sh"
prefix="linux-x64-mbx-velnor-mbx-1.21.1-share-out-dir-disabled-v1-action-1687e54eb349cadf61fa38b5813a77875489e8e6-dir-dir-rust-778fbc643a80-rust-tron-migration__local-"
work="$(mktemp -d "$root/seed-test.XXXXXX")"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT

payload="$work/payload"
dest_parent="$work/seed"
mkdir -p "$payload/bundle/obj" "$dest_parent/generator"
printf 'keep\n' >"$dest_parent/generator/MARKER"
printf 'object\n' >"$payload/bundle/obj/a"
printf '%s\n' "$prefix" >"$payload/PREFIX"

bash "$tool" "$payload" "$dest_parent/mbx"
got="$(awk 'NR==1 { print; exit }' "$dest_parent/mbx/PREFIX")"
[ "$got" = "$prefix" ]
[ -f "$dest_parent/mbx/bundle/obj/a" ]
[ "$(cat "$dest_parent/generator/MARKER")" = "keep" ]

# A second copy replaces mbx and leaves the sibling marker.
printf 'object-2\n' >"$payload/bundle/obj/a"
bash "$tool" "$payload" "$dest_parent/mbx"
[ "$(cat "$dest_parent/mbx/bundle/obj/a")" = "object-2" ]
[ "$(cat "$dest_parent/generator/MARKER")" = "keep" ]

# Missing bundle refuses and leaves the previous dest.
rm -rf "$payload/bundle"
if bash "$tool" "$payload" "$dest_parent/mbx"; then
  printf '%s\n' "missing bundle was accepted" >&2
  exit 1
fi
[ -f "$dest_parent/mbx/bundle/obj/a" ]

# Symlink bundle refuses.
mkdir -p "$payload/bundle"
rm -rf "$payload/bundle"
ln -s "$dest_parent/mbx/bundle" "$payload/bundle"
if bash "$tool" "$payload" "$dest_parent/mbx"; then
  printf '%s\n' "symlink bundle was accepted" >&2
  exit 1
fi
rm "$payload/bundle"
mkdir -p "$payload/bundle/obj"
printf 'object\n' >"$payload/bundle/obj/a"

# Live mode refuses while a worker is reported.
busy="$work/busy.sh"
cat >"$busy" <<'EOF'
#!/bin/bash
printf '%s\n' "w7a5eed3e3cad06ad769fedbb8c29496c-runner"
EOF
chmod 755 "$busy"
if MBX_SEED_LIVE=1 MBX_SEED_BUSY_CMD="$busy" bash "$tool" "$payload" "$dest_parent/mbx"; then
  printf '%s\n' "busy seed was written" >&2
  exit 1
fi
[ "$(cat "$dest_parent/mbx/bundle/obj/a")" = "object-2" ]

# Live mode copies when the worker check is empty, the producer succeeded,
# and the installed daemon is the executing ACL hash.
idle="$work/idle.sh"
cat >"$idle" <<'EOF'
#!/bin/bash
exit 0
EOF
chmod 755 "$idle"
ok_producer="$work/producer-ok.sh"
cat >"$ok_producer" <<'EOF'
#!/bin/bash
printf '%s\n' success
EOF
chmod 755 "$ok_producer"
bad_producer="$work/producer-bad.sh"
cat >"$bad_producer" <<'EOF'
#!/bin/bash
printf '%s\n' in_progress
EOF
chmod 755 "$bad_producer"
ok_daemon="$work/daemon-ok.sh"
cat >"$ok_daemon" <<'EOF'
#!/bin/bash
printf '%s\n' 391fbaf956ab69a66ff3b9b507962ec2cbb0062f4960719f7a1d37b7ed5253d9
EOF
chmod 755 "$ok_daemon"
bad_daemon="$work/daemon-bad.sh"
cat >"$bad_daemon" <<'EOF'
#!/bin/bash
printf '%s\n' 5e8cc66a400df8054779f1ad56668581d3c9b358976a3cfbaad99fe15573a83f
EOF
chmod 755 "$bad_daemon"
printf 'warm\n' >"$payload/bundle/obj/a"
if MBX_SEED_LIVE=1 MBX_SEED_BUSY_CMD="$idle" MBX_SEED_PRODUCER_CMD="$bad_producer" MBX_SEED_DAEMON_CMD="$ok_daemon" bash "$tool" "$payload" "$dest_parent/mbx"; then
  printf '%s\n' "unfinished producer was accepted" >&2
  exit 1
fi
[ "$(cat "$dest_parent/mbx/bundle/obj/a")" = "object-2" ]
if MBX_SEED_LIVE=1 MBX_SEED_BUSY_CMD="$idle" MBX_SEED_PRODUCER_CMD="$ok_producer" MBX_SEED_DAEMON_CMD="$bad_daemon" bash "$tool" "$payload" "$dest_parent/mbx"; then
  printf '%s\n' "old daemon was accepted" >&2
  exit 1
fi
[ "$(cat "$dest_parent/mbx/bundle/obj/a")" = "object-2" ]
MBX_SEED_LIVE=1 MBX_SEED_BUSY_CMD="$idle" MBX_SEED_PRODUCER_CMD="$ok_producer" MBX_SEED_DAEMON_CMD="$ok_daemon" bash "$tool" "$payload" "$dest_parent/mbx"
[ "$(cat "$dest_parent/mbx/bundle/obj/a")" = "warm" ]
[ "$(cat "$dest_parent/generator/MARKER")" = "keep" ]
printf '%s\n' "populate-mbx-seed tests passed"
