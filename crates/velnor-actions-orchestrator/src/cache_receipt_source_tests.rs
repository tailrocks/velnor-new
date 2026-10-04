use super::*;

#[test]
fn literal_source_closure_isolated_and_public_bundle_guards_are_closed() {
    let sources = body::producer_modules();
    let encoded = serde_json::to_string(&sources).expect("source JSON");
    let literal = serde_json::to_string(&encoded).expect("literal JSON");
    let entry = runtime::ENTRYPOINT
        .split("\ntry:\n    {'manifest'")
        .next()
        .expect("entrypoint");
    let consumer = consumer_runtime::ENTRYPOINT
        .split("\ntry:\n")
        .next()
        .expect("consumer entrypoint");
    let script = format!(
        "import json,sys,types\n_SOURCES=json.loads({literal})\n_CONFIG={{}}\n{}\n{entry}\n{consumer}\n{PYTHON_TESTS}",
        runtime::LOADER
    );
    let directory = tempfile::tempdir().expect("isolated test root");
    for name in [
        "cache_receipt.py",
        "cache_receipt_common.py",
        "source_archive_inventory.py",
        "source_archive_inventory_common.py",
        "source_archive_inventory_fs.py",
        "source_archive_inventory_leaf.py",
        "source_archive_inventory_walk.py",
        "json.py",
        "sitecustomize.py",
    ] {
        std::fs::write(
            directory.path().join(name),
            "raise RuntimeError('counterfeit_startup_executed')\n",
        )
        .expect("counterfeit module");
    }
    let output = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-c", &script])
        .current_dir(directory.path())
        .env("PYTHONPATH", directory.path())
        .output()
        .expect("system Python");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Ran 7 tests"));
}

#[test]
fn isolated_consumer_literal_registry_cannot_admit_unpublished_receipts() {
    let modules = body::consumer_modules(
        &serde_json::json!({"gh_distribution": null}),
        &body::producer_modules(),
    )
    .expect("consumer module registry");
    let literal = serde_json::to_string(&serde_json::to_string(&modules).expect("registry JSON"))
        .expect("literal JSON");
    let script = format!(
        "import json,sys,types\n_SOURCES=json.loads({literal})\n_CONFIG={{}}\n{}\n{}",
        consumer_runtime::LOADER,
        consumer_runtime::ENTRYPOINT
    );
    let directory = tempfile::tempdir().expect("consumer isolated startup");
    for name in [
        "cache_receipt_common.py",
        "source_archive_inventory.py",
        "source_archive_inventory_common.py",
        "source_archive_inventory_fs.py",
        "source_archive_inventory_leaf.py",
        "source_archive_inventory_walk.py",
        "cache_receipt_policy.py",
        "cache_receipt_gh.py",
        "receipt_fresh_gh_download.py",
        "receipt_fresh_gh_archive.py",
        "receipt_fresh_gh.py",
        "cache_receipt_materialize_transaction.py",
        "cache_receipt_materialize.py",
        "cache_receipt.py",
        "json.py",
        "sitecustomize.py",
    ] {
        std::fs::write(
            directory.path().join(name),
            "raise RuntimeError('counterfeit_consumer_startup_executed')\n",
        )
        .expect("counterfeit consumer module");
    }
    let output_file = directory.path().join("github-output");
    let output = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-c", &script])
        .current_dir(directory.path())
        .env("PYTHONPATH", directory.path())
        .env("GITHUB_OUTPUT", &output_file)
        .output()
        .expect("actual system Python");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(output_file).expect("cold evidence"),
        "verified=false\nerror=CACHE_RECEIPT_UNQUALIFIED\n"
    );
}

#[test]
fn compiled_initializer_preserves_literal_transport_paths_without_source_interpolation() {
    let configuration = serde_json::json!({"transport_layout": {
        "sdk_paths": ["${{ runner.temp }}/velnor/mise", "${{ runner.temp }}/velnor/cache-receipts/fixed"]
    }});
    let modules = body::producer_modules();
    let initializer = body::initializer(&configuration, &modules).expect("hex initializer");
    assert!(!initializer.contains("${{"));
    let expected =
        serde_json::to_string(&serde_json::to_string(&configuration).expect("config JSON"))
            .expect("config literal");
    let script = format!(
        "{initializer}assert _CONFIG == json.loads({expected})\n{}",
        runtime::LOADER
    );
    let output = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-c", &script])
        .output()
        .expect("system Python hex initializer");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

const PYTHON_TESTS: &str = r#"
import copy, pathlib, tempfile, unittest

class SourceClosureTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='velnor-source-receipt-test-')
        self.root = pathlib.Path(self.temporary.name).resolve()
        os.environ['VELNOR_CACHE_RECEIPT_ROOT'] = str(self.root / 'evidence')
        os.environ['VELNOR_CACHE_RECEIPT_BUNDLE_PATH'] = str(self.root / 'action-bundle')
        _CONFIG.clear()
        _CONFIG.update(role='tool-planning', descriptor_sha256='a'*64, helper_sha256='b'*64,
            catalog_sha256='c'*64, policy_sha256='d'*64, recipe_sha256='e'*64,
            allowed_roots=['mise'], optional_roots=[], predicate_type='https://velnor.dev/cache-producer/v1')
        self.subject = canonical({'schema': 2, 'entries': []})
        self.predicate = {'schema': 1, 'manifest_sha256': hashlib.sha256(self.subject).hexdigest()}
        _write('manifest.json', self.subject)
        _write('predicate.json', canonical(self.predicate))
        statement = {'_type': 'https://in-toto.io/Statement/v1',
            'subject': [{'name': 'manifest.json', 'digest': {'sha256': hashlib.sha256(self.subject).hexdigest()}}],
            'predicateType': _CONFIG['predicate_type'], 'predicate': self.predicate}
        encoded = lambda value: base64.b64encode(value).decode('ascii')
        self.bundle = {'mediaType': 'application/vnd.dev.sigstore.bundle.v0.3+json',
            'verificationMaterial': {'certificate': {'rawBytes': encoded(b'opaque public certificate')},
                'timestampVerificationData': {}, 'tlogEntries': [{'logId': {'keyId': encoded(b'log-id')},
                    'kindVersion': {'kind': 'dsse', 'version': '0.0.1'},
                    'canonicalizedBody': encoded(b'public-log-evidence'),
                    'inclusionPromise': {'signedEntryTimestamp': encoded(b'public-promise')}}]},
            'dsseEnvelope': {'payloadType': 'application/vnd.in-toto+json',
                'payload': encoded(canonical(statement)), 'signatures': [{'sig': encoded(b'public-signature')}]}}

    def tearDown(self):
        self.temporary.cleanup()

    def store(self, value):
        (self.root / 'action-bundle').write_bytes(canonical(value))

    def test_public_shape_and_exact_statement_copy(self):
        self.store(self.bundle)
        _bundle()
        self.assertEqual((self.root / 'evidence/bundle.sigstore.json').read_bytes(), canonical(self.bundle))

    def test_transport_evidence_rejects_extra_files_and_unsafe_entries(self):
        self.store(self.bundle)
        evidence = self.root / 'evidence'
        extra = evidence / 'caller-file'
        for kind in ('file', 'directory', 'symlink'):
            if kind == 'file': extra.write_bytes(b'unsigned')
            elif kind == 'directory': extra.mkdir()
            else: extra.symlink_to('manifest.json')
            with self.assertRaises(ColdReceipt): _bundle()
            if kind == 'directory': extra.rmdir()
            else: extra.unlink()
        manifest = evidence / 'manifest.json'
        manifest.chmod(0o644)
        with self.assertRaises(ColdReceipt): _bundle()
        manifest.chmod(0o600)
        outside = self.root / 'outside-hardlink'
        os.link(manifest, outside)
        with self.assertRaises(ColdReceipt): _bundle()
        outside.unlink()
        _bundle()
        manifest.unlink()
        os.mkfifo(manifest, 0o600)
        with self.assertRaises(ColdReceipt): _evidence_complete()

    def test_malformed_certificate_and_signature_rejected(self):
        for field in ('certificate', 'signature'):
            value = copy.deepcopy(self.bundle)
            if field == 'certificate': value['verificationMaterial']['certificate']['rawBytes'] = '!!invalid!!'
            else: value['dsseEnvelope']['signatures'][0]['sig'] = ''
            self.store(value)
            with self.assertRaises(ColdReceipt): _bundle()

    def test_token_and_foreign_public_material_rejected(self):
        for injected in ('eyJabcdefghijk.abcdefghijk.abcdefghijk', 'unexpected'):
            value = copy.deepcopy(self.bundle)
            value['verificationMaterial']['credential'] = injected
            self.store(value)
            with self.assertRaises(ColdReceipt): _bundle()

    def test_changed_subject_and_extra_log_fields_rejected(self):
        value = copy.deepcopy(self.bundle)
        value['verificationMaterial']['tlogEntries'][0]['caller_control'] = 'forged'
        self.store(value)
        with self.assertRaises(ColdReceipt): _bundle()
        self.store(self.bundle)
        _write('manifest.json', self.subject + b' ')
        with self.assertRaises(ColdReceipt): _bundle()

    def test_unpublished_consumer_always_cold(self):
        with self.assertRaisesRegex(ColdReceipt, 'producer_policy_unqualified'): _verify()

    @unittest.skipUnless(sys.platform == 'linux', 'actual Linux producer metadata proof required')
    def test_actual_producer_manifest_exact_payload(self):
        payload = self.root / 'payload'
        (payload / 'mise').mkdir(parents=True)
        (payload / 'mise/tool').write_bytes(b'actual pure tool bytes')
        (payload / 'unrelated').mkdir()
        (payload / 'unrelated/state').write_bytes(b'excluded state')
        os.environ.update(VELNOR_CACHE_PAYLOAD_ROOT=str(payload),
            VELNOR_CACHE_SOURCE_REPOSITORY='owner/repository', VELNOR_CACHE_SOURCE_REPOSITORY_ID='123',
            VELNOR_CACHE_SOURCE_SHA='a'*40, VELNOR_CACHE_RECEIPT_KEY='fixed-snapshot-key',
            VELNOR_CACHE_RUN_ID='12', VELNOR_CACHE_RUN_ATTEMPT='2')
        _manifest()
        result = strict_json((self.root / 'evidence/manifest.json').read_bytes())
        self.assertEqual(result['schema'], 2)
        self.assertEqual([entry['path'] for entry in result['entries']], ['mise', 'mise/tool'])

unittest.main(argv=['compiled-source'], exit=True)
"#;
