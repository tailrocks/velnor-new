// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn text_compilation_fallback_distinguishes_selected_targets_and_cargo_modes() {
    for flag in ["--test", "--bin", "--example", "--bench"] {
        for repeated in [false, true] {
            let lanes: Vec<_> = ["alpha", "beta", "gamma"]
                .into_iter()
                .enumerate()
                .map(|(index, target)| {
                    let target = if repeated { "alpha" } else { target };
                    let mut value = lane();
                    value["lane"] = json!(format!("lane-{index}"));
                    value["caches"] = json!([]);
                    value["invocations"][0]["argv"] = if index == 1 {
                        json!([
                            "cargo",
                            "+stable",
                            "--config",
                            "--test",
                            "test",
                            format!("{flag}={target}")
                        ])
                    } else {
                        json!(["cargo", "test", "--timings", flag, target])
                    };
                    metrics(value)
                })
                .collect();
            let result = check(&lanes);
            assert_eq!(
                result.is_err(),
                repeated,
                "{flag} repeated={repeated}: {result:?}"
            );
        }
    }
    for repeated in [false, true] {
        let lanes: Vec<_> = ["generic", "native", "x86-64-v3"].into_iter().enumerate().map(|(index, cpu)| {
            let mut value = lane();
            value["lane"] = json!(format!("execution-{index}"));
            value["caches"] = json!([]);
            value["invocations"][0]["execution"] = json!({"RUSTFLAGS":format!("-C target-cpu={}", if repeated { "generic" } else { cpu })});
            metrics(value)
        }).collect();
        assert_eq!(check(&lanes).is_err(), repeated);
    }
    let lanes: Vec<_> = [
        vec!["cargo", "check"],
        vec!["cargo", "test"],
        vec!["cargo", "nextest", "run"],
    ]
    .into_iter()
    .enumerate()
    .map(|(index, argv)| {
        let mut value = lane();
        value["lane"] = json!(format!("lane-{index}"));
        value["caches"] = json!([]);
        value["invocations"][0]["argv"] = json!(argv);
        metrics(value)
    })
    .collect();
    assert!(check(&lanes).is_ok());
}
