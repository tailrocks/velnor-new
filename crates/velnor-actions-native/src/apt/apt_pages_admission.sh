set -euo pipefail
: "${FULL_CI_RESULT:?missing full CI result}"
test "$FULL_CI_RESULT" = success
: "${EXPECTED_ARTIFACT_ID:?missing staging artifact id}"
: "${EXPECTED_ARTIFACT_DIGEST:?missing staging artifact digest}"
export ARTIFACT_ID="$EXPECTED_ARTIFACT_ID"
export ARTIFACT_DIGEST="$EXPECTED_ARTIFACT_DIGEST"
python3 -I -S - <<'VELNOR_APT_ADMISSION_SOURCE'
@@APT_SHARED_ADMISSION_SOURCE@@
VELNOR_APT_ADMISSION_SOURCE
python3 -I -S - staging <<'VELNOR_APT_TRANSPORT_SOURCE'
@@APT_TRANSPORT_SOURCE@@
VELNOR_APT_TRANSPORT_SOURCE
exec python3 -I -S - guard .github/velnor/apt-delivery.jsonc <<'VELNOR_APT_ENTRY_SOURCE'
@@APT_ENTRY_SOURCE@@
VELNOR_APT_ENTRY_SOURCE
