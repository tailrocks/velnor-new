//! Closed source asset preload; caller owns qualified Foundation execution.

use crate::MiseError;
use crate::source_archive_inventory::{
    InventorySourceProgram, fixed_sources, source_template_sha256,
};
use serde_json::json;
use velnor_actions_contract::compiled_source_sha256;

/// Compile the fixed eight-module inventory preload without an entrypoint.
///
/// All source bytes and ordered closure identities are checked before any asset
/// executes. This source grants no host qualification or inventory capability.
///
/// # Errors
/// Returns [`MiseError::Contract`] if compiler-owned literals cannot be encoded.
pub fn compiled_inventory_loader() -> Result<String, MiseError> {
    let program = InventorySourceProgram::OriginalFilesystem;
    let sources = fixed_sources(program);
    let order = sources.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let records = sources
        .iter()
        .map(|(name, source)| {
            json!({"name": name, "source": source,
                "sha256": compiled_source_sha256(source.as_bytes())})
        })
        .collect::<Vec<_>>();
    let order = encode(&order)?;
    let capsule = encode(&json!({"schema": 1, "modules": records}))?;
    Ok(format!(
        "import hashlib, json, sys, types\n\
         _INVENTORY_ORDER_HEX = '{order}'\n\
         _INVENTORY_CAPSULE_HEX = '{capsule}'\n\
         _INVENTORY_TEMPLATE_SHA256 = '{}'\n{}",
        source_template_sha256(program),
        PRELOAD,
    ))
}

fn encode(value: &impl serde::Serialize) -> Result<String, MiseError> {
    let bytes = serde_json::to_vec(value).map_err(|error| MiseError::Contract {
        problem: format!("inventory_source_literals: {error}"),
    })?;
    let digits = b"0123456789abcdef";
    Ok(bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(digits[usize::from(byte >> 4)]),
                char::from(digits[usize::from(byte & 0x0f)]),
            ]
        })
        .collect::<String>())
}

const PRELOAD: &str = "
def _inventory_unique(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError('inventory_source_duplicate_field')
        value[key] = item
    return value

def _inventory_frame(digest, value):
    digest.update(len(value).to_bytes(8, 'big'))
    digest.update(value)

_inventory_order = json.loads(bytes.fromhex(_INVENTORY_ORDER_HEX))
_inventory_capsule = json.loads(bytes.fromhex(_INVENTORY_CAPSULE_HEX),
                               object_pairs_hook=_inventory_unique)
if (type(_inventory_order) is not list or len(_inventory_order) != 8
        or any(type(name) is not str for name in _inventory_order)
        or len(set(_inventory_order)) != 8
        or type(_inventory_capsule) is not dict
        or set(_inventory_capsule) != {'schema', 'modules'}
        or type(_inventory_capsule['schema']) is not int
        or _inventory_capsule['schema'] != 1
        or type(_inventory_capsule['modules']) is not list
        or len(_inventory_capsule['modules']) != 8):
    raise ValueError('inventory_source_capsule')
_inventory_digest = hashlib.sha256()
_inventory_frame(_inventory_digest, b'velnor-source-archive-inventory-template-v1')
_inventory_digest.update(len(_inventory_order).to_bytes(8, 'big'))
for _inventory_index, _inventory_record in enumerate(_inventory_capsule['modules']):
    if (type(_inventory_record) is not dict
            or set(_inventory_record) != {'name', 'source', 'sha256'}
            or type(_inventory_record['name']) is not str
            or _inventory_record['name'] != _inventory_order[_inventory_index]
            or type(_inventory_record['source']) is not str
            or not _inventory_record['source']
            or type(_inventory_record['sha256']) is not str):
        raise ValueError('inventory_source_asset')
    _inventory_bytes = _inventory_record['source'].encode('utf-8')
    if hashlib.sha256(_inventory_bytes).hexdigest() != _inventory_record['sha256']:
        raise ValueError('inventory_source_asset_digest')
    _inventory_frame(_inventory_digest, _inventory_record['name'].encode('utf-8'))
    _inventory_frame(_inventory_digest, _inventory_bytes)
if _inventory_digest.hexdigest() != _INVENTORY_TEMPLATE_SHA256:
    raise ValueError('inventory_source_template_digest')
for _inventory_name in _inventory_order:
    _inventory_module = types.ModuleType(_inventory_name)
    _inventory_module.__file__ = '<compiled-source:' + _inventory_name + '>'
    sys.modules[_inventory_name] = _inventory_module
for _inventory_record in _inventory_capsule['modules']:
    _inventory_name = _inventory_record['name']
exec(compile(_inventory_record['source'], '<compiled-source:' + _inventory_name + '>', 'exec'),
     sys.modules[_inventory_name].__dict__)
del _inventory_capsule, _INVENTORY_CAPSULE_HEX
";
