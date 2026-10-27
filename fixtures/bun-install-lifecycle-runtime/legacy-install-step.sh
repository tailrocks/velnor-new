set -euo pipefail
cd -P -- "$GITHUB_WORKSPACE"
d="$RUNNER_TEMP/velnor/bun"
mkdir -p "$d"
cfg="$d/bunfig.toml"
printf '%s\n' '[install]' > "$cfg"
'mise' '--no-config' '--no-env' '--no-hooks' 'exec' "bun@${VELNOR_TOOL_BUN}" '--' 'bun' 'install' '--frozen-lockfile' '--cwd=frontend' "--config=$cfg"
