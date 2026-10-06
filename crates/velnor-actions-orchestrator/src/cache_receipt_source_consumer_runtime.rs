//! Consumer activation source never enters producer identity or signed policy.
pub(super) const LOADER: &str = r#"
_ORDER = ('cache_receipt_common', 'cache_receipt_policy', 'opaque_inventory_metadata',
          'metadata_container', 'source_archive_inventory_common',
          'source_archive_inventory_fs', 'source_archive_inventory_leaf',
          'source_archive_inventory_walk', 'source_archive_inventory',
          'cache_receipt_manifest', 'cache_receipt_virtual',
          'cache_receipt_api', 'cache_receipt_gh', 'receipt_fresh_gh_download',
          'receipt_fresh_gh_archive', 'receipt_fresh_gh', 'cache_receipt',
          'cache_receipt_materialize_transaction', 'cache_receipt_materialize')
if set(_SOURCES) != set(_ORDER):
    raise RuntimeError('receipt_source_registry')
for _name in _ORDER:
    _module = types.ModuleType(_name)
    _module.__file__ = '<compiled-source:' + _name + '>'
    sys.modules[_name] = _module
for _name in _ORDER:
    exec(compile(_SOURCES[_name], '<compiled-source:' + _name + '>', 'exec'),
         sys.modules[_name].__dict__)
del _SOURCES
"#;

pub(super) const ENTRYPOINT: &str = r#"
import os
from cache_receipt_common import ColdReceipt

# No actual immutable callee/caller/protected-source publication is qualified.
def _verify():
    raise ColdReceipt('producer_policy_unqualified')

try:
    _verify()
except (ColdReceipt, OSError, KeyError, ValueError, RecursionError):
    _output = os.environ.get('GITHUB_OUTPUT')
    if _output:
        with open(_output, 'a', encoding='ascii') as _stream:
            _stream.write('verified=false\nerror=CACHE_RECEIPT_UNQUALIFIED\n')
    sys.exit(0)
"#;
