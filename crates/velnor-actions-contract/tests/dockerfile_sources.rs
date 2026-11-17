use velnor_actions_contract::config::analyze_dockerfile;

#[test]
fn copy_and_add_expand_stage_arguments_and_defaults() -> Result<(), String> {
    for instruction in [
        "COPY ${SRC}/main.txt /dest",
        "ADD ${SRC}/main.txt /dest",
        "COPY [\"${SRC}/main.txt\", \"/dest\"]",
        "ADD [\"${SRC}/main.txt\", \"/dest\"]",
    ] {
        let text = format!("FROM scratch\nARG SRC=src\n{instruction}\n");
        let facts = analyze_dockerfile(&text, &[]).map_err(|e| format!("{e:?}"))?;
        assert!(
            facts.context_sources.contains("src/main.txt"),
            "{instruction}: {facts:?}"
        );
    }
    let facts = analyze_dockerfile("FROM scratch\nCOPY ${SRC:-src}/main.txt /dest\n", &[])
        .map_err(|e| format!("{e:?}"))?;
    assert!(facts.context_sources.contains("src/main.txt"));
    Ok(())
}

#[test]
fn args_are_scoped_to_each_stage_and_globals_need_redeclaration() -> Result<(), String> {
    let text = "ARG SRC=src\nFROM scratch\nARG SRC\nCOPY $SRC/main.txt /dest\nFROM scratch\nCOPY ${SRC:-other}/main.txt /dest\n";
    let facts = analyze_dockerfile(text, &[]).map_err(|e| format!("{e:?}"))?;
    assert!(facts.context_sources.contains("src/main.txt"));
    assert!(facts.context_sources.contains("other/main.txt"));
    let unresolved =
        analyze_dockerfile("ARG SRC=src\nFROM scratch\nCOPY $SRC/main.txt /dest\n", &[]);
    assert!(
        unresolved.is_err() || unresolved.is_ok_and(|facts| facts.context_sources.contains("."))
    );
    Ok(())
}

#[test]
fn unknown_inherited_values_never_narrow_context_sources() -> Result<(), String> {
    for text in [
        "FROM scratch AS base\nARG SRC=src\nFROM base\nCOPY ${SRC:-other}/main.txt /dest\n"
            .to_owned(),
        "FROM scratch AS base\nENV SRC=src\nFROM base\nCOPY ${SRC:-other}/main.txt /dest\n"
            .to_owned(),
        format!(
            "FROM example@sha256:{}\nARG SRC=src\nCOPY $SRC/main.txt /dest\n",
            "a".repeat(64)
        ),
    ] {
        assert!(analyze_dockerfile(&text, &[]).is_err(), "{text}");
    }
    let text = "ARG SRC=global\nFROM scratch\nARG SRC=src\nARG SRC\nCOPY $SRC/main.txt /dest\n";
    let facts = analyze_dockerfile(text, &[]).map_err(|e| format!("{e:?}"))?;
    assert!(facts.context_sources.contains("src/main.txt"));
    Ok(())
}

#[test]
fn quoted_argument_with_spaces_keeps_the_whole_source() -> Result<(), String> {
    let text = "FROM scratch\nARG SRC=\"src dir\"\nCOPY [\"$SRC/main.txt\", \"/dest\"]\n";
    let facts = analyze_dockerfile(text, &[]).map_err(|e| format!("{e:?}"))?;
    assert!(facts.context_sources.contains("src dir/main.txt"));
    Ok(())
}

#[test]
fn ambiguous_operand_quoting_does_not_narrow_sources() {
    for operand in [
        r#""src dir/main.txt""#,
        r"src\ dir/main.txt",
        r"\$SRC/main.txt",
    ] {
        for keyword in ["COPY", "ADD"] {
            let text = format!("FROM scratch\nARG SRC=src\n{keyword} {operand} /dest\n");
            assert!(analyze_dockerfile(&text, &[]).is_err(), "{text}");
        }
    }
}

#[test]
fn json_commas_are_part_of_the_source_and_quoted_defaults_are_not_guessed() -> Result<(), String> {
    let facts = analyze_dockerfile(
        "FROM scratch\nCOPY [\"src,dir/main.txt\", \"/dest\"]\n",
        &[],
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(facts.context_sources.contains("src,dir/main.txt"));
    let text = "FROM scratch\nCOPY [\"${SRC:-'src'}/main.txt\", \"/dest\"]\n";
    assert!(analyze_dockerfile(text, &[]).is_err());
    Ok(())
}

#[test]
fn injected_build_args_and_quoted_json_sources_remain_unresolved() {
    for name in [
        "SOURCE_SHA",
        "VELNOR_SOURCE_SHA",
        "TARGETARCH",
        "BUILDPLATFORM",
    ] {
        let text = format!("FROM scratch\nARG {name}=default\nCOPY ${name}/main.txt /dest\n");
        assert!(analyze_dockerfile(&text, &[]).is_err(), "{name}");
    }
    assert!(
        analyze_dockerfile("FROM scratch\nCOPY [\"'src'/main.txt\", \"/dest\"]\n", &[]).is_err()
    );
}

#[test]
fn implicit_platform_args_and_shell_word_splitting_are_not_guessed() {
    assert!(analyze_dockerfile("FROM ${TARGETARCH:-scratch}\n", &[]).is_err());
    assert!(
        analyze_dockerfile(
            "FROM scratch\nARG SRC=\"src dir\"\nCOPY $SRC/main.txt /dest\n",
            &[]
        )
        .is_err()
    );
}

#[test]
fn unsupported_global_argument_value_does_not_fall_back_to_scratch() {
    let text = "ARG BASE=busy\"box\":latest\nFROM ${BASE:-scratch}\n";
    assert!(analyze_dockerfile(text, &[]).is_err());
}
