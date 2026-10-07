//! Mise-use detection words for one shell step.
//!
//! Split from `cache_p08` (size gate): the detectors behind setup
//! insertion and tool inference share this word expansion.

/// Words one shell step exposes to the mise detectors.
///
/// Bare argv exposes its elements; inline `sh -c` scripts expose the
/// script's whitespace-separated words with one quote layer removed.
/// The generator joins fixed `mise install`/`exec` argv into isolation
/// scripts (deny/fetch), so element-only detection would miss the
/// `mise` use and drop the bootstrap step the script needs at
/// runtime. Detection-only: the detected tokens (`mise`,
/// `install`/`exec`, `<tool>@<version>`, `--`) never contain
/// whitespace by construction, so splitting loses no signal.
pub(crate) fn detector_words(run: &[String]) -> Vec<String> {
    if velnor_actions_workflow_steps::commands::is_inline_shell(run) {
        run[2].split_whitespace().map(unquote_word).collect()
    } else {
        run.iter().map(|arg| unquote_word(arg.as_str())).collect()
    }
}

/// One shell word with a single surrounding quote layer removed.
///
/// The generator quotes joined argv through the one quoter; detection
/// compares the payload, so a drift into quoted specs still bootstraps
/// instead of silently losing the setup step.
fn unquote_word(word: &str) -> String {
    word.strip_prefix('\'')
        .and_then(|inner| inner.strip_suffix('\''))
        .or_else(|| {
            word.strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
        })
        .unwrap_or(word)
        .to_owned()
}
