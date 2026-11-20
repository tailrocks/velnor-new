// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use super::super::archive::Scanned;
use super::super::json::is_digest;
use super::super::layer::{Budget, Layer, scan_shared};
use super::{BlobSource, Descriptor, MAX_LAYER_MEMBERS, MAX_UNCOMPRESSED, fail};
use crate::error::CiError;

pub(super) fn diff_ids(config: &Value, layers: usize) -> Result<Vec<String>, CiError> {
    let ids = config
        .get("rootfs")
        .filter(|rootfs| rootfs.get("type") == Some(&json!("layers")))
        .and_then(|rootfs| rootfs.get("diff_ids"))
        .and_then(Value::as_array)
        .filter(|ids| ids.len() == layers && ids.iter().all(is_digest));
    match ids {
        Some(ids) => Ok(ids
            .iter()
            .map(|id| id.as_str().unwrap_or_default().to_owned())
            .collect()),
        None => fail("OCI image configuration has incorrect DiffID data for its manifest layers."),
    }
}

pub(super) fn check_layers(
    scan: &Scanned,
    source: &dyn BlobSource,
    entries: &[Descriptor],
    diff_ids: &[String],
) -> Result<(), CiError> {
    let budget = Budget::new(MAX_UNCOMPRESSED, MAX_LAYER_MEMBERS);
    let results = run_layers(scan, source, entries, &budget);
    for ((entry, expected), result) in entries.iter().zip(diff_ids).zip(results) {
        let layer = result?;
        if format!("sha256:{}", layer.diff_id) != *expected {
            return fail(
                "OCI layer DiffID is not the same as the digest of its uncompressed bytes.",
            );
        }
        if layer.blob_sha != scan.blobs[&entry.hex].sha {
            return fail(
                "OCI blob bytes are not the same as the bytes identified by their descriptor.",
            );
        }
    }
    Ok(())
}

fn run_layers(
    scan: &Scanned,
    source: &dyn BlobSource,
    entries: &[Descriptor],
    budget: &Budget,
) -> Vec<Result<Layer, CiError>> {
    let slots: Vec<Mutex<Option<Result<Layer, CiError>>>> =
        entries.iter().map(|_| Mutex::new(None)).collect();
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(4)
        .min(entries.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(entry) = entries.get(index) else {
                        break;
                    };
                    let outcome = one_layer(scan, source, entry, budget);
                    if let Ok(mut slot) = slots[index].lock() {
                        *slot = Some(outcome);
                    }
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .ok()
                .flatten()
                .unwrap_or_else(|| fail("OCI layer was not checked."))
        })
        .collect()
}

fn one_layer(
    scan: &Scanned,
    source: &dyn BlobSource,
    entry: &Descriptor,
    budget: &Budget,
) -> Result<Layer, CiError> {
    let blob = &scan.blobs[&entry.hex];
    let stream = source
        .open(blob)
        .map_err(|_| CiError::new("Cannot read OCI archive."))?;
    scan_shared(stream, entry.media_type.ends_with("+gzip"), budget)
}
