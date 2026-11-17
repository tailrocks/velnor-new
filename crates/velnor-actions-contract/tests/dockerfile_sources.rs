use velnor_actions_contract::config::{DockerfileProblem, analyze_dockerfile};

fn rejects_or_claims_whole_context(text: &str) -> bool {
    match analyze_dockerfile(text, &[]) {
        Ok(facts) => facts.claims_whole_context || facts.context_sources.contains("."),
        Err(_) => true,
    }
}

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
    assert!(rejects_or_claims_whole_context(
        "ARG SRC=src\nFROM scratch\nCOPY $SRC/main.txt /dest\n"
    ));
    Ok(())
}

#[test]
fn unknown_inherited_values_never_narrow_context_sources() -> Result<(), String> {
    for text in [
        "FROM scratch AS base\nARG SRC=src\nFROM base\nCOPY ${SRC:-other}/main.txt /dest\n"
            .to_owned(),
        "FROM scratch AS base\nENV SRC=src\nFROM base\nCOPY ${SRC:-other}/main.txt /dest\n"
            .to_owned(),
        "FROM scratch AS base\nARG SRC=src\nFROM base\nARG SRC\nCOPY ${SRC:-other}/main.txt /dest\n"
            .to_owned(),
        format!(
            "FROM example@sha256:{}\nARG SRC=src\nCOPY $SRC/main.txt /dest\n",
            "a".repeat(64)
        ),
    ] {
        assert!(rejects_or_claims_whole_context(&text), "{text}");
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
    for name in ["TARGETARCH", "BUILDPLATFORM"] {
        let text = format!("FROM scratch\nARG {name}=default\nCOPY ${name}/main.txt /dest\n");
        assert!(rejects_or_claims_whole_context(&text), "{name}");
    }
    for name in ["SOURCE_SHA", "VELNOR_SOURCE_SHA"] {
        let text = format!("FROM scratch\nARG {name}=src\nCOPY ${name}/main.txt /dest\n");
        assert!(
            velnor_actions_contract::config::analyze_runner_dockerfile(&text, &[]).is_err(),
            "{name}"
        );
    }
    assert!(
        analyze_dockerfile("FROM scratch\nCOPY [\"'src'/main.txt\", \"/dest\"]\n", &[]).is_err()
    );
}

#[test]
fn declared_unset_arguments_use_fallbacks() -> Result<(), String> {
    let facts = analyze_dockerfile("ARG BASE\nFROM ${BASE:-scratch}\n", &[])
        .map_err(|e| format!("{e:?}"))?;
    assert!(facts.external_images.is_empty());
    for keyword in ["COPY", "ADD"] {
        for operands in [
            "${SRC:-src}/main.txt /dest",
            "[\"${SRC:-src}/main.txt\", \"/dest\"]",
        ] {
            let text = format!("FROM scratch\nARG SRC\n{keyword} {operands}\n");
            let facts = analyze_dockerfile(&text, &[]).map_err(|e| format!("{e:?}"))?;
            assert!(facts.context_sources.contains("src/main.txt"), "{text}");
        }
    }
    Ok(())
}

#[test]
fn ordinary_sha_arguments_use_their_defaults() -> Result<(), String> {
    for name in ["SOURCE_SHA", "VELNOR_SOURCE_SHA"] {
        let text = format!(
            "ARG {name}=scratch\nFROM ${name}\nARG {name}=src\nCOPY ${name}/main.txt /dest\n"
        );
        let facts = analyze_dockerfile(&text, &[]).map_err(|e| format!("{e:?}"))?;
        assert!(facts.context_sources.contains("src/main.txt"), "{name}");
    }
    Ok(())
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
    for keyword in ["COPY", "ADD"] {
        let text = format!(
            "FROM scratch\nARG SRC=some\"thing\"\n{keyword} ${{SRC:-src}}/main.txt /dest\n"
        );
        assert!(rejects_or_claims_whole_context(&text), "{text}");
    }
}

#[test]
fn unsupported_variable_modifiers_do_not_narrow_sources() {
    for expression in [
        "${SRC-other:-src}",
        "${SRC+other:-src}",
        "${SRC?other:-src}",
    ] {
        let text = format!("FROM scratch\nARG SRC=actual\nCOPY {expression}/main.txt /dest\n");
        assert!(rejects_or_claims_whole_context(&text), "{expression}");
    }
}

#[test]
fn nested_variable_defaults_do_not_narrow_sources() {
    let text = "FROM scratch\nARG SRC=actual\nCOPY ${SRC:-${OTHER:-src}}/main.txt /dest\n";
    assert!(rejects_or_claims_whole_context(text));
}

#[test]
fn json_sources_beginning_with_dashes_are_operands() -> Result<(), String> {
    for keyword in ["COPY", "ADD"] {
        for source in ["--src/main.txt", "--from=src/main.txt"] {
            let text = format!("FROM scratch\n{keyword} [\"{source}\", \"/dest\"]\n");
            let facts = analyze_dockerfile(&text, &[]).map_err(|e| format!("{e:?}"))?;
            assert!(facts.context_sources.contains(source), "{text}: {facts:?}");
        }
    }
    let facts = analyze_dockerfile("FROM scratch\nCOPY --chown=1:1 src/main.txt /dest\n", &[])
        .map_err(|e| format!("{e:?}"))?;
    assert!(facts.context_sources.contains("src/main.txt"));
    assert_eq!(facts.context_sources.len(), 1);
    Ok(())
}

#[test]
fn multiple_argument_names_do_not_override_imported_values_with_fallbacks() {
    let text = "ARG SRC=actual\nFROM scratch\nARG SRC OTHER\nCOPY ${SRC:-src}/main.txt /dest\n";
    if let Ok(facts) = analyze_dockerfile(text, &[])
        && !facts.claims_whole_context
    {
        assert!(
            facts.context_sources.contains("actual/main.txt"),
            "{facts:?}"
        );
        assert!(!facts.context_sources.contains("src/main.txt"), "{facts:?}");
    }
}

#[test]
fn multiple_argument_defaults_do_not_replace_values_with_fallbacks() {
    let text = "FROM scratch\nARG OTHER=unused SRC=actual\nCOPY ${SRC:-src}/main.txt /dest\n";
    if let Ok(facts) = analyze_dockerfile(text, &[])
        && !facts.claims_whole_context
    {
        assert!(
            facts.context_sources.contains("actual/main.txt"),
            "{facts:?}"
        );
        assert!(!facts.context_sources.contains("src/main.txt"), "{facts:?}");
    }
}

#[test]
fn add_sources_must_be_proven_local_before_claiming_context() -> Result<(), String> {
    for text in [
        "FROM scratch\nARG PATH\nADD https://host/$PATH /dest\n",
        "FROM scratch\nENV PATH=src\nADD https://host/$PATH /dest\n",
        "FROM scratch\nARG HTTP_PROXY=src\nADD $HTTP_PROXY /dest\n",
        "FROM scratch\nARG SRC=https://host/file\nADD $SRC /dest\n",
        "FROM scratch AS base\nADD --from=base https://host/file /dest\n",
        "FROM scratch\nARG PATH\nADD https://host/$PATH\n",
    ] {
        assert!(
            matches!(
                analyze_dockerfile(text, &[]),
                Err(DockerfileProblem::RemoteAdd(_))
            ),
            "{text}: {:?}",
            analyze_dockerfile(text, &[])
        );
    }
    let local = analyze_dockerfile("FROM scratch\nARG SRC=src\nADD $SRC/file /dest\n", &[])
        .map_err(|e| format!("{e:?}"))?;
    assert!(local.context_sources.contains("src/file"));
    let unknown = analyze_dockerfile(
        "FROM scratch\nARG HTTP_PROXY=src\nCOPY $HTTP_PROXY/file /dest\n",
        &[],
    )
    .map_err(|e| format!("{e:?}"))?;
    assert!(unknown.claims_whole_context);
    Ok(())
}

#[test]
fn fired_onbuild_instructions_keep_direct_refusals() -> Result<(), String> {
    for instruction in [
        "RUN --mount=type=secret,id=x echo x",
        "RUN --mount=type=ssh echo x",
        "RUN --mount=type=bind,from=busybox:latest echo x",
        "ADD https://host/file /dest",
        "COPY dir/../file /dest",
        "ADD ../file /dest",
        "COPY [\"'src'/file\", \"/dest\"]",
        "ADD --from=base https://host/file /dest",
    ] {
        let direct = analyze_dockerfile(&format!("FROM scratch AS base\n{instruction}\n"), &[]);
        assert!(direct.is_err(), "{instruction}: {direct:?}");
        let inherited = analyze_dockerfile(
            &format!("FROM scratch AS base\nONBUILD {instruction}\nFROM base\n"),
            &[],
        );
        assert_eq!(inherited, direct, "{instruction}");
    }
    for instruction in ["RUN echo x", "COPY src/file /dest", "ADD src/file /dest"] {
        analyze_dockerfile(
            &format!("FROM scratch AS base\nONBUILD {instruction}\nFROM base\n"),
            &[],
        )
        .map_err(|e| format!("{e:?}"))?;
    }
    analyze_dockerfile(
        "FROM scratch\nONBUILD RUN --mount=type=secret,id=x echo x\n",
        &[],
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[test]
fn parent_components_refuse_before_normalization_or_unknown_context() -> Result<(), String> {
    for keyword in ["COPY", "ADD"] {
        for (args, source) in [
            ("", "dir/../file"),
            ("", "../file"),
            ("ARG SRC=dir\n", "$SRC/../file"),
            ("ARG SRC=dir/..\n", "$SRC/file"),
            ("ARG HTTP_PROXY\n", "$HTTP_PROXY/../file"),
            ("", "--from=0 ../file"),
        ] {
            let text = format!("FROM scratch\n{args}{keyword} {source} /dest\n");
            assert!(
                matches!(
                    analyze_dockerfile(&text, &[]),
                    Err(DockerfileProblem::ContextEscape(_))
                ),
                "{text}: {:?}",
                analyze_dockerfile(&text, &[])
            );
        }
        for source in ["dir/./file", "dir//file"] {
            let text = format!("FROM scratch\n{keyword} {source} /dest\n");
            let facts = analyze_dockerfile(&text, &[]).map_err(|e| format!("{e:?}"))?;
            assert!(facts.context_sources.contains("dir/file"));
        }
        let text = format!("FROM scratch\n{keyword} ../file\n");
        assert!(matches!(
            analyze_dockerfile(&text, &[]),
            Err(DockerfileProblem::ContextEscape(_))
        ));
    }
    Ok(())
}
