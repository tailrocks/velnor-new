#!/bin/sh
set -eu

image_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/velnor-cgroup-test.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM

helper_source="$image_dir/cgroup-v2-delegation.sh"
wrapper_source="$image_dir/start-dockerd.sh"
mock_bin="$scratch/bin"
mkdir -p "$mock_bin"

fail() {
	printf >&2 'FAIL: %s\n' "$1"
	exit 1
}

new_fixture() {
	root=$1
	controllers=$2
	mkdir -p "$root/init"
	printf '%s\n' "$controllers" > "$root/cgroup.controllers"
	printf '101\n' > "$root/cgroup.procs"
	printf 'existing\n' > "$root/init/cgroup.procs"
	: > "$root/cgroup.subtree_control"
}

prepare_wrapper() {
	root=$1
	max_attempts=$2
	cat "$helper_source" > "$scratch/helper.original"
	[ "$(grep -Fxc 'CGROUP_ROOT=/sys/fs/cgroup' "$scratch/helper.original")" -eq 1 ] \
		|| fail 'fixed cgroup root anchor changed'
	[ "$(grep -Fxc 'MAX_ATTEMPTS=100' "$scratch/helper.original")" -eq 1 ] \
		|| fail 'fixed retry count anchor changed'
	[ "$(grep -Fxc 'RETRY_DELAY_SECONDS=0.05' "$scratch/helper.original")" -eq 1 ] \
		|| fail 'fixed retry delay anchor changed'
	sed \
		-e "s|^CGROUP_ROOT=/sys/fs/cgroup$|CGROUP_ROOT=$root|" \
		-e "s|^MAX_ATTEMPTS=100$|MAX_ATTEMPTS=$max_attempts|" \
		-e 's|^RETRY_DELAY_SECONDS=0.05$|RETRY_DELAY_SECONDS=0|' \
		"$helper_source" > "$scratch/helper.sh"
	[ "$(grep -Fxc "CGROUP_ROOT=$root" "$scratch/helper.sh")" -eq 1 ] \
		|| fail 'test cgroup fixture substitution failed'
	sed \
		-e "s|^\\. /usr/local/libexec/velnor/cgroup-v2-delegation.sh$|. $scratch/helper.sh|" \
		-e "s|^exec /usr/local/bin/dockerd|exec $mock_bin/dockerd|" \
		"$wrapper_source" > "$scratch/wrapper.sh"
	[ -x "$scratch/wrapper.sh" ] || chmod 0755 "$scratch/wrapper.sh"
}

cat > "$mock_bin/xargs" <<'EOF'
#!/bin/sh
[ "$#" -eq 1 ] && [ "$1" = '-rn1' ] || exit 64
count=$(cat "$VELNOR_TEST_XARGS_COUNT")
count=$((count + 1))
printf '%s\n' "$count" > "$VELNOR_TEST_XARGS_COUNT"
tee -a "$VELNOR_TEST_MIGRATED_PIDS"
: > "$VELNOR_TEST_CGROUP_PROCS"
if [ "$count" -eq 1 ] && [ "${VELNOR_TEST_ADD_RACING_PID:-0}" -eq 1 ]; then
	printf '202\n' > "$VELNOR_TEST_CGROUP_PROCS"
fi
EOF

cat > "$mock_bin/sed" <<'EOF'
#!/bin/sh
count=$(cat "$VELNOR_TEST_SED_COUNT")
count=$((count + 1))
printf '%s\n' "$count" > "$VELNOR_TEST_SED_COUNT"
if [ "$count" -le "$VELNOR_TEST_SED_FAILS" ]; then
	exit 1
fi
output=$(/usr/bin/sed "$@") || exit $?
printf '%s\n' "$output" | /usr/bin/sed 's/+//g'
EOF

cat > "$mock_bin/dockerd" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" > "$VELNOR_TEST_DOCKER_ARGS"
printf 'started\n' > "$VELNOR_TEST_DOCKER_STARTED"
EOF
chmod 0755 "$mock_bin/xargs" "$mock_bin/sed" "$mock_bin/dockerd"

run_wrapper() {
	PATH="$mock_bin:/usr/bin:/bin" \
		VELNOR_TEST_CGROUP_PROCS="$fixture/cgroup.procs" \
		VELNOR_TEST_XARGS_COUNT="$scratch/xargs.count" \
		VELNOR_TEST_SED_COUNT="$scratch/sed.count" \
		VELNOR_TEST_MIGRATED_PIDS="$scratch/migrated-pids" \
		VELNOR_TEST_DOCKER_ARGS="$scratch/dockerd.args" \
		VELNOR_TEST_DOCKER_STARTED="$scratch/dockerd.started" \
		VELNOR_TEST_SED_FAILS="$sed_fails" \
		VELNOR_TEST_ADD_RACING_PID="$racing_pid" \
		"$scratch/wrapper.sh" 2> "$scratch/error.log"
}

reset_counts() {
	printf '0\n' > "$scratch/xargs.count"
	printf '0\n' > "$scratch/sed.count"
	: > "$scratch/migrated-pids"
	rm -f "$scratch/dockerd.args" "$scratch/dockerd.started"
}

fixture="$scratch/missing-cpu"
new_fixture "$fixture" 'memory pids'
prepare_wrapper "$fixture" 3
reset_counts
sed_fails=0
racing_pid=0
if run_wrapper; then fail 'missing CPU controller started dockerd'; fi
grep -Fq 'controller cpu is not delegated' "$scratch/error.log" \
	|| fail 'missing CPU controller did not report the failure'
[ ! -e "$scratch/dockerd.started" ] || fail 'dockerd started without CPU controller'
[ ! -s "$scratch/migrated-pids" ] || fail 'missing CPU controller moved tasks'

fixture="$scratch/missing-memory"
new_fixture "$fixture" 'cpu pids'
prepare_wrapper "$fixture" 3
reset_counts
sed_fails=0
racing_pid=0
if run_wrapper; then fail 'missing memory controller started dockerd'; fi
grep -Fq 'controller memory is not delegated' "$scratch/error.log" \
	|| fail 'missing memory controller did not report the failure'
[ ! -e "$scratch/dockerd.started" ] || fail 'dockerd started without memory controller'
[ ! -s "$scratch/migrated-pids" ] || fail 'missing memory controller moved tasks'

fixture="$scratch/write-failure"
new_fixture "$fixture" 'cpu memory pids'
rm "$fixture/cgroup.subtree_control"
mkdir "$fixture/cgroup.subtree_control"
prepare_wrapper "$fixture" 3
reset_counts
sed_fails=0
racing_pid=0
if run_wrapper; then fail 'permanent write failure started dockerd'; fi
grep -Fq 'did not settle after 3 attempts' "$scratch/error.log" \
	|| fail 'permanent write failure did not report bounded exhaustion'
[ "$(cat "$scratch/xargs.count")" -eq 3 ] || fail 'write failure retry count was not bounded'
[ ! -e "$scratch/dockerd.started" ] || fail 'dockerd started after failed delegation'
[ -d "$fixture/cgroup.subtree_control" ] \
	|| fail 'failed controller write replaced the rejected target'

fixture="$scratch/transient-race"
new_fixture "$fixture" 'cpu memory pids'
prepare_wrapper "$fixture" 5
reset_counts
sed_fails=1
racing_pid=1
run_wrapper || {
	cat "$scratch/error.log" >&2
	fail 'transient migration race did not recover'
}
[ "$(cat "$scratch/sed.count")" -eq 2 ] || fail 'transient race did not retry once'
grep -Fxq '101' "$scratch/migrated-pids" || fail 'initial root task was not moved'
grep -Fxq '202' "$scratch/migrated-pids" || fail 'racing root task was not moved'
[ "$(cat "$fixture/cgroup.subtree_control")" = 'cpu memory pids' ] \
	|| fail 'recovered delegation did not enable all controllers'
[ "$(cat "$scratch/dockerd.args")" = '--host=unix:///var/run/docker.sock' ] \
	|| fail 'dockerd did not keep the Unix-socket-only endpoint'
[ -e "$scratch/dockerd.started" ] || fail 'dockerd did not start after delegation succeeded'

printf 'cgroup-v2 delegation checks passed\n'
