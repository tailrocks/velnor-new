use velnor_actions_contract::Step;

/// Return an ordered longest common subsequence using linear auxiliary space.
///
/// The inputs retain each step's source index so callers can keep unmatched
/// lane-specific steps in their original jobs. Hirschberg reconstruction
/// avoids the quadratic length table for large workflows.
pub(crate) fn ordered_matches(
    hosted: &[(usize, &Step)],
    local: &[(usize, &Step)],
) -> Vec<(usize, usize)> {
    if hosted.len() == local.len()
        && hosted
            .iter()
            .zip(local)
            .all(|(hosted_step, local_step)| hosted_step.1 == local_step.1)
    {
        return hosted
            .iter()
            .zip(local)
            .map(|(hosted_step, local_step)| (hosted_step.0, local_step.0))
            .collect();
    }
    let mut matches = Vec::new();
    collect_matches(hosted, local, &mut matches);
    matches
}

fn collect_matches(
    hosted: &[(usize, &Step)],
    local: &[(usize, &Step)],
    matches: &mut Vec<(usize, usize)>,
) {
    if hosted.is_empty() || local.is_empty() {
        return;
    }
    let mut prefix_len = 0;
    while prefix_len < hosted.len()
        && prefix_len < local.len()
        && hosted[prefix_len].1 == local[prefix_len].1
    {
        matches.push((hosted[prefix_len].0, local[prefix_len].0));
        prefix_len += 1;
    }
    if prefix_len > 0 {
        collect_matches(&hosted[prefix_len..], &local[prefix_len..], matches);
        return;
    }
    if hosted.len() == 1 {
        if let Some(local_index) = local.iter().position(|(_, step)| *step == hosted[0].1) {
            matches.push((hosted[0].0, local[local_index].0));
        }
        return;
    }
    if local.len() == 1 {
        if let Some(hosted_index) = hosted.iter().position(|(_, step)| *step == local[0].1) {
            matches.push((hosted[hosted_index].0, local[0].0));
        }
        return;
    }

    let middle = hosted.len() / 2;
    let split = split_index(&hosted[..middle], &hosted[middle..], local);
    collect_matches(&hosted[..middle], &local[..split], matches);
    collect_matches(&hosted[middle..], &local[split..], matches);
}

fn split_index(
    hosted_left: &[(usize, &Step)],
    hosted_right: &[(usize, &Step)],
    local: &[(usize, &Step)],
) -> usize {
    let prefix_lengths = prefix_lengths(hosted_left, local);
    let suffix_lengths = suffix_lengths(hosted_right, local);
    let mut best_length = 0;
    let mut best_split = 0;
    for split in 0..=local.len() {
        let length = prefix_lengths[split] + suffix_lengths[split];
        // Ascending scan plus strict comparison keeps the earliest split on
        // ties, yielding deterministic matches for repeated steps.
        if length > best_length {
            best_length = length;
            best_split = split;
        }
    }
    best_split
}

fn prefix_lengths(hosted: &[(usize, &Step)], local: &[(usize, &Step)]) -> Vec<usize> {
    let mut previous = vec![0; local.len() + 1];
    let mut current = vec![0; local.len() + 1];
    for (_, hosted_step) in hosted {
        current[0] = 0;
        for (index, (_, local_step)) in local.iter().enumerate() {
            current[index + 1] = if hosted_step == local_step {
                previous[index] + 1
            } else {
                previous[index + 1].max(current[index])
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous
}

fn suffix_lengths(hosted: &[(usize, &Step)], local: &[(usize, &Step)]) -> Vec<usize> {
    let mut previous = vec![0; local.len() + 1];
    let mut current = vec![0; local.len() + 1];
    for (_, hosted_step) in hosted.iter().rev() {
        current[local.len()] = 0;
        for index in (0..local.len()).rev() {
            current[index] = if hosted_step == &local[index].1 {
                previous[index + 1] + 1
            } else {
                previous[index].max(current[index + 1])
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous
}
