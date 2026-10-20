impl TufCache {
    fn prepare_root(&self) -> io::Result<()> {
        validate_path_for_creation(&self.root)?;
        let created = match fs::create_dir(&self.root) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(error),
        };
        reject_real_private_directory(&self.root, created)?;
        let generations = self.root.join(GENERATIONS_DIR);
        validate_path_for_creation(&generations)?;
        let created = match fs::create_dir(&generations) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(error),
        };
        reject_real_private_directory(&generations, created)
    }

    fn read_active(&self) -> io::Result<Option<ActiveGeneration>> {
        let current_path = self.root.join(CURRENT_FILE);
        let pointer = match read_regular_file(&current_path, 128) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return self.read_empty_cache();
            }
            Err(error) => return Err(error),
        };
        self.read_generation(&pointer)
    }

    fn read_empty_cache(&self) -> io::Result<Option<ActiveGeneration>> {
        let allowed = BTreeSet::from([LOCK_FILE.to_owned(), GENERATIONS_DIR.to_owned()]);
        for entry in fs::read_dir(&self.root)? {
            if !allowed.contains(&entry?.file_name().to_string_lossy().into_owned()) {
                return Err(io::Error::other("cache without CURRENT is not empty"));
            }
        }
        if fs::read_dir(self.root.join(GENERATIONS_DIR))?.next().is_some() {
            return Err(io::Error::other("cache has generation without CURRENT"));
        }
        Ok(None)
    }

    fn read_generation(&self, pointer: &[u8]) -> io::Result<Option<ActiveGeneration>> {
        let id = std::str::from_utf8(pointer)
            .map_err(|_| io::Error::other("invalid CURRENT encoding"))?
            .trim_end_matches('\n');
        if !valid_generation_id(id) || pointer != format!("{id}\n").as_bytes() {
            return Err(io::Error::other("invalid CURRENT generation id"));
        }
        let path = self.root.join(GENERATIONS_DIR).join(id);
        reject_real_private_directory(&path, false)?;
        let manifest: GenerationManifest =
            serde_json::from_slice(&read_bounded(&path.join(MANIFEST_FILE), MAX_STATE_FILE_BYTES)?)?;
        Self::validate_generation_manifest(&manifest, id, &path)?;
        let state: RootHighWater =
            serde_json::from_slice(&read_bounded(&path.join(STATE_FILE), MAX_STATE_FILE_BYTES)?)?;
        Self::validate_saved_root(&manifest, &state, &path)?;
        Ok(Some(ActiveGeneration { id: id.to_owned(), path, state }))
    }

    fn validate_generation_manifest(
        manifest: &GenerationManifest,
        id: &str,
        path: &Path,
    ) -> io::Result<()> {
        if manifest.schema != 1 || manifest.generation != id {
            return Err(io::Error::other("generation manifest identity mismatch"));
        }
        if hash_tree_files(path)? != manifest.files {
            return Err(io::Error::other("generation inventory or digest mismatch"));
        }
        Ok(())
    }

    fn validate_saved_root(
        manifest: &GenerationManifest,
        state: &RootHighWater,
        path: &Path,
    ) -> io::Result<()> {
        if state.schema != 1 || manifest_tuf_files(manifest)? != state.tuf_files {
            return Err(io::Error::other("root high-water metadata digest mismatch"));
        }
        let root_bytes = read_bounded(&path.join(TUF_DIR).join("root.json"), MAX_METADATA_FILE_BYTES)?;
        let identity = RootIdentity::from_bytes(&root_bytes).map_err(io::Error::other)?;
        if identity.version != state.root_version || identity.signed_sha256 != state.root_signed_sha256 {
            return Err(io::Error::other("stored root differs from root high-water record"));
        }
        Ok(())
    }

    fn create_staging(&self, active: Option<&ActiveGeneration>) -> io::Result<StagedGeneration> {
        let id = unique_generation_id();
        let generation = StagedGeneration {
            path: self.root.join(GENERATIONS_DIR).join(format!("{id}.stage")),
            id,
            previous: active.map(|value| value.id.clone()),
        };
        fs::create_dir(&generation.path)?;
        if let Err(error) = Self::prepare_staging(&generation, active) {
            Self::remove_staging(&generation)?;
            return Err(error);
        }
        Ok(generation)
    }

    fn prepare_staging(
        generation: &StagedGeneration,
        active: Option<&ActiveGeneration>,
    ) -> io::Result<()> {
        fs::set_permissions(&generation.path, fs::Permissions::from_mode(0o700))?;
        let tuf_path = generation.path.join(TUF_DIR);
        fs::create_dir(&tuf_path)?;
        fs::set_permissions(&tuf_path, fs::Permissions::from_mode(0o700))?;
        if let Some(active) = active {
            Self::copy_active_metadata(active, &tuf_path)?;
        }
        Ok(())
    }

    fn remove_staging(generation: &StagedGeneration) -> io::Result<()> {
        match fs::symlink_metadata(&generation.path) {
            Ok(metadata) => {
                require_owned_directory(&metadata, effective_uid())?;
                fs::remove_dir_all(&generation.path)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn copy_active_metadata(active: &ActiveGeneration, destination_root: &Path) -> io::Result<()> {
        let source_root = active.path.join(TUF_DIR);
        for (relative, _) in hash_tree_files(&source_root)? {
            let bytes = read_bounded(&source_root.join(&relative), MAX_METADATA_FILE_BYTES)?;
            let destination = destination_root.join(&relative);
            if let Some(parent) = destination.parent()
                && parent != destination_root
            {
                fs::create_dir_all(parent)?;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
            write_file(&destination, &bytes, 0o600)?;
        }
        Ok(())
    }

    fn publish(&self, generation: &StagedGeneration) -> io::Result<()> {
        self.place_generation(generation)?;
        self.activate_generation(&generation.id)
    }

    fn place_generation(&self, generation: &StagedGeneration) -> io::Result<()> {
        let final_path = self.root.join(GENERATIONS_DIR).join(&generation.id);
        fs::rename(&generation.path, final_path)?;
        sync_directory(&self.root.join(GENERATIONS_DIR))
    }

    fn activate_generation(&self, id: &str) -> io::Result<()> {
        let pointer_temp = self.root.join(format!("CURRENT.tmp-{id}"));
        write_file(&pointer_temp, format!("{id}\n").as_bytes(), 0o600)?;
        fs::rename(pointer_temp, self.root.join(CURRENT_FILE))?;
        sync_directory(&self.root)
    }

    fn cleanup_generations(&self, active: &str, previous: Option<&str>) -> io::Result<()> {
        for entry in fs::read_dir(self.root.join(GENERATIONS_DIR))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == active || previous.is_some_and(|old| name == old) {
                continue;
            }
            fs::remove_dir_all(entry.path())?;
        }
        Ok(())
    }
}
