use crate::BunError;
use crate::workspace::Member;

const COMPOUND: [&str; 10] = ["&&", "||", ";", "|", "&", "$(", "`", ">", "\n", "\r"];

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
