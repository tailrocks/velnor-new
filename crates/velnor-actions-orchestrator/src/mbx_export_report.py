# Source-owned native export ABI 2 parser. Bundle delta is not workspace usefulness.
def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError('duplicate public native report field')
        value[key] = item
    return value

def invalid_constant(value):
    raise ValueError('nonfinite public native report value')

def decode_native_report(text):
    return json.loads(text, object_pairs_hook=unique_object, parse_constant=invalid_constant)

def unsigned(value):
    return type(value) is int and 0 <= value <= 18446744073709551615

def export_report(report):
    fields = {'version', 'budget_refused', 'snapshot_budget_bytes', 'exported',
        'actions', 'objects', 'bytes', 'emitted_bundle_useful_delta',
        'workspace_usefulness', 'delta', 'semantic_digest', 'workspace_comparison',
        'workspace_comparison_exclusions', 'workspace_transport_scope',
        'workspace_capture', 'workspace_capture_unavailable_reason',
        'workspace_persistence_verified', 'qualification'}
    if type(report) is not dict or set(report) != fields:
        raise ValueError('invalid public native export field inventory')
    if type(report['version']) is not int or report['version'] != 2:
        raise ValueError('unsupported public native export ABI')
    if report['budget_refused'] is not False:
        raise ValueError('native export budget refused')
    # This caller always supplies --compare; the owner returns a budget and delta.
    if not all(unsigned(report[field]) for field in
        ('snapshot_budget_bytes', 'actions', 'objects', 'bytes')):
        raise ValueError('invalid native export unsigned counts')
    if type(report['exported']) is not bool or type(report['emitted_bundle_useful_delta']) is not bool:
        raise ValueError('invalid native bundle delta booleans')
    if report['exported'] != report['emitted_bundle_useful_delta']:
        raise ValueError('inconsistent emitted native bundle delta')
    delta = report['delta']
    delta_fields = {'new_action_results', 'changed_action_results', 'new_predictions',
        'new_workspace_variants', 'changed_workspace_variants'}
    if type(delta) is not dict or set(delta) != delta_fields or not all(unsigned(value) for value in delta.values()):
        raise ValueError('invalid native comparison delta')
    usefulness = report['workspace_usefulness']
    captures = {'captured': 'scheduler_validity_not_proven',
        'unavailable_owner_coverage': 'owner_coverage_unavailable',
        'unavailable_owner_proof': 'owner_proof_unavailable',
        'unavailable_managed_overlap': 'managed_overlap'}
    capture = report['workspace_capture']
    if type(capture) is not str or capture not in captures:
        raise ValueError('invalid native workspace capture')
    if type(usefulness) is not dict or set(usefulness) != {'status', 'reason'}:
        raise ValueError('invalid native workspace usefulness shape')
    if usefulness['status'] != 'unavailable' or usefulness['reason'] != captures[capture]:
        raise ValueError('unsupported native workspace usefulness')
    reason = report['workspace_capture_unavailable_reason']
    if (capture == 'captured' and reason is not None) or (capture != 'captured' and (type(reason) is not str or not reason)):
        raise ValueError('invalid native capture explanation')
    if report['workspace_persistence_verified'] is not False:
        raise ValueError('unsupported native workspace persistence claim')
    if report['workspace_comparison'] != 'relative_path_type_content_mode_symlink_target' or report['workspace_comparison_exclusions'] != ['effective_build_root/.rustc_info.json'] or report['workspace_transport_scope'] != 'recorded_target_and_build_directories':
        raise ValueError('unsupported native workspace comparison scope')
    digest = report['semantic_digest']
    if type(digest) is not str or not re.fullmatch('[0-9a-f]{64}', digest):
        raise ValueError('invalid native export semantic digest')
    if type(report['qualification']) is not str or not report['qualification']:
        raise ValueError('missing native export qualification explanation')
    return report['emitted_bundle_useful_delta'], digest, usefulness['reason']
