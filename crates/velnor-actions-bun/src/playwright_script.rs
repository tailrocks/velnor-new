//! Admissibility of a script a sharded Playwright suite runs.
//!
//! A leg runs `bun run <script> --project=<p> --shard=<i>/<n>`; Bun appends
//! the trailing arguments to the script's command. They reach Playwright only
//! when the script is one Playwright command. This check reads the
//! `package.json` text and runs nothing; the leg's blob verification proves
//! at run time that the arguments arrived (a wrapper that drops them fails
//! there, not silently).

use crate::BunError;
use crate::workspace::Member;

/// Shell syntax that would split the command, so appended arguments would
/// reach only its last part (or a pipe, redirect or subshell).
const COMPOUND: [&str; 10] = ["&&", "||", ";", "|", "&", "$(", "`", ">", "\n", "\r"];

/// Reject a script a sharded suite cannot safely append flags to.
///
/// # Errors
///
/// - `bun_shard_script_name_shadow:<member>`: the member has a script named
///   `playwright`, which would shadow the report job's `bun run playwright`;
/// - `bun_script_missing:<member>:<script>`: no such script;
/// - `bun_shard_script_compound:<script>`: `&&`, `||`, `;`, `|`, `&`, `$(`, a
///   backtick, `>` or a newline;
/// - `bun_shard_script_not_playwright:<script>`: no `playwright test` token pair;
/// - `bun_shard_script_sets_shard:<script>`: the text passes `--shard` or `--project`;
/// - `bun_shard_script_updates_snapshots:<script>`: `-u` or `--update-snapshots`
///   (CI never writes goldens).
pub fn check_shard_script(member: &Member, script: &str) -> Result<(), BunError> {
    if member.scripts.contains("playwright") {
        return Err(BunError::new("bun_shard_script_name_shadow", &member.name));
    }
    let text = member
        .commands
        .get(script)
        .ok_or_else(|| BunError::new("bun_script_missing", &format!("{}:{script}", member.name)))?;
    let fail = |code: &str| Err(BunError::new(code, script));
    if COMPOUND.iter().any(|token| text.contains(token)) {
        return fail("bun_shard_script_compound");
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if !words
        .windows(2)
        .any(|pair| pair[0] == "playwright" && pair[1] == "test")
    {
        return fail("bun_shard_script_not_playwright");
    }
    let flag = |word: &str, name: &str| {
        word == name
            || word
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('='))
    };
    if words
        .iter()
        .any(|word| flag(word, "--shard") || flag(word, "--project"))
    {
        return fail("bun_shard_script_sets_shard");
    }
    if words
        .iter()
        .any(|word| *word == "-u" || flag(word, "--update-snapshots"))
    {
        return fail("bun_shard_script_updates_snapshots");
    }
    Ok(())
}
