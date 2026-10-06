//! Compiled cache observations using the sole shared archive inventory authority.

use velnor_actions_contract::CacheSnapshotDomain;

use crate::MiseError;
use crate::inventory_loader::compiled_inventory_loader;

/// Compile a fixed observer for closed payload ownership.
///
/// The domain and before/after arguments select closed policy without changing source
/// identity. Recorded metadata never grants privacy or execution authority.
/// # Errors
/// Rejects failures encoding the compiler-owned shared source closure.
pub fn snapshot_source() -> Result<String, MiseError> {
    let domains = CacheSnapshotDomain::ALL
        .into_iter()
        .map(domain_case)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(include_str!("cache_snapshot.sh")
        .replace("@DOMAINS@", &domains)
        .replace("@ENGINE@", &compiled_inventory_loader()?)
        .replace("@ENTRY@", ENTRY))
}

const ENTRY: &str = "
from source_archive_inventory import source_archive_inventory
from source_archive_inventory_common import InventoryError

# The current registry has no capability. Arguments describe the selected
# payload, but cannot create one. The shared source factory refuses before
# traversal; raw metadata and original-filesystem observations cannot export.
try:
    if len(sys.argv) != 3:
        raise InventoryError('snapshot_arguments')
    _snapshot_root = sys.argv[1]
    _snapshot_roots = json.loads(sys.argv[2])
    if (type(_snapshot_roots) is not list or not _snapshot_roots
            or any(type(root) is not str for root in _snapshot_roots)):
        raise InventoryError('snapshot_roots')
    _snapshot_context = {
        'schema': 2, 'root': _snapshot_root, 'roots': tuple(_snapshot_roots),
        'purpose': 'payload-v1',
    }
    _snapshot_result = source_archive_inventory(_snapshot_context, None)
    if json.loads(_snapshot_result.canonical_bytes)['schema'] != 2:
        raise InventoryError('snapshot_payload_schema')
    print(_snapshot_result.digest, _snapshot_result.files, _snapshot_result.bytes)
except (InventoryError, OSError, ValueError, TypeError, KeyError):
    raise SystemExit(1) from None
";

fn domain_case(domain: CacheSnapshotDomain) -> String {
    format!(
        "  {})\n    output='{}'; roots='{}'; roots_json='[\"{}\"]' ;;",
        domain.name(),
        domain.name().to_ascii_uppercase(),
        domain.roots().join(","),
        domain.roots().join("\",\""),
    )
}
