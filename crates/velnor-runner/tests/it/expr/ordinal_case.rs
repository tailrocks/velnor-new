// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{empty_env, env_with, truthy};
use anyhow::Result;
use velnor_runner::context_value::{ordinal_ignore_case_cmp, ordinal_ignore_case_eq};
use velnor_runner::expr::RootContext;

fn ypogegrammeni_pairs() -> Vec<(char, char)> {
    let lowercase = (0x1f80..=0x1f87)
        .chain(0x1f90..=0x1f97)
        .chain(0x1fa0..=0x1fa7)
        .chain([0x1fb3, 0x1fc3, 0x1ff3]);
    lowercase
        .filter_map(|code| {
            let lower = char::from_u32(code)?;
            let upper = char::from_u32(if matches!(code, 0x1fb3 | 0x1fc3 | 0x1ff3) {
                code + 9
            } else {
                code + 8
            })?;
            Some((lower, upper))
        })
        .collect()
}

#[test]
fn expr_string_comparison_folds_the_greek_ypogegrammeni_letters() -> Result<()> {
    let env = empty_env();

    assert!(truthy("'ᾀ' == 'ᾈ'", &env)?);
    assert!(truthy("'ᾈ' == 'ᾀ'", &env)?);
    assert!(truthy("contains('xᾀx', 'ᾈ')", &env)?);
    assert!(truthy("startsWith('ᾀx', 'ᾈ')", &env)?);
    let pairs = ypogegrammeni_pairs();
    assert_eq!(pairs.len(), 27);
    for (lower, upper) in pairs {
        assert!(
            ordinal_ignore_case_eq(&lower.to_string(), &upper.to_string()),
            "U+{:04X} and U+{:04X}",
            u32::from(lower),
            u32::from(upper)
        );
    }
    Ok(())
}

#[test]
fn expr_string_comparison_keeps_letters_without_a_simple_uppercase_apart() -> Result<()> {
    let env = empty_env();

    assert!(!truthy("'ß' == 'ẞ'", &env)?);
    assert!(!truthy("'ı' == 'I'", &env)?);
    assert!(!truthy("'ı' == 'i'", &env)?);
    assert!(!truthy("'ſ' == 'S'", &env)?);
    assert!(!truthy("'ſ' == 's'", &env)?);
    assert!(truthy("'ǆ' == 'ǅ'", &env)?);
    assert!(truthy("'ÿ' == 'Ÿ'", &env)?);
    Ok(())
}

#[test]
fn expr_object_keys_fold_the_greek_ypogegrammeni_letters() -> Result<()> {
    let env = env_with(&[(RootContext::Env, r#"{"ᾀ":"found"}"#)])?;

    assert!(truthy("env['ᾈ'] == 'found'", &env)?);
    assert!(ordinal_ignore_case_cmp("ᾀ", "ᾈ").is_eq());
    Ok(())
}

#[test]
fn expr_string_ordering_puts_every_bmp_character_before_a_surrogate_pair() -> Result<()> {
    let json = format!(r#"{{"A":"{}","B":"{}"}}"#, '\u{10000}', '\u{e000}');
    let env = env_with(&[(RootContext::Env, &json)])?;

    assert!(truthy("env.B < env.A", &env)?);
    assert!(truthy("env.A > env.B", &env)?);
    assert!(!truthy("env.A == env.B", &env)?);
    assert!(truthy("'Ａ' < '𐐀'", &env)?);
    assert!(ordinal_ignore_case_cmp("\u{ff21}", "\u{10400}").is_lt());
    Ok(())
}

#[test]
fn expr_string_ordering_folds_surrogate_pairs_before_comparing_them() -> Result<()> {
    let env = empty_env();

    assert!(truthy("'𐐨' == '𐐀'", &env)?);
    assert!(truthy("'𐐨' < '𐐁'", &env)?);
    assert!(truthy("'𐐁' > '𐐨'", &env)?);
    assert!(truthy("'a𐐨' < 'a𐐁'", &env)?);
    assert!(!truthy("'𐐨' > '𐐁'", &env)?);
    Ok(())
}

#[test]
fn expr_contains_matches_at_every_character_boundary_without_folding_to_multiple_characters()
-> Result<()> {
    let env = empty_env();

    assert!(truthy("contains('', '')", &env)?);
    assert!(truthy("contains('abc', '')", &env)?);
    assert!(truthy("contains('aaab', 'aab')", &env)?);
    assert!(truthy("contains('abababc', 'ababc')", &env)?);
    assert!(truthy("contains('ÀᾀB', 'ᾈb')", &env)?);
    assert!(truthy("contains('éa', 'ÉA')", &env)?);
    assert!(truthy("contains('xyé', 'É')", &env)?);
    assert!(truthy("contains('𐐨x', '𐐀X')", &env)?);
    assert!(!truthy("contains('ab', 'abc')", &env)?);
    assert!(!truthy("contains('', 'a')", &env)?);
    assert!(!truthy("contains('straße', 'SS')", &env)?);
    assert!(!truthy("contains('xß', 'ẞ')", &env)?);
    Ok(())
}
