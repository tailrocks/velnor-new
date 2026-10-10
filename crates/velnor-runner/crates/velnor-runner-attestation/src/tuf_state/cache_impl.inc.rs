struct RefreshBase {
    starting_root: Vec<u8>,
    prior_state: Option<RootHighWater>,
    bootstrap_hash: String,
    embedded_identity: RootIdentity,
    starting_identity: RootIdentity,
}

impl TufCache {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub(crate) async fn refresh<T, F, V>(
        &self,
        request: TufRefreshRequest<'_, T, F>,
    ) -> Result<TufRefresh<V>, Box<dyn Error>>
    where
        T: Transport + Send + Sync + 'static,
        F: for<'target> FnOnce(Option<&'target [u8]>) -> Result<V, Box<dyn Error>>,
    {
        self.prepare_root()?;
        let _lock = CacheLock::acquire(&self.root, LOCK_WAIT).await?;
        let active = self.read_active()?;
        let base = Self::resolve_starting_root(request.bootstrap, request.migration, active.as_ref())?;
        let generation = self.create_staging(active.as_ref())?;
        match Self::finish_generation(self, &base, &generation, request).await {
            Ok(result) => Ok(result),
            Err(error) => {
                Self::remove_staging(&generation)?;
                Err(error)
            }
        }
    }

    async fn finish_generation<T, F, V>(
        cache: &Self,
        base: &RefreshBase,
        generation: &StagedGeneration,
        request: TufRefreshRequest<'_, T, F>,
    ) -> Result<TufRefresh<V>, Box<dyn Error>>
    where
        T: Transport + Send + Sync + 'static,
        F: for<'target> FnOnce(Option<&'target [u8]>) -> Result<V, Box<dyn Error>>,
    {
        let TufRefreshRequest {
            metadata_url,
            targets_url,
            transport,
            capture,
            target_name,
            validate_target,
            ..
        } = request;
        let tuf_path = generation.path.join(TUF_DIR);
        let (repository, root_chain) = cache
            .load_and_verify(base, generation, metadata_url, targets_url, transport, capture)
            .await?;
        let target_value = Self::read_target(&repository, target_name, validate_target).await?;
        cache.commit_generation(generation, base, &root_chain, &tuf_path)?;
        Ok(TufRefresh {
            #[cfg(test)]
            repository,
            #[cfg(test)]
            root_chain,
            target_value,
        })
    }

    fn resolve_starting_root(
        bootstrap: &[u8],
        migration: Option<&BootstrapMigration>,
        active: Option<&ActiveGeneration>,
    ) -> Result<RefreshBase, Box<dyn Error>> {
        let bootstrap_hash = sha256_hex(bootstrap);
        let embedded_identity = RootIdentity::from_bytes(bootstrap).map_err(io::Error::other)?;
        let (starting_root, prior_state) = if let Some(active) = active {
            let root_path = active.path.join(TUF_DIR).join("root.json");
            let root_bytes = read_bounded(&root_path, MAX_METADATA_FILE_BYTES)?;
            let saved_identity = RootIdentity::from_bytes(&root_bytes).map_err(io::Error::other)?;
            let state = active.state.clone();
            let state_identity = RootIdentity {
                version: state.root_version,
                signed_sha256: state.root_signed_sha256.clone(),
            };
            if saved_identity != state_identity {
                return Err(io::Error::other("saved root differs from root high-water state").into());
            }
            Self::validate_bootstrap(&state, migration, &bootstrap_hash, &embedded_identity)?;
            (root_bytes, Some(state))
        } else {
            if migration.is_some() {
                return Err(io::Error::other("bootstrap migration requires existing state").into());
            }
            (bootstrap.to_vec(), None)
        };
        let starting_identity = RootIdentity::from_bytes(&starting_root).map_err(io::Error::other)?;
        Ok(RefreshBase {
            starting_root,
            prior_state,
            bootstrap_hash,
            embedded_identity,
            starting_identity,
        })
    }

    async fn load_and_verify<T>(
        &self,
        base: &RefreshBase,
        generation: &StagedGeneration,
        metadata_url: Url,
        targets_url: Url,
        transport: T,
        capture: &RootResponseCapture,
    ) -> Result<(Repository, VerifiedRootChain), Box<dyn Error>>
    where
        T: Transport + Send + Sync + 'static,
    {
        let tuf_path = generation.path.join(TUF_DIR);
        let repository = self
            .load_repository(
                &base.starting_root,
                metadata_url,
                targets_url,
                transport,
                &tuf_path,
            )
            .await?;
        secure_tree(&generation.path)?;
        let captured = capture.take().map_err(io::Error::other)?;
        let chain = verify_composite_chain(&base.starting_identity, &captured, repository.root())
            .map_err(io::Error::other)?;
        Self::validate_migrated_bootstrap(base, &chain)?;
        Ok((repository, chain))
    }

    fn validate_migrated_bootstrap(
        base: &RefreshBase,
        chain: &VerifiedRootChain,
    ) -> io::Result<()> {
        if let Some(previous) = base.prior_state.as_ref()
            && base.bootstrap_hash != previous.bootstrap_sha256
            && !chain.identities.contains(&base.embedded_identity)
        {
            return Err(io::Error::other(
                "new embedded bootstrap is absent from authenticated root chain",
            ));
        }
        Ok(())
    }

    async fn read_target<F, V>(
        repository: &Repository,
        target_name: Option<&TargetName>,
        validate_target: F,
    ) -> Result<V, Box<dyn Error>>
    where
        F: for<'target> FnOnce(Option<&'target [u8]>) -> Result<V, Box<dyn Error>>,
    {
        let target_bytes = match target_name {
            Some(target_name) => {
                let target = repository
                    .read_target(target_name)
                    .await?
                    .ok_or_else(|| io::Error::other("authenticated TUF target missing"))?;
                let bytes = target.into_vec().await?;
                if bytes.len() as u64 > MAX_METADATA_FILE_BYTES {
                    return Err(io::Error::other("authenticated TUF target exceeds byte limit").into());
                }
                Some(bytes)
            }
            None => None,
        };
        validate_target(target_bytes.as_deref())
    }

    fn commit_generation(
        &self,
        generation: &StagedGeneration,
        base: &RefreshBase,
        chain: &VerifiedRootChain,
        tuf_path: &Path,
    ) -> io::Result<()> {
        let high_water = RootHighWater {
            schema: 1,
            bootstrap_sha256: base.bootstrap_hash.clone(),
            bootstrap_version: base.embedded_identity.version,
            bootstrap_signed_sha256: base.embedded_identity.signed_sha256.clone(),
            root_version: chain.final_identity.version,
            root_signed_sha256: chain.final_identity.signed_sha256.clone(),
            tuf_files: hash_tuf_files(tuf_path)?,
        };
        Self::validate_high_water(base.prior_state.as_ref(), chain, &high_water)?;
        write_file(
            &generation.path.join(STATE_FILE),
            &serde_json::to_vec(&high_water).map_err(io::Error::other)?,
            0o600,
        )?;
        let manifest = GenerationManifest {
            schema: 1,
            generation: generation.id.clone(),
            files: hash_tree_files(&generation.path)?,
        };
        write_file(
            &generation.path.join(MANIFEST_FILE),
            &serde_json::to_vec(&manifest).map_err(io::Error::other)?,
            0o600,
        )?;
        sync_tree(&generation.path)?;
        self.publish(generation)?;
        self.cleanup_generations(&generation.id, generation.previous.as_deref())
    }

    fn validate_high_water(
        prior: Option<&RootHighWater>,
        chain: &VerifiedRootChain,
        candidate: &RootHighWater,
    ) -> io::Result<()> {
        if candidate.root_version < chain.identities[0].version {
            return Err(io::Error::other("root high-water moved backwards"));
        }
        if prior.is_some_and(|state| candidate.root_version < state.root_version) {
            return Err(io::Error::other("root high-water moved backwards"));
        }
        Ok(())
    }

    async fn load_repository<T>(
        &self,
        root: &[u8],
        metadata_url: Url,
        targets_url: Url,
        transport: T,
        tuf_path: &Path,
    ) -> Result<Repository, Box<dyn Error>>
    where
        T: Transport + Send + Sync + 'static,
    {
        let root_bytes = root.to_vec();
        let loader = RepositoryLoader::new(&root_bytes, metadata_url, targets_url)
            .transport(transport)
            .limits(Limits::default())
            .expiration_enforcement(ExpirationEnforcement::Safe)
            .datastore(tuf_path);
        Ok(loader.load().await?)
    }

    fn validate_bootstrap(
        state: &RootHighWater,
        migration: Option<&BootstrapMigration>,
        bootstrap_hash: &str,
        embedded_identity: &RootIdentity,
    ) -> Result<(), Box<dyn Error>> {
        let current_bootstrap = RootIdentity {
            version: state.bootstrap_version,
            signed_sha256: state.bootstrap_signed_sha256.clone(),
        };
        if state.bootstrap_sha256 == bootstrap_hash {
            if current_bootstrap != *embedded_identity || migration.is_some() {
                return Err(io::Error::other("bootstrap identity mismatch").into());
            }
            return Ok(());
        }
        let migration = migration
            .ok_or_else(|| io::Error::other("bootstrap change requires reviewed migration"))?;
        if migration.from_sha256 != state.bootstrap_sha256
            || migration.from_root != current_bootstrap
            || migration.to_sha256 != bootstrap_hash
            || migration.to_root != *embedded_identity
        {
            return Err(io::Error::other("bootstrap migration identity mismatch").into());
        }
        let saved_root = RootIdentity {
            version: state.root_version,
            signed_sha256: state.root_signed_sha256.clone(),
        };
        if embedded_identity.version < saved_root.version
            || (embedded_identity.version == saved_root.version && embedded_identity != &saved_root)
        {
            return Err(io::Error::other("bootstrap migration would downgrade saved root").into());
        }
        if current_bootstrap.version < 1 {
            return Err(io::Error::other("invalid prior bootstrap version").into());
        }
        Ok(())
    }
}
