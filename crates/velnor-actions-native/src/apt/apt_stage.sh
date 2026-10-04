set -euo pipefail
exec python3 -I -S - "$@" <<'VELNOR_APT_ENTRY_SOURCE'
@@APT_ENTRY_SOURCE@@
VELNOR_APT_ENTRY_SOURCE
