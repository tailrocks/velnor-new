use super::*;

fn ffmpeg_setup(bytes: &[u8]) -> (tempfile::TempDir, InstallRequest, LockedArchive) {
    let dir = tempfile::tempdir().expect("The test directory must exist.");
    let root = dir
        .path()
        .canonicalize()
        .expect("The test directory path must resolve.");
    let mut locked = archive(bytes);
    locked.directory = "ffmpeg-1011".to_owned();
    locked.url = FFMPEG_PRIMARY.to_owned();
    let mut req = request(&root, bytes);
    req.pins = vec![("ffmpeg-1011".to_owned(), sha(bytes))];
    (dir, req, locked)
}

#[test]
fn candidates_follow_the_registry_order_and_cft_has_no_fallback() {
    assert_eq!(
        playwright_download_candidates(FFMPEG_PRIMARY),
        [FFMPEG_PRIMARY, FFMPEG_ESRP, FFMPEG_BUCKET]
    );
    assert_eq!(
        playwright_download_candidates(FFMPEG_BUCKET),
        [FFMPEG_BUCKET, FFMPEG_PRIMARY, FFMPEG_ESRP]
    );
    assert_eq!(playwright_download_candidates(URL), [URL]);
}

#[test]
fn a_dead_primary_falls_back_to_the_next_mirror() {
    let bytes = zip_bytes();
    let (_dir, req, locked) = ffmpeg_setup(&bytes);
    let scripted = Scripted {
        hosts: vec![
            (FFMPEG_PRIMARY, Host::ServerError),
            (FFMPEG_ESRP, Host::Dns),
            (FFMPEG_BUCKET, Host::Serve(bytes.clone())),
        ],
        seen: std::cell::RefCell::new(Vec::new()),
    };
    assert_eq!(
        install(&req, &[&locked], &scripted).expect("The browser installation must succeed."),
        1
    );
    assert_eq!(
        *scripted.seen.borrow(),
        [FFMPEG_PRIMARY, FFMPEG_ESRP, FFMPEG_BUCKET]
    );
    assert!(req.browsers_dir().join("ffmpeg-1011").is_dir());
}

#[test]
fn a_mirror_serving_the_wrong_hash_is_rejected() {
    let bytes = zip_bytes();
    let (_dir, req, locked) = ffmpeg_setup(&bytes);
    let mut wrong = bytes.clone();
    wrong.push(0);
    let scripted = Scripted {
        hosts: vec![
            (FFMPEG_PRIMARY, Host::Dns),
            (FFMPEG_ESRP, Host::Serve(wrong.clone())),
            (FFMPEG_BUCKET, Host::Serve(wrong)),
        ],
        seen: std::cell::RefCell::new(Vec::new()),
    };
    let error = install(&req, &[&locked], &scripted)
        .expect_err("The installer must reject the wrong digest.");
    assert!(
        error
            .to_string()
            .contains("playwright_sha256_mismatch:ffmpeg-1011"),
        "{error}"
    );
    assert!(!req.browsers_dir().join("ffmpeg-1011").exists());
    assert!(!req.download_dir().join("ffmpeg-1011.zip").exists());
}

#[test]
fn all_hosts_failing_reports_the_download_failure() {
    let bytes = zip_bytes();
    let (_dir, req, locked) = ffmpeg_setup(&bytes);
    let scripted = Scripted {
        hosts: vec![],
        seen: std::cell::RefCell::new(Vec::new()),
    };
    let error = install(&req, &[&locked], &scripted).expect_err("The download must fail.");
    assert!(
        error.to_string().contains("playwright_download_failed"),
        "{error}"
    );
    assert_eq!(scripted.seen.borrow().len(), 3);
}
