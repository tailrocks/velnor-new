set -euo pipefail
: "${VERIFY:?missing verification result}"
: "${ADMIT:?missing admission result}"
: "${STAGE:?missing staging result}"
: "${DEPLOY:?missing deployment result}"
: "${PUBLICATION_ELIGIBLE:?missing publication eligibility}"
test "$VERIFY" = success
test "$ADMIT" = success
case "$PUBLICATION_ELIGIBLE" in
  true) expected=success ;;
  false) expected=skipped ;;
  *) echo 'invalid publication eligibility' >&2; exit 1 ;;
esac
test "$STAGE" = "$expected"
test "$DEPLOY" = "$expected"
