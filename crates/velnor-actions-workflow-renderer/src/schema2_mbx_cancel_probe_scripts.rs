//! Bounded shell scripts for the isolated hosted cancellation jobs.

pub(super) const CACHE_SNAPSHOT_FUNCTION: &str = r#"
cache_snapshot() {
  local source="$1" key="$2"
  if jq -cse --arg key "$key" '
    def valid_cache:
      type == "object"
      and (.id | type == "number" and . > 0 and . == floor)
      and (.key | type == "string" and length > 0)
      and (.ref | type == "string" and length > 0)
      and (.size_in_bytes | type == "number" and . >= 0 and . == floor)
      and has("last_accessed_at")
      and (.last_accessed_at == null or (.last_accessed_at | type == "string" and length > 0));
    if length != 1 then error("cache response count")
    elif (.[0] | type) != "object" then error("cache response object")
    elif (.[0].total_count | type) != "number"
      or (.[0].total_count | . < 0 or . != floor) then error("cache total count")
    elif (.[0].actions_caches | type) != "array" then error("cache array")
    elif .[0].total_count != (.[0].actions_caches | length) then error("cache page incomplete")
    elif (.[0].actions_caches | all(.[]; valid_cache)) != true then error("cache entry")
    else
      .[0].actions_caches
      | [.[] | select(.key == $key and .ref == "refs/heads/main")]
      | {count:length,caches:map({id,key,ref,size_in_bytes,last_accessed_at})}
    end
  ' "$source" 2>/dev/null; then
    return 0
  fi
  printf '%s\n' '{"count":-1,"caches":[]}'
}
"#;

#[path = "schema2_mbx_cancel_probe_controller_dispatch.rs"]
mod controller_dispatch;
#[path = "schema2_mbx_cancel_probe_controller_live.rs"]
mod controller_live;
#[path = "schema2_mbx_cancel_probe_controller_receipt.rs"]
mod controller_receipt;
#[path = "schema2_mbx_cancel_probe_observer.rs"]
mod observer;
#[path = "schema2_mbx_cancel_probe_victim.rs"]
mod victim;

pub(super) use controller_dispatch::{DISPATCH, GENERATE_ID};
pub(super) use controller_live::{cancel_exact, wait_readiness, wait_terminal};
pub(super) use controller_receipt::{CONTROLLER_RECEIPT, VALIDATE_CONTROLLER_RECEIPT};
pub(super) use observer::{
    OBSERVER_CLASSIFY, OBSERVER_IMPORT_MEASURE, OBSERVER_REUSE_MEASURE, observer_cache_before,
    observer_evidence,
};
pub(super) use victim::{
    BUILD_WORKSPACE, FETCH_SOURCE, PRE_SAVE_GUARD, PRE_SAVE_WAIT, VICTIM_IDENTITY,
    WRITE_VICTIM_RECEIPT,
};
