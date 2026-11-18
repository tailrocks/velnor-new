use velnor_actions_contract::config::analyze_dockerfile;

fn problem_code(text: &str) -> Option<String> {
    match analyze_dockerfile(text, &[]) {
        Ok(facts) => {
            assert!(facts.external_images.is_empty(), "{text}: {facts:?}");
            None
        }
        Err(problem) => Some(problem.code()),
    }
}

#[test]
fn a_token_that_starts_no_heredoc_does_not_hide_the_next_stage() {
    for instruction in [
        "USER <<EOF",
        "ENV NOTE=<<EOF",
        "RUN echo <<EOF<bar",
        "RUN echo <<<\"text\"",
        "RUN echo $((1 << 3))",
        "RUN [\"echo\", \"<<EOF\"]",
    ] {
        let text = format!("FROM scratch\n{instruction}\nFROM ubuntu:latest\n");
        assert_eq!(
            problem_code(&text).as_deref(),
            Some("image_base_unpinned:ubuntu:latest"),
            "{text}"
        );
    }
}

#[test]
fn a_real_heredoc_hides_its_body_and_the_stage_after_it_is_still_checked() {
    let text = "FROM scratch\nRUN cat <<EOF\nFROM ubuntu:latest\nEOF\n";
    assert_eq!(problem_code(text), None);
    for text in [
        "FROM scratch\nRUN cat <<EOF\nbody\nEOF\nFROM ubuntu:latest\n",
        "FROM scratch\nRUN cat <<\"EOF\"\nbody\nEOF\nFROM ubuntu:latest\n",
        "FROM scratch\nCOPY <<EOF /dest\nbody\nEOF\nFROM ubuntu:latest\n",
    ] {
        assert_eq!(
            problem_code(text).as_deref(),
            Some("image_base_unpinned:ubuntu:latest"),
            "{text}"
        );
    }
}
