use velnor_actions_gradle::version::*;

#[test]
fn accepts_the_documented_forms() {
    let graal = parse_java("oracle-graalvm-25.0.3").expect("graal");
    assert_eq!(
        (graal.vendor, graal.core.as_str()),
        ("oracle-graalvm", "25.0.3")
    );
    assert_eq!(
        (graal.major(), graal.build.clone(), graal.lts),
        (25, None, false)
    );
    let temurin = parse_java("temurin-21.0.8+9.0.LTS").expect("temurin");
    assert_eq!(temurin.build.as_deref(), Some("9.0"));
    assert!(temurin.lts);
    assert_eq!(temurin.to_string(), "temurin-21.0.8+9.0.LTS");
    for ok in [
        "corretto-17.0.12",
        "zulu-21.0.4",
        "graalvm-community-21.0.2+13.1",
    ] {
        assert_eq!(parse_java(ok).expect(ok).to_string(), ok);
    }
}

#[test]
fn rejects_loose_and_unknown_declarations() {
    for bad in [
        "21",
        "temurin-21",
        "lts",
        "latest",
        "temurin-latest",
        "oracle-graalvm-25",
        "oracle-graalvm-25.0",
        "oracle-graalvm-25.0.03",
        "openjdk-21.0.1",
        "temurin-21.0.8+",
        "temurin-21.0.8+x",
        "temurin-21.0.8+9..1",
        "temurin-^21.0.8",
        "temurin-21.0.x",
        "temurin-21.0.8 ",
        "temurin-21.0.8\nFOO=1",
        "Temurin-21.0.8",
        "temurin-21.0.8$(id)",
        "temurin-21.0.8;",
        "",
        "oracle-graalvm-25.0.3.LTS.LTS",
    ] {
        assert!(parse_java(bad).is_err(), "{bad:?} must be rejected");
    }
}

#[test]
fn gradle_versions_are_exact() {
    assert_eq!(parse_gradle("9.5.1").as_deref(), Ok("9.5.1"));
    assert_eq!(parse_gradle("8.14").as_deref(), Ok("8.14"));
    for bad in ["9", "9.5.1-rc-1", "9.5.1.0", "09.5.1", "latest", "9.+", ""] {
        assert!(parse_gradle(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn accepted_strings_are_env_safe() {
    let version = parse_java("temurin-21.0.8+9.0.LTS")
        .expect("temurin")
        .to_string();
    assert!(
        version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".+-".contains(c))
    );
}
