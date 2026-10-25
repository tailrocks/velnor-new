use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::effective::{Dialect, config_shape};
use crate::parser::{FileModel, MAX_FILE_BYTES, parse_json, parse_native};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadFailure {
    kind: std::io::ErrorKind,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinnedOutcome {
    Absent,
    Text(String),
    Unreadable(String),
}

#[derive(Debug, Default)]
pub struct FileCache {
    raw: HashMap<PathBuf, Result<Vec<u8>, ReadFailure>>,
    walks: HashMap<(PathBuf, String), Result<Vec<String>, String>>,
    models: HashMap<PathBuf, Result<Option<FileModel>, String>>,
    pinned: HashMap<PathBuf, PinnedOutcome>,
}

impl FileCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_raw(&mut self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        if let Some(hit) = self.raw.get(path) {
            return hit
                .clone()
                .map_err(|failure| std::io::Error::new(failure.kind, failure.message.clone()));
        }
        if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            let refused = std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("symlink_refused:{}", path.display()),
            );
            self.raw.insert(
                path.to_path_buf(),
                Err(ReadFailure {
                    kind: refused.kind(),
                    message: refused.to_string(),
                }),
            );
            return Err(refused);
        }
        match std::fs::read(path) {
            Ok(bytes) => {
                if u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= MAX_FILE_BYTES {
                    self.raw.insert(path.to_path_buf(), Ok(bytes.clone()));
                }
                Ok(bytes)
            }
            Err(err) => {
                self.raw.insert(
                    path.to_path_buf(),
                    Err(ReadFailure {
                        kind: err.kind(),
                        message: err.to_string(),
                    }),
                );
                Err(err)
            }
        }
    }

    pub(crate) fn unit_files(&mut self, root: &Path, unit: &str) -> Result<Vec<String>, String> {
        if let Some(hit) = self.walks.get(&(root.to_path_buf(), unit.to_owned())) {
            return hit.clone();
        }
        let outcome = crate::closure::collect_unit_files(root, unit);
        self.walks
            .insert((root.to_path_buf(), unit.to_owned()), outcome.clone());
        outcome
    }

    pub(crate) fn model_for(
        &mut self,
        root: &Path,
        path: &str,
    ) -> Result<Option<FileModel>, String> {
        let joined = root.join(path);
        if let Some(hit) = self.models.get(&joined) {
            return hit.clone();
        }
        let outcome = self.parse_model_live(root, path);
        self.models.insert(joined, outcome.clone());
        outcome
    }

    fn parse_model_live(&mut self, root: &Path, path: &str) -> Result<Option<FileModel>, String> {
        let bytes = match self.read_raw(&root.join(path)) {
            Ok(bytes) => bytes,
            Err(err) => return Err(format!("unreadable:{path}:{err}")),
        };
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(err) => return Err(format!("unreadable:{path}:{err}")),
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        let Some(shape) = config_shape(name) else {
            return Ok(None);
        };
        let model = match shape.dialect {
            Dialect::Native => parse_native(&text),
            Dialect::Json => parse_json(&text),
        }
        .map_err(|err| format!("malformed:{path}:{err}"))?;
        Ok(Some(model))
    }

    #[must_use]
    pub fn pinned(&self, path: &Path) -> Option<PinnedOutcome> {
        self.pinned.get(path).cloned()
    }

    pub fn store_pinned(&mut self, path: PathBuf, outcome: PinnedOutcome) {
        self.pinned.insert(path, outcome);
    }
}
