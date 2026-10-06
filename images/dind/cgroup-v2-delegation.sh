# This cgroup-v2 block is adapted from Moby hack/dind at
# 8d9e3502aba39127e4d12196dae16d306f76993d (SHA-256
# 673bb7adc8b463b56e9e929672ea74b40951f655a023d0b9801f46c9da4bc509).
# The source is Apache-2.0 licensed. See LICENSE-moby.
#
# Only the cgroup-v2 delegation block is used. This code does not change mount
# propagation, mount securityfs, or mount a tmpfs. The production caller uses
# the fixed /sys/fs/cgroup path in its private cgroup namespace.

CGROUP_ROOT=/sys/fs/cgroup
MAX_ATTEMPTS=100
RETRY_DELAY_SECONDS=0.05

velnor_enable_cgroup_v2() {
	if [ ! -f "$CGROUP_ROOT/cgroup.controllers" ]; then
		return 0
	fi

	controllers=$(cat "$CGROUP_ROOT/cgroup.controllers") || {
		printf >&2 'DinD cgroup-v2 startup failed: cannot read delegated controllers\n'
		return 1
	}
	available=" $controllers "
	for required in cpu memory; do
		case "$available" in
			*" $required "*) ;;
			*)
				printf >&2 'DinD cgroup-v2 startup failed: controller %s is not delegated\n' "$required"
				return 1
				;;
		esac
	done

	if ! mkdir -p "$CGROUP_ROOT/init"; then
		printf >&2 'DinD cgroup-v2 startup failed: cannot create delegated init cgroup\n'
		return 1
	fi

	# Move root tasks first. Domain controllers cannot be enabled on a populated
	# cgroup. Retry because docker exec can add a task during this transition.
	attempt=0
	while ! {
		xargs -rn1 < "$CGROUP_ROOT/cgroup.procs" > "$CGROUP_ROOT/init/cgroup.procs" || :
		sed -e 's/ / +/g' -e 's/^/+/' < "$CGROUP_ROOT/cgroup.controllers" \
			> "$CGROUP_ROOT/cgroup.subtree_control"
	}; do
		attempt=$((attempt + 1))
		if [ "$attempt" -ge "$MAX_ATTEMPTS" ]; then
			printf >&2 'DinD cgroup-v2 startup failed: controller delegation did not settle after %s attempts\n' "$MAX_ATTEMPTS"
			return 1
		fi
		if ! sleep "$RETRY_DELAY_SECONDS"; then
			printf >&2 'DinD cgroup-v2 startup failed: retry delay failed\n'
			return 1
		fi
	done

	enabled=$(cat "$CGROUP_ROOT/cgroup.subtree_control") || {
		printf >&2 'DinD cgroup-v2 startup failed: cannot read enabled controllers\n'
		return 1
	}
	enabled=" $enabled "
	for required in cpu memory; do
		case "$enabled" in
			*" $required "*) ;;
			*)
				printf >&2 'DinD cgroup-v2 startup failed: controller %s was not enabled\n' "$required"
				return 1
				;;
		esac
	done
}
