use super::*;
#[test]
fn the_size_step_refuses_failed_tar_zstd_wc_and_invalid_counts() {
    let plan = bun();
    for (case, tar_body, zstd_body, wc_body) in [
        (
            "tar-fails",
            "#!/bin/sh\nexit 7\n",
            "#!/bin/sh\ncat >/dev/null; printf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null\nprintf '1\\n'\n",
        ),
        (
            "zstd-fails",
            "#!/bin/sh\nprintf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null\nexit 7\n",
            "#!/bin/sh\ncat >/dev/null\nprintf '1\\n'\n",
        ),
        (
            "wc-fails",
            "#!/bin/sh\nprintf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null; printf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null\nexit 7\n",
        ),
        (
            "invalid-count",
            "#!/bin/sh\nprintf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null; printf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null\nprintf 'not-a-count\\n'\n",
        ),
        (
            "empty-count",
            "#!/bin/sh\nprintf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null\n",
            "#!/bin/sh\ncat >/dev/null\nexit 0\n",
        ),
        (
            "trailing-count-token",
            "#!/bin/sh\nprintf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null; printf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null\nprintf '1 garbage\\n'\n",
        ),
        (
            "multiple-count-lines",
            "#!/bin/sh\nprintf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null; printf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null\nprintf '1\\n999\\n'\n",
        ),
        (
            "unterminated-extra-count-line",
            "#!/bin/sh\nprintf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null; printf 'archive'\n",
            "#!/bin/sh\ncat >/dev/null\nprintf '1\\n999'\n",
        ),
    ] {
        let sandbox = Sandbox::new(case);
        std::fs::create_dir_all(sandbox.0.join("runner/velnor/bun-cache"))
            .expect("The cache directory must exist.");
        write_exec(&sandbox.0.join("bin/tar"), tar_body);
        write_exec(&sandbox.0.join("bin/zstd"), zstd_body);
        write_exec(&sandbox.0.join("bin/wc"), wc_body);

        let (stdout, outputs) = sandbox.run_with("bash", &size_script(&plan), &[]);
        assert!(stdout.contains("size_unreadable"), "{case}: {stdout}");
        assert_eq!(outputs.trim(), "admitted=false", "{case}");
    }
}

#[test]
fn the_size_step_admits_a_successful_compressed_measurement() {
    let plan = bun();
    let sandbox = Sandbox::new("compressed-small");
    std::fs::create_dir_all(sandbox.0.join("runner/velnor/bun-cache"))
        .expect("The cache directory must exist.");
    write_exec(&sandbox.0.join("bin/tar"), "#!/bin/sh\nprintf 'archive'\n");
    write_exec(&sandbox.0.join("bin/zstd"), "#!/bin/sh\ncat >/dev/null\n");
    write_exec(
        &sandbox.0.join("bin/wc"),
        "#!/bin/sh\ncat >/dev/null\nprintf '8\\n'\n",
    );

    let (_, outputs) = sandbox.run_with("bash", &size_script(&plan), &[]);
    assert_eq!(outputs.trim(), "admitted=true");
}

#[test]
fn the_size_step_admits_zero_compressed_bytes() {
    let plan = bun();
    let sandbox = Sandbox::new("compressed-zero");
    std::fs::create_dir_all(sandbox.0.join("runner/velnor/bun-cache"))
        .expect("The cache directory must exist.");
    write_exec(&sandbox.0.join("bin/tar"), "#!/bin/sh\nprintf 'archive'\n");
    write_exec(&sandbox.0.join("bin/zstd"), "#!/bin/sh\ncat >/dev/null\n");
    write_exec(
        &sandbox.0.join("bin/wc"),
        "#!/bin/sh\ncat >/dev/null\nprintf '0\\n'\n",
    );

    let (_, outputs) = sandbox.run_with("bash", &size_script(&plan), &[]);
    assert_eq!(outputs.trim(), "admitted=true");
}

#[test]
fn the_size_step_refuses_a_failed_disk_usage_measurement() {
    let plan = bun();
    let sandbox = Sandbox::new("du-fails");
    std::fs::create_dir_all(sandbox.0.join("runner/velnor/bun-cache"))
        .expect("The cache directory must exist.");
    let limited_path = sandbox.0.join("limited-bin");
    std::fs::create_dir_all(&limited_path).expect("The limited command directory must exist.");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/bin/mkdir", limited_path.join("mkdir"))
        .expect("The mkdir command link must be created.");
    write_exec(&limited_path.join("du"), "#!/bin/sh\nexit 7\n");
    let path = limited_path.to_string_lossy().into_owned();

    let (stdout, outputs) = sandbox.run_with("/bin/bash", &size_script(&plan), &[("PATH", &path)]);
    assert!(stdout.contains("size_unreadable"), "{stdout}");
    assert_eq!(outputs.trim(), "admitted=false");
}

#[test]
fn the_disk_usage_fallback_accepts_the_exact_path_with_spaces() {
    let plan = bun();
    let sandbox = Sandbox::new("du-spaced-path");
    let runner_temp = sandbox.0.join("runner temp");
    let cache_dir = runner_temp.join("velnor/bun-cache");
    std::fs::create_dir_all(&cache_dir).expect("The cache directory must exist.");
    std::fs::write(cache_dir.join("entry"), vec![7_u8; 4096])
        .expect("The test cache data must be written.");
    let limited_path = sandbox.0.join("limited-bin");
    std::fs::create_dir_all(&limited_path).expect("The limited command directory must exist.");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/bin/mkdir", limited_path.join("mkdir"))
            .expect("The mkdir command link must be created.");
        std::os::unix::fs::symlink("/usr/bin/du", limited_path.join("du"))
            .expect("The du command link must be created.");
    }
    let path = limited_path.to_string_lossy().into_owned();
    let runner_temp = runner_temp.to_string_lossy().into_owned();

    let (_, outputs) = sandbox.run_with(
        "/bin/bash",
        &size_script(&plan),
        &[("PATH", &path), ("RUNNER_TEMP", &runner_temp)],
    );
    assert_eq!(outputs.trim(), "admitted=true");
}

#[test]
fn the_disk_usage_fallback_rejects_wrong_paths_and_extra_lines() {
    let plan = bun();
    for (tag, body) in [
        ("du-wrong-path", "#!/bin/sh\nprintf '1\\twrong-path\\n'\n"),
        (
            "du-extra-line",
            "#!/bin/sh\nprintf '1\\t%s\\n2\\t%s\\n' \"$1\" \"$1\"\n",
        ),
        (
            "du-unterminated-extra-line",
            "#!/bin/sh\nprintf '1\\t%s\\n999' \"$1\"\n",
        ),
    ] {
        let sandbox = Sandbox::new(tag);
        std::fs::create_dir_all(sandbox.0.join("runner/velnor/bun-cache"))
            .expect("The cache directory must exist.");
        let limited_path = sandbox.0.join("limited-bin");
        std::fs::create_dir_all(&limited_path).expect("The limited command directory must exist.");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/bin/mkdir", limited_path.join("mkdir"))
            .expect("The mkdir command link must be created.");
        write_exec(&limited_path.join("du"), body);
        let path = limited_path.to_string_lossy().into_owned();

        let (stdout, outputs) =
            sandbox.run_with("/bin/bash", &size_script(&plan), &[("PATH", &path)]);
        assert!(stdout.contains("size_unreadable"), "{tag}: {stdout}");
        assert_eq!(outputs.trim(), "admitted=false", "{tag}");
    }
}
