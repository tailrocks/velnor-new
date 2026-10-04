set -eu
verified=false
cache_available=false
error=PREPARATION_FAILED
if [ "$VELNOR_SOURCE_OUTCOME" = success ] && [ "$VELNOR_SOURCE_VERIFIED" = true ]; then
  verified=true
  error=CACHE_NOT_PUBLISHED
  if [ "$VELNOR_SOURCE_SNAPSHOT_OUTCOME" != success ]; then
    verified=false
    error=SOURCE_VERIFICATION_FAILED
  elif [ "$VELNOR_SOURCE_SAVE_OUTCOME" = failure ]; then
    error=CACHE_TRANSPORT_FAILED
    if [ "$VELNOR_SOURCE_PUBLICATION_OUTCOME" = success ] &&
       [ -n "$VELNOR_SOURCE_PUBLICATION_MATCHED_KEY" ] &&
       [ "$VELNOR_SOURCE_PUBLICATION_MATCHED_KEY" = "$VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY" ]; then
      cache_available=true
    fi
  elif [ "$VELNOR_SOURCE_SAVE_OUTCOME" = success ]; then
    if [ "$VELNOR_SOURCE_PUBLICATION_OUTCOME" != success ]; then
      error=CACHE_TRANSPORT_UNAVAILABLE
    elif [ -z "$VELNOR_SOURCE_PUBLICATION_MATCHED_KEY" ]; then
      error=CACHE_TRANSPORT_UNAVAILABLE
    elif [ "$VELNOR_SOURCE_PUBLICATION_MATCHED_KEY" = "$VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY" ]; then
      cache_available=true
      error=NONE
    fi
  elif [ "$VELNOR_SOURCE_SAVE_OUTCOME" = skipped ]; then
    if [ "$VELNOR_SOURCE_RESTORE_OUTCOME" != success ]; then
      error=CACHE_TRANSPORT_UNAVAILABLE
    elif [ "$VELNOR_SOURCE_SNAPSHOT_OUTCOME" != success ]; then
      error=CACHE_TRANSPORT_UNAVAILABLE
    elif [ "$VELNOR_SOURCE_SNAPSHOT_CHANGED" != false ]; then
      error=CACHE_NOT_PUBLISHED
    else
      case "$VELNOR_SOURCE_RESTORE_KEY" in
        "$VELNOR_SOURCE_IDENTITY"|"$VELNOR_SOURCE_IDENTITY"-snapshot-*)
          cache_available=true
          error=NONE
          ;;
        *) error=CACHE_NOT_PUBLISHED ;;
      esac
    fi
  else
    error=CACHE_TRANSPORT_UNAVAILABLE
  fi
elif [ "$VELNOR_SOURCE_OUTCOME" = failure ]; then
  error=SOURCE_VERIFICATION_FAILED
fi
case "$VELNOR_SOURCE_ERROR" in
  PREPARATION_FAILED) error=PREPARATION_FAILED; verified=false; cache_available=false ;;
  PRIVATE_OR_AUTH_REQUIRED) error=PRIVATE_OR_AUTH_REQUIRED; verified=false; cache_available=false ;;
  PUBLIC_AUTHORITY_UNAVAILABLE|UNSUPPORTED_REGISTRY) error="$VELNOR_SOURCE_ERROR"; verified=false; cache_available=false ;;
  SOURCE_VERIFICATION_FAILED) error=SOURCE_VERIFICATION_FAILED; verified=false; cache_available=false ;;
  NONE|"") ;;
  *) error=SOURCE_VERIFICATION_FAILED; verified=false; cache_available=false ;;
esac
printf 'cache_available=%s\nverified=%s\nsourceidentity=%s\nerror=%s\n' \
  "$cache_available" "$verified" "$VELNOR_SOURCE_IDENTITY" "$error" >> "$GITHUB_OUTPUT"
