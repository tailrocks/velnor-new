#!/usr/bin/env bash
set -euo pipefail
proof_dir=/root/.cache/velnor-pr25-final-proof-1ece60d/runtime-probe-20261005T183149Z-1323142
dind=sha256:cf25ee0752caf965af68d9ed078ae63f5dcad49b9ad9effb0be24cd173e1da23
runner=sha256:71363278e057e2f9c405a3ff766bb84e70d42ab0b740f7578b274d753e825929
probe_id="$(date -u +%Y%m%dT%H%M%SZ)-$$-$RANDOM"
volume="velnor-pr25-agg-work-${probe_id}"
worker="runner25-agg-${probe_id}"
dind_name="runner25-agg-dind-${probe_id}"
runner_name="runner25-agg-runner-${probe_id}"
entry_name="runner25-agg-entry-${probe_id}"
inspect_name="runner25-agg-inspect-${probe_id}"
cleanup() {
  docker rm -f "$dind_name" "$runner_name" "$entry_name" "$inspect_name" >/dev/null 2>&1 || true
  docker volume rm "$volume" >/dev/null 2>&1 || true
}
printf 'probe_id=%s\nvolume=%s\nworker=%s\ndind=%s\nrunner=%s\n' \
  "$probe_id" "$volume" "$worker" "$dind" "$runner"
if docker volume inspect "$volume" >"$proof_dir/volume-preflight.stdout" 2>"$proof_dir/volume-preflight.stderr"; then
  echo 'volume preflight unexpectedly found existing volume' >&2
  exit 1
fi
grep -F 'no such volume' "$proof_dir/volume-preflight.stderr" >/dev/null
for object in "$dind_name" "$runner_name" "$entry_name" "$inspect_name"; do
  if docker container inspect "$object" >"$proof_dir/$object-preflight.stdout" 2>"$proof_dir/$object-preflight.stderr"; then
    echo "probe preflight unexpectedly found existing container: $object" >&2
    exit 1
  elif ! grep -F "No such container" "$proof_dir/$object-preflight.stderr" >/dev/null; then
    cat "$proof_dir/$object-preflight.stderr" >&2
    exit 1
  fi
done
trap cleanup EXIT
printf 'expected: volume absent before create\n'
docker volume create --label "velnor.worker=$worker" --label velnor.role=work "$volume"
docker volume inspect "$volume" --format 'labels={{.Labels}}'
docker run --rm --network none --name "$dind_name" \
  --mount "type=volume,src=$volume,dst=/home/runner/_work" \
  --entrypoint /bin/sh "$dind" -ec \
  'id; stat -c "%u:%g %a %n" /home/runner/_work; printf "dind-probe\\n" > /home/runner/_work/dind-probe; stat -c "%u:%g %a %n" /home/runner/_work/dind-probe'
if docker container inspect "$dind_name" >/dev/null 2>"$proof_dir/dind-container-absent.stderr"; then
  echo 'DinD probe container unexpectedly remains' >&2
  exit 1
fi
grep -F 'No such container' "$proof_dir/dind-container-absent.stderr" >/dev/null
printf 'expected: container absent: %s\n' "$dind_name"
docker run --rm --network none --name "$runner_name" \
  --mount "type=volume,src=$volume,dst=/home/runner/_work" \
  --entrypoint /bin/sh "$runner" -ec \
  'id; stat -c "%u:%g %a %n" /home/runner/_work; test "$(cat /home/runner/_work/dind-probe)" = dind-probe; printf "runner-probe\\n" > /home/runner/_work/runner-probe; stat -c "%u:%g %a %n" /home/runner/_work/runner-probe; cat /home/runner/_work/dind-probe; cat /home/runner/_work/runner-probe'
if docker container inspect "$runner_name" >/dev/null 2>"$proof_dir/runner-container-absent.stderr"; then
  echo 'runner access probe container unexpectedly remains' >&2
  exit 1
fi
grep -F 'No such container' "$proof_dir/runner-container-absent.stderr" >/dev/null
printf 'expected: container absent: %s\n' "$runner_name"
set +e
entrypoint_output=$(printf '{}' | docker run --network none --interactive --name "$entry_name" \
  --mount "type=volume,src=$volume,dst=/home/runner/_work" \
  --mount "type=bind,src=$proof_dir/probe-bin,dst=/probe-bin,readonly" \
  --env PATH=/probe-bin:/usr/local/bin:/usr/bin:/bin \
  --env WAIT_DOCKER_SOCK_TRIES=1 "$runner" 2>&1)
entrypoint_status=$?
set -e
printf '%s\nentrypoint_status=%s\n' "$entrypoint_output" "$entrypoint_status"
test "$entrypoint_status" -eq 1
test "$entrypoint_output" = 'docker socket missing'
docker inspect "$entry_name" --format '{{json .Mounts}}' | tee "$proof_dir/entrypoint-mounts.json"
/root/.local/share/mise/installs/jq/1.8.1/jq -e '
  . as $m
  | any($m[]; .Destination == "/home/runner/_work" and .Type == "volume")
  and any($m[]; .Destination == "/probe-bin" and .Type == "bind")
  and all($m[]; .Destination != "/tmp")
' "$proof_dir/entrypoint-mounts.json" >/dev/null
observations=$(docker run --rm --network none --name "$inspect_name" \
  --mount "type=volume,src=$volume,dst=/home/runner/_work" \
  --entrypoint /bin/sh "$runner" -ec \
  'cat /home/runner/_work/jit-observations; test "$(stat -c "%u:%g %a" /home/runner/_work/runner-probe)" = "1000:1000 644"; test "$(cat /home/runner/_work/dind-probe)" = dind-probe; test "$(cat /home/runner/_work/runner-probe)" = runner-probe')
printf '%s\n' "$observations" | tee "$proof_dir/jit-observations.txt"
jit_path=$(printf '%s\n' "$observations" | sed -n 's/^path=//p' | sed -n '1p')
case "$jit_path" in
  /tmp/velnor-jit.*) printf 'private_jit_path=%s\n' "$jit_path" ;;
  *) echo "unexpected JIT path: $jit_path" >&2; exit 1 ;;
esac
mktemp_mode=$(printf '%s\n' "$observations" | sed -n 's/^mktemp_mode=//p' | sed -n '1p')
chmod_mode=$(printf '%s\n' "$observations" | sed -n 's/^chmod_mode=//p' | sed -n '1p' | awk '{print $1}')
printf 'mktemp_mode=%s explicit_chmod_mode=%s\n' "$mktemp_mode" "$chmod_mode"
test "$mktemp_mode" = 600
test "$chmod_mode" = 600
if docker cp "$entry_name:$jit_path" "$proof_dir/unexpected-jit-copy" >"$proof_dir/jit-copy.stdout" 2>"$proof_dir/jit-copy.stderr"; then
  echo 'JIT tempfile remains in stopped runner container' >&2
  exit 1
fi
printf 'expected: JIT tempfile absent from stopped runner container\n'
cat "$proof_dir/jit-copy.stderr"
docker rm "$entry_name" >/dev/null
if docker container inspect "$entry_name" >"$proof_dir/entrypoint-container-absent.stdout" 2>"$proof_dir/entrypoint-container-absent.stderr"; then
  echo 'entrypoint probe container unexpectedly remains' >&2
  exit 1
fi
grep -F 'No such container' "$proof_dir/entrypoint-container-absent.stderr" >/dev/null
printf 'expected: container absent: %s\n' "$entry_name"
docker volume rm "$volume"
trap - EXIT
if docker volume inspect "$volume" >"$proof_dir/volume-postflight.stdout" 2>"$proof_dir/volume-postflight.stderr"; then
  echo 'volume unexpectedly remains after cleanup' >&2
  exit 1
fi
grep -F 'no such volume' "$proof_dir/volume-postflight.stderr" >/dev/null
printf 'expected: volume absent after cleanup\n'
