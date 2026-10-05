from oci_digest import *
import hashlib
import io
import stat
import zipfile


def platform_of(descriptor):
    platform = descriptor.get("platform") if isinstance(descriptor, dict) else None
    if not isinstance(platform, dict):
        return None
    os_name, arch = platform.get("os"), platform.get("architecture")
    if os_name != "linux" or not safe_arch(arch):
        return None
    extra = set(platform) - {"os", "architecture"}
    need(not (extra - {"variant"}), "platform_shape")
    if "variant" in extra:
        need(arch == "arm64" and platform.get("variant") in {"v8", None}, "platform_variant")
    return arch


def verify_recovery_attestation(image, digest, repository, source, ref):
    registry = required("REGISTRY")
    expected_name = image if image.startswith(registry + "/") else registry + "/" + image
    command = [
        "gh", "attestation", "verify", "oci://" + expected_name + "@" + digest,
        "--repo", repository,
        "--source-digest", source,
        "--source-ref", ref,
        "--signer-workflow", repository + "/.github/workflows/delivery-oci.yml",
        "--predicate-type", "https://slsa.dev/provenance/v1",
        "--format", "json",
        "--deny-self-hosted-runners",
    ]
    try:
        result = json_document(run(command), "recovery_attestation_json")
    except GateError as error:
        raise GateError("recovery_attestation") from error
    need(isinstance(result, list) and result, "recovery_attestation_empty")
    subject_found = False
    for item in result:
        verification = item.get("verificationResult") if isinstance(item, dict) else None
        statement = verification.get("statement") if isinstance(verification, dict) else None
        need(isinstance(statement, dict), "recovery_attestation_statement")
        need(statement.get("predicateType") == "https://slsa.dev/provenance/v1", "recovery_predicate_type")
        subjects = statement.get("subject")
        need(isinstance(subjects, list) and subjects, "recovery_attestation_subject")
        for subject in subjects:
            digests = subject.get("digest") if isinstance(subject, dict) else None
            need(isinstance(subject, dict) and subject.get("name") == expected_name, "recovery_attestation_subject_name")
            need(isinstance(digests, dict) and digests.get("sha256") == digest[7:], "recovery_attestation_subject")
            subject_found = True
    need(subject_found, "recovery_attestation_subject")


def metadata_entry(document, arch, field, code):
    need(isinstance(document, dict) and document, code)
    if field in document:
        value = document[field]
    else:
        candidates = [document.get("linux/" + arch)]
        if arch == "arm64":
            candidates.append(document.get("linux/arm64/v8"))
        candidates = [item for item in candidates if item is not None]
        need(len(candidates) == 1, code)
        value = candidates[0].get(field) if isinstance(candidates[0], dict) else None
    need(isinstance(value, dict) and value, code)
    return value


def validate_attestation_predicates(image, parent_digest, arch, child_digest, source, repository):
    need(lower_sha(child_digest), "child_digest_binding")
    parent = image + "@" + parent_digest
    provenance = metadata_entry(docker_field(parent, "Provenance"), arch, "SLSA", "provenance_payload")
    sbom = metadata_entry(docker_field(parent, "SBOM"), arch, "SPDX", "sbom_payload")
    need(isinstance(provenance, dict) and provenance, "provenance_payload")
    need(isinstance(sbom, dict) and sbom, "sbom_payload")
    if "buildDefinition" in provenance:
        definition = provenance.get("buildDefinition")
        details = provenance.get("runDetails")
        need(isinstance(definition, dict) and isinstance(details, dict), "provenance_shape")
        need(definition.get("buildType") in {
            "https://mobyproject.org/buildkit@v1",
            "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md",
        }, "provenance_build_type")
        need(isinstance(details.get("builder"), dict), "provenance_builder")
        buildkit = details.get("metadata", {}).get("buildkit_metadata", {}) if isinstance(details.get("metadata"), dict) else {}
    else:
        need(provenance.get("buildType") in {
            "https://mobyproject.org/buildkit@v1",
            "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md",
        }, "provenance_build_type")
        need(isinstance(provenance.get("builder"), dict), "provenance_builder")
        invocation = provenance.get("invocation")
        need(isinstance(invocation, dict), "provenance_invocation")
        environment = invocation.get("environment", {})
        if isinstance(environment, dict) and "platform" in environment:
            need(environment.get("platform") == "linux/" + arch, "provenance_platform")
        metadata = provenance.get("metadata", {})
        buildkit = metadata.get("https://mobyproject.org/buildkit@v1#metadata", {}) if isinstance(metadata, dict) else {}
    need(isinstance(buildkit, dict), "provenance_metadata")
    vcs = buildkit.get("vcs")
    if isinstance(vcs, dict):
        if "revision" in vcs:
            need(vcs.get("revision") == source, "provenance_source_sha")
        if "source" in vcs:
            need(vcs.get("source") == "https://github.com/" + repository, "provenance_source_repo")
    need(sbom.get("SPDXID") == "SPDXRef-DOCUMENT", "sbom_document")
    need(re.fullmatch(r"SPDX-[0-9]+\.[0-9]+", sbom.get("spdxVersion", "")) is not None, "sbom_version")
    need(isinstance(sbom.get("creationInfo"), dict), "sbom_creation")


def validate_child_labels(image, digest, arch, version, source, repository):
    document = docker_full_inspect(image + "@" + digest)
    manifest = document.get("manifest") if isinstance(document, dict) else None
    need(isinstance(manifest, dict) and manifest.get("digest") == digest, "child_digest_binding")
    image_data = document.get("image") if isinstance(document, dict) else None
    config = image_data.get("config") if isinstance(image_data, dict) else None
    labels = config.get("Labels") if isinstance(config, dict) else None
    need(isinstance(image_data, dict) and image_data.get("os") == "linux", "child_identity_os")
    need(image_data.get("architecture") == arch, "child_identity_architecture")
    variant = image_data.get("variant")
    need(variant is None or (arch == "arm64" and variant == "v8"), "child_identity_variant")
    expected = {
        "org.opencontainers.image.version": version,
        "org.opencontainers.image.revision": source,
        "org.opencontainers.image.source": "https://github.com/" + repository,
    }
    need(isinstance(labels, dict) and all(labels.get(key) == value for key, value in expected.items()), "child_identity_labels")


def index_parts(image, digest, expected_arches, version, source, repository):
    ref = image + "@" + digest
    descriptor = docker_inspect(ref)
    need(isinstance(descriptor, dict) and descriptor.get("digest") == digest, "child_digest_binding")
    raw = docker_inspect(ref, raw=True)
    manifests = raw.get("manifests") if isinstance(raw, dict) else None
    need(isinstance(manifests, list) and manifests, "index_shape")
    runnable, attestations = {}, []
    for item in manifests:
        need(isinstance(item, dict) and lower_sha(item.get("digest")), "manifest_descriptor")
        arch = platform_of(item)
        annotations = item.get("annotations", {})
        need(isinstance(annotations, dict), "manifest_annotations")
        if arch is not None:
            need(arch in expected_arches and arch not in runnable, "runnable_platform_set")
            need(annotations.get("vnd.docker.reference.type") != "attestation-manifest", "runnable_attestation_mix")
            runnable[arch] = item["digest"]
        else:
            need(annotations.get("vnd.docker.reference.type") == "attestation-manifest", "unknown_manifest_descriptor")
            subjects = [
                annotations[key]
                for key in ("vnd.docker.reference.digest", "com.docker.reference.digest")
                if key in annotations
            ]
            need(subjects and all(isinstance(value, str) for value in subjects) and len(set(subjects)) == 1, "attestation_subject")
            subject = subjects[0]
            need(lower_sha(subject), "attestation_subject")
            attestations.append((item["digest"], subject))
    need(set(runnable) == set(expected_arches), "platform_set")
    need(len(set(runnable.values())) == len(runnable), "runnable_digest_alias")
    for arch, child in runnable.items():
        validate_child_labels(image, child, arch, version, source, repository)
    covered = {arch: False for arch in expected_arches}
    digest_to_arch = {value: arch for arch, value in runnable.items()}
    for attestation, subject in attestations:
        need(subject in digest_to_arch, "attestation_subject_ref")
        covered[digest_to_arch[subject]] = True
    need(all(covered.values()), "attestation_coverage")
    for arch, child in runnable.items():
        validate_attestation_predicates(image, digest, arch, child, source, repository)
    return runnable


def expected_arches():
    value = required("PLATFORMS").split(",")
    need(len(value) == len(set(value)) and value and all(safe_arch(item) for item in value), "platform_input")
    return value
MAX_ARCHIVE = 512 * 1024 * 1024
MAX_RECORD = 256 * 1024


def artifact_context():
    repository = required("REPOSITORY")
    need(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository) is not None, "artifact_repository")
    configured = os.environ.get("GITHUB_REPOSITORY")
    need(configured in (None, repository), "artifact_repository_binding")
    image, image_id, arch, version, source = required("IMAGE"), required("IMAGE_ID"), required("ARCH"), required("VERSION"), required("SOURCE_SHA")
    run_id, attempt = required("GITHUB_RUN_ID"), required("GITHUB_RUN_ATTEMPT")
    need(safe_id(image_id) and safe_arch(arch) and oci_version(version) and source_sha(source), "artifact_identity")
    need(re.fullmatch(r"[1-9][0-9]{0,19}", run_id) and re.fullmatch(r"[1-9][0-9]{0,19}", attempt), "artifact_run_identity")
    artifact_id = required("ARTIFACT_ID")
    need(re.fullmatch(r"[1-9][0-9]{0,19}", artifact_id), "artifact_id")
    digest = required("ARTIFACT_DIGEST")
    need(lower_sha(digest), "artifact_digest_binding")
    return {"repository": repository, "image": image, "image_id": image_id, "arch": arch, "version": version, "source": source,
            "run": int(run_id), "attempt": int(attempt), "id": int(artifact_id), "archive_digest": digest,
            "name": f"oci-{run_id}-{attempt}-{image_id}-{arch}"}


def verify_artifact_run(ctx, run):
    repository = run.get("repository", {})
    need(run.get("id") == ctx["run"] and run.get("run_attempt") == ctx["attempt"], "artifact_run")
    need(run.get("head_sha") == ctx["source"] and repository.get("full_name") == ctx["repository"], "artifact_source")
    repo_id = repository.get("id")
    need(type(repo_id) is int and repo_id > 0, "artifact_repository_id")
    return repo_id


def artifact_metadata(token, ctx):
    root = github_root(ctx["repository"]) + "/actions/artifacts"
    return api_json(token, root + "/" + str(ctx["id"]))


def verify_artifact_metadata(ctx, artifact, repository_id):
    need(artifact.get("id") == ctx["id"] and artifact.get("name") == ctx["name"], "artifact_identity")
    need(artifact.get("expired") is False, "artifact_expired")
    size = artifact.get("size_in_bytes")
    need(type(size) is int and 0 < size <= MAX_ARCHIVE, "artifact_size")
    digest = artifact.get("digest")
    need(lower_sha(digest), "artifact_digest")
    workflow = artifact.get("workflow_run")
    need(isinstance(workflow, dict), "artifact_workflow")
    need(workflow.get("id") == ctx["run"], "artifact_run")
    if "run_attempt" in workflow:
        need(workflow.get("run_attempt") == ctx["attempt"], "artifact_run")
    need(workflow.get("head_sha") == ctx["source"], "artifact_source")
    need(workflow.get("repository_id") == repository_id and workflow.get("head_repository_id") == repository_id, "artifact_repository")
    need(ctx["archive_digest"] == digest, "artifact_digest_binding")
    return size, digest
def record_member(payload, filename):
    try:
        with zipfile.ZipFile(io.BytesIO(payload)) as archive:
            entries = archive.infolist()
            need(len(entries) == 1, "artifact_zip_entries")
            entry = entries[0]
            need(entry.filename in {filename, "digests/" + filename} and entry.orig_filename == entry.filename, "artifact_zip_path")
            need(not entry.is_dir() and not entry.flag_bits & 1, "artifact_zip_file")
            mode = stat.S_IFMT(entry.external_attr >> 16)
            need(mode in (0, stat.S_IFREG) and entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED), "artifact_zip_type")
            need(entry.file_size <= MAX_RECORD, "artifact_record_size")
            record = archive.read(entry)
            need(len(record) == entry.file_size, "artifact_record_size")
            return record
    except (OSError, zipfile.BadZipFile) as error:
        raise GateError("artifact_zip") from error


def validate_record_bytes(raw, ctx):
    try:
        value = json_document(raw.decode("utf-8"), "artifact_record_json")
    except (UnicodeDecodeError, AttributeError) as error:
        raise GateError("artifact_record_json") from error
    need(isinstance(value, dict), "artifact_record_shape")
    need(set(value) == {"schema", "image_id", "image", "arch", "version", "source_sha", "digest", "run_id", "run_attempt"}, "artifact_record_shape")
    need(type(value["schema"]) is int and value["schema"] == 1 and value["image"] == ctx["image"] and value["image_id"] == ctx["image_id"] and value["arch"] == ctx["arch"] and value["version"] == ctx["version"], "artifact_record_identity")
    need(value["source_sha"] == ctx["source"] and lower_sha(value["digest"]), "artifact_record_source")
    need(value["run_id"] == str(ctx["run"]) and value["run_attempt"] == str(ctx["attempt"]), "artifact_record_run")
    canonical = json.dumps(value, separators=(",", ":"), sort_keys=True).encode("utf-8")
    need(raw == canonical, "artifact_record_bytes")
    return raw


def download_record():
    ctx, token = artifact_context(), required("GH_TOKEN")
    root = github_root(ctx["repository"])
    repository_id = verify_artifact_run(ctx, api_json(token, root + "/actions/runs/" + str(ctx["run"])))
    verify_producer(token, ctx)
    artifact = artifact_metadata(token, ctx)
    size, digest = verify_artifact_metadata(ctx, artifact, repository_id)
    payload = auth_request(token, root + "/actions/artifacts/" + str(ctx["id"]) + "/zip", MAX_ARCHIVE, True)
    need(len(payload) == size and hashlib.sha256(payload).hexdigest() == digest[7:], "artifact_archive_digest")
    verify_artifact_run(ctx, api_json(token, root + "/actions/runs/" + str(ctx["run"])))
    second_size, second_digest = verify_artifact_metadata(ctx, artifact_metadata(token, ctx), repository_id)
    need((second_size, second_digest) == (size, digest), "artifact_metadata_changed")
    record = validate_record_bytes(record_member(payload, ctx["image_id"] + "-" + ctx["arch"] + ".json"), ctx)
    directory = Path("digests")
    need(not directory.is_symlink(), "artifact_destination")
    need(not directory.exists() or directory.is_dir(), "artifact_destination")
    directory.mkdir(mode=0o755, exist_ok=True)
    path = directory / (ctx["image_id"] + "-" + ctx["arch"] + ".json")
    need(not path.exists() and not path.is_symlink(), "artifact_destination")
    try:
        with path.open("xb") as handle:
            handle.write(record)
    except OSError as error:
        raise GateError("artifact_write") from error
def read_records(image, image_id, version, sha, arches):
    directory = Path("digests")
    need(directory.is_dir() and not directory.is_symlink(), "missing_digest_directory")
    expected = {image_id + "-" + arch + ".json" for arch in arches}
    actual = set()
    for path in directory.iterdir():
        need(path.is_file() and not path.is_symlink(), "unsafe_digest_file")
        actual.add(path.name)
    need(actual == expected, "digest_file_set")
    records = {}
    for arch in arches:
        value = json_file(directory / (image_id + "-" + arch + ".json"), "record_json")
        need(set(value) == {"schema", "image_id", "image", "arch", "version", "source_sha", "digest", "run_id", "run_attempt"}, "record_shape")
        need(type(value["schema"]) is int and value["schema"] == 1 and value["image_id"] == image_id and value["image"] == image and value["arch"] == arch and value["version"] == version and value["source_sha"] == sha and lower_sha(value["digest"]), "record_binding")
        need(value["run_id"] == required("GITHUB_RUN_ID") and value["run_attempt"] == required("GITHUB_RUN_ATTEMPT"), "record_run_binding")
        records[arch] = value["digest"]
    return records


def write_index_proof(image, image_id, version, sha, digest, platforms):
    run_id = required("GITHUB_RUN_ID")
    attempt = required("GITHUB_RUN_ATTEMPT")
    need(re.fullmatch(r"[1-9][0-9]*", run_id) and re.fullmatch(r"[1-9][0-9]*", attempt), "run_identity")
    need(lower_sha(digest), "index_proof_digest")
    need(set(platforms) and all(lower_sha(value) for value in platforms.values()), "index_platform_digests")
    proof = {"schema": 1, "image_id": image_id, "image": image, "version": version, "source_sha": sha, "run_id": run_id, "run_attempt": attempt, "index_digest": digest, "platform_digests": dict(sorted(platforms.items()))}
    path = Path("index-proof.json")
    need(not path.is_symlink(), "index_proof_symlink")
    if path.exists():
        need(json_file(path, "index_proof_json") == proof, "index_proof_conflict")
        return
    try:
        with path.open("x", encoding="utf-8") as handle:
            json.dump(proof, handle, separators=(",", ":"), sort_keys=True)
    except OSError as error:
        raise GateError("index_proof_write") from error


def assembly_main(config):
    image, image_id, version, sha = required("IMAGE"), required("IMAGE_ID"), required("VERSION"), required("SOURCE_SHA")
    need(safe_id(image_id) and oci_version(version) and source_sha(sha), "assembly_identity")
    source_main(config)
    arches = expected_arches()
    existing, expected = required("EXISTING"), os.environ.get("EXPECTED_INDEX_DIGEST", "")
    need(existing in {"true", "false"}, "existing_output")
    if existing == "true":
        need(lower_sha(expected), "existing_index_digest")
        need(tag_digest(image, version) == expected, "existing_tag_changed")
        verify_recovery_attestation(
            image, expected, required("REPOSITORY"), sha, required("REF")
        )
        platform_digests = index_parts(image, expected, set(arches), version, sha, config["repository"])
        write_index_proof(image, image_id, version, sha, expected, platform_digests)
        output_values({"index_digest": expected})
        return
    need(expected == "", "unexpected_existing_digest")
    records = read_records(image, image_id, version, sha, arches)
    child_platforms = {}
    refs = []
    for arch in arches:
        child_platforms[arch] = index_parts(image, records[arch], {arch}, version, sha, config["repository"])[arch]
        refs.append(image + "@" + records[arch])
    source_main(config)
    need(tag_digest(image, version) is None, "tag_created_after_admission")
    try:
        run(["docker", "buildx", "imagetools", "create", "-t", image + ":" + version, *refs])
    except GateError as error:
        raise GateError("index_create") from error
    final = tag_digest(image, version)
    need(final is not None, "index_missing_after_create")
    actual = index_parts(image, final, set(arches), version, sha, config["repository"])
    need(actual == child_platforms, "final_platform_digest_set")
    write_index_proof(image, image_id, version, sha, final, actual)
    output_values({"index_digest": final})


def oci_dispatch(mode, config):
    try:
        if mode == "verify":
            verify_main(config)
        elif mode == "source":
            source_main(config)
        elif mode == "admission":
            admission_main()
        elif mode == "record":
            record_main()
        elif mode == "artifact":
            download_record()
        elif mode == "assembly":
            assembly_main(config)
        else:
            raise GateError("unknown_mode")
    except GateError as error:
        die(str(error))
