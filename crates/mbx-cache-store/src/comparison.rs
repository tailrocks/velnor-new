//! Owner-defined inventory, independent of bundle layout and object counts.
use super::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonState {
    pub version: u8,
    /// Full action-result values, keyed by the complete action digest tuple.
    pub action_results: BTreeMap<String, serde_json::Value>,
    /// Actual native adapter owner of every complete action-result key.
    pub action_owners: BTreeMap<String, String>,
    /// Full prediction tuples including their task identity.
    pub predictions: BTreeSet<String>,
    pub attachments: BTreeMap<String, CacheDigest>,
}

impl ComparisonState {
    /// The current complete owner-inventory schema version.
    pub const VERSION: u8 = 2;

    /// Validate the complete owner state shape without requiring retained blobs.
    pub fn validate(&self) -> Result<()> {
        if self.version != Self::VERSION {
            eyre::bail!("unsupported comparison version");
        }
        if self.action_owners.keys().collect::<BTreeSet<_>>()
            != self.action_results.keys().collect()
        {
            eyre::bail!("comparison ownership does not cover exactly its action results");
        }
        for owner in self.action_owners.values() {
            CapturedMetadataKind::for_adapter(owner)?;
        }
        for (key, value) in &self.action_results {
            let action: CacheDigest = serde_json::from_str(key)?;
            let result: RemoteActionResult = serde_json::from_value(value.clone())?;
            if result.version != 1
                || result.action != action
                || action.algorithm != "blake3"
                || *key != serde_json::to_string(&action)?
            {
                eyre::bail!("invalid comparison action result");
            }
            for digest in [
                Some(&action),
                result.metadata.as_ref(),
                result.output_root.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                digest.validate()?;
            }
        }
        let mut tasks: BTreeMap<String, Vec<ActionPrediction>> = BTreeMap::new();
        for value in &self.predictions {
            let (task, prediction): (String, ActionPrediction) = serde_json::from_str(value)?;
            if !is_task_identity(&task)
                || !self
                    .action_results
                    .contains_key(&serde_json::to_string(&prediction.action)?)
                || self
                    .action_owners
                    .get(&serde_json::to_string(&prediction.action)?)
                    != Some(&prediction.adapter)
            {
                eyre::bail!("invalid comparison prediction");
            }
            prediction.validate()?;
            tasks.entry(task).or_default().push(prediction);
        }
        for (task, predictions) in tasks {
            if !(TaskActionManifest {
                version: 1,
                task,
                predictions,
            })
            .validate()
            {
                eyre::bail!("invalid or conflicting comparison task predictions");
            }
        }
        for (name, digest) in &self.attachments {
            if !valid_attachment_name(name) {
                eyre::bail!("invalid comparison attachment");
            }
            digest.validate()?;
        }
        Ok(())
    }

    /// Read an owner-generated directory bundle without consuming it.
    pub fn from_directory(root: &Path) -> Result<Self> {
        validate_directory_bundle(root)?;
        Self::from_root(root)
    }

    pub(super) fn from_root(root: &Path) -> Result<Self> {
        let (manifest, actions) = read_export_manifest(root)?;
        let mut closure = strict_closure(root, &actions, &manifest.action_owners)?;
        let cas = LocalCas::new(root);
        for digest in &manifest.objects {
            require_object(&cas, &mut closure, digest)?;
        }
        verify_pending(&mut closure.pending)?;
        Self::from_manifest(root, &manifest)
    }

    pub(super) fn from_manifest(root: &Path, manifest: &ExportManifest) -> Result<Self> {
        let cache = mbx_cache_core::LocalActionCache::new(root);
        let mut action_results = BTreeMap::new();
        for action in &manifest.actions {
            let result = cache
                .find(action)?
                .ok_or_else(|| eyre::eyre!("missing action result"))?;
            action_results.insert(
                serde_json::to_string(action)?,
                serde_json::to_value(result)?,
            );
        }
        let mut predictions = BTreeSet::new();
        for task in &manifest.tasks {
            for prediction in &task.predictions {
                predictions.insert(serde_json::to_string(&(&task.task, prediction))?);
            }
        }
        Ok(Self {
            version: Self::VERSION,
            action_results,
            action_owners: manifest.action_owners.clone(),
            predictions,
            attachments: manifest.attachments.clone(),
        })
    }
}
