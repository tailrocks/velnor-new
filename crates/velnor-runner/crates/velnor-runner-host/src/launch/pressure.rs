//! Test oracle for fair header allocation.
//!
//! The host does not use this allocator. It must persist its cursor before it uses this policy.

/// Allocate a target across active connections and rotate the remainder.
#[must_use]
pub(crate) fn allocate_headers(active: &[bool], total: u32, cursor: &mut usize) -> Vec<u32> {
    let mut headers = vec![0; active.len()];
    if active.is_empty() {
        *cursor = 0;
        return headers;
    }
    let active_count = active.iter().filter(|enabled| **enabled).count();
    if active_count == 0 || total == 0 {
        *cursor %= active.len();
        return headers;
    }
    let divisor = u32::try_from(active_count).unwrap_or(u32::MAX);
    let each = total / divisor;
    let mut remainder = total % divisor;
    for (index, enabled) in active.iter().enumerate() {
        if *enabled {
            headers[index] = each;
        }
    }
    let start = *cursor % active.len();
    let mut last = start;
    for offset in 0..active.len() {
        if remainder == 0 {
            break;
        }
        let index = (start + offset) % active.len();
        if active[index] {
            headers[index] += 1;
            remainder -= 1;
            last = index;
        }
    }
    *cursor = next_active(active, last);
    headers
}

fn next_active(active: &[bool], last: usize) -> usize {
    (1..=active.len())
        .map(|offset| (last + offset) % active.len())
        .find(|index| active[*index])
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::allocate_headers;

    #[test]
    fn a_small_target_rotates_across_connections() {
        let active = [true, true, false];
        let mut cursor = 0;
        assert_eq!(allocate_headers(&active, 1, &mut cursor), [1, 0, 0]);
        assert_eq!(allocate_headers(&active, 1, &mut cursor), [0, 1, 0]);
        assert_eq!(allocate_headers(&active, 1, &mut cursor), [1, 0, 0]);
    }

    #[test]
    fn drained_connections_receive_no_header() {
        let mut cursor = 1;
        assert_eq!(
            allocate_headers(&[true, false, true], 1, &mut cursor),
            [0, 0, 1]
        );
        assert_eq!(allocate_headers(&[false, false], 4, &mut cursor), [0, 0]);
    }
}
