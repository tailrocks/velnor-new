use super::{normalize, normalize_v40_guard_aliases};

#[test]
fn preserves_quoted_text_and_word_boundaries() {
    assert_ne!(
        normalize("CHECK(state='ready_held')"),
        normalize("CHECK(state='ready_ held')")
    );
    assert_ne!(
        normalize("CHECK(path='source-proof/inbox')"),
        normalize("CHECK(path='source-proof/in box')")
    );
    assert_ne!(
        normalize("CHECK(state='ready_held')"),
        normalize("CHECK(state='readyheld')")
    );
    assert_eq!(
        normalize("CREATE TABLE IF NOT EXISTS x (a TEXT)"),
        normalize("CREATE TABLE x ( a TEXT )")
    );
    assert_eq!(
        normalize("SELECT alpha  \n\t beta"),
        normalize("SELECT alpha beta")
    );
    assert_ne!(
        normalize("SELECT 'alpha  beta'"),
        normalize("SELECT 'alpha beta'")
    );
    assert_eq!(normalize("SELECT 'alpha  beta'"), "select 'alpha  beta'");
    assert_ne!(
        normalize("CREATE TABLE x (a TEXT CHECK(a = 'ready'))"),
        normalize("CREATE TABLE x (a TEXT)")
    );
    let unicode = "SELECT 'é ẞ', \"münchen\"";
    let normalized_unicode = normalize(unicode);
    assert_eq!(normalized_unicode, "select 'é ẞ',\"münchen\"");
    assert_eq!(normalize(&normalized_unicode), normalized_unicode);
    assert_eq!(
        normalize("SELECT 'a''é', \"a\"\"é\""),
        "select 'a''é',\"a\"\"é\""
    );
}

#[test]
fn unique_index_if_not_exists_normalizes_without_weakening_predicates() {
    assert_eq!(
        normalize(
            "CREATE UNIQUE INDEX IF NOT EXISTS runner_id ON runners(connection_id, runner_id) WHERE runner_id IS NOT NULL"
        ),
        normalize(
            "CREATE UNIQUE INDEX runner_id ON runners(connection_id, runner_id) WHERE runner_id IS NOT NULL"
        )
    );
    assert_ne!(
        normalize(
            "CREATE UNIQUE INDEX IF NOT EXISTS runner_id ON runners(connection_id, runner_id) WHERE runner_id IS NOT NULL"
        ),
        normalize(
            "CREATE UNIQUE INDEX IF NOT EXISTS runner_id ON runners(connection_id, runner_id) WHERE runner_id IS NOT NULL AND state != 'cleaned'"
        )
    );
}

#[test]
fn v40_guard_normalization_only_accepts_source_owned_alias_quotes() {
    let source = "SELECT current.id, conflict.reservation_id FROM physical_attempt_current current JOIN physical_attempt_jit_conflict_recovery_claims conflict ON conflict.reservation_id=current.reservation_id";
    let persisted = "SELECT \"current\".id, \"conflict\".reservation_id FROM physical_attempt_current \"current\" JOIN physical_attempt_jit_conflict_recovery_claims \"conflict\" ON \"conflict\".reservation_id=\"current\".reservation_id";
    assert_eq!(
        normalize_v40_guard_aliases(source),
        normalize_v40_guard_aliases(persisted)
    );

    let explicit_as = "SELECT \"current\".id FROM physical_attempt_current AS \"current\"";
    assert_eq!(
        normalize_v40_guard_aliases("SELECT current.id FROM physical_attempt_current AS current"),
        normalize_v40_guard_aliases(explicit_as)
    );

    let nested_current =
        "SELECT \"current\".phase FROM physical_attempt_jit_conflict_recovery_progress \"current\"";
    assert_eq!(
        normalize_v40_guard_aliases(
            "SELECT current.phase FROM physical_attempt_jit_conflict_recovery_progress current"
        ),
        normalize_v40_guard_aliases(nested_current)
    );
}

#[test]
fn v40_guard_normalization_preserves_keywords_literals_and_other_identifiers() {
    for keyword in ["TRUE", "FALSE", "NULL", "OR", "NOT", "COUNT"] {
        assert_ne!(
            normalize_v40_guard_aliases(&format!("SELECT {keyword}")),
            normalize_v40_guard_aliases(&format!("SELECT \"{keyword}\""))
        );
    }
    assert_ne!(
        normalize_v40_guard_aliases("SELECT count(*)"),
        normalize_v40_guard_aliases("SELECT \"count\"(*)")
    );
    assert_ne!(
        normalize_v40_guard_aliases("SELECT current.id FROM physical_attempt_current current"),
        normalize_v40_guard_aliases("SELECT \"current\" FROM physical_attempt_current current")
    );
    assert_ne!(
        normalize_v40_guard_aliases("SELECT current.id FROM physical_attempt_current current"),
        normalize_v40_guard_aliases("SELECT \"current\".id FROM unrelated_table \"current\"")
    );
}

#[test]
fn v40_guard_normalization_keeps_literals_comments_and_token_boundaries() {
    let source =
        "SELECT 'text with \"current\".id', current.id FROM physical_attempt_current current";
    let persisted = "SELECT 'text with \"current\".id', \"current\".id FROM physical_attempt_current \"current\"";
    assert_eq!(
        normalize_v40_guard_aliases(source),
        normalize_v40_guard_aliases(persisted)
    );

    assert_ne!(
        normalize_v40_guard_aliases(source),
        normalize_v40_guard_aliases(
            "SELECT 'text with \"current\".id', current.id FROM physical_attempt_current current -- \"current\".id"
        )
    );
    assert_ne!(
        normalize_v40_guard_aliases(source),
        normalize_v40_guard_aliases(
            "SELECT 'text with \"conflict\".id', current.id FROM physical_attempt_current current"
        )
    );
    assert_ne!(
        normalize_v40_guard_aliases("SELECT current.id FROM physical_attempt_current current"),
        normalize_v40_guard_aliases(
            "SELECT x\"current\".id FROM physical_attempt_current \"current\""
        )
    );
    assert_ne!(
        normalize_v40_guard_aliases("SELECT current.id FROM physical_attempt_current current"),
        normalize_v40_guard_aliases(
            "SELECT \"cur\"\"rent\".id FROM physical_attempt_current \"current\""
        )
    );
    let escaped_identifier = "SELECT \"current\".id FROM physical_attempt_current \"cur\"\"rent\"";
    assert_eq!(
        normalize_v40_guard_aliases(escaped_identifier),
        normalize(escaped_identifier)
    );
    let adjacent_identifier = "SELECT x\"current\".id FROM physical_attempt_current \"current\"";
    assert_eq!(
        normalize_v40_guard_aliases(adjacent_identifier),
        normalize(adjacent_identifier)
    );
    let commented = "SELECT \"current\".id FROM physical_attempt_current \"current\" -- preserve";
    assert_eq!(normalize_v40_guard_aliases(commented), normalize(commented));
}

#[test]
fn v40_aliases_resolve_only_from_direct_table_sources() {
    let column_alias = "SELECT id AS \"current\" FROM physical_attempt_current current";
    assert_eq!(
        normalize_v40_guard_aliases(column_alias),
        normalize(column_alias)
    );

    let column_name_does_not_shadow_correlated_table_alias =
        "SELECT id AS current, \"current\".id FROM physical_attempt_current current";
    assert_eq!(
        normalize_v40_guard_aliases(column_name_does_not_shadow_correlated_table_alias),
        normalize("SELECT id AS current, current.id FROM physical_attempt_current current")
    );

    let alias_outside_source_clause = "SELECT \"current\".id AS value, current AS \"conflict\"";
    assert_eq!(
        normalize_v40_guard_aliases(alias_outside_source_clause),
        normalize(alias_outside_source_clause)
    );
}

#[test]
fn v40_alias_resolution_uses_nearest_select_scope_and_isolates_siblings() {
    let input = "SELECT current.id FROM physical_attempt_current current WHERE EXISTS (SELECT \"current\".id FROM physical_attempt_current current) AND EXISTS (SELECT \"current\".id FROM unrelated current)";
    let expected = "SELECT current.id FROM physical_attempt_current current WHERE EXISTS (SELECT current.id FROM physical_attempt_current current) AND EXISTS (SELECT \"current\".id FROM unrelated current)";
    assert_eq!(normalize_v40_guard_aliases(input), normalize(expected));

    let correlated = "SELECT current.id FROM physical_attempt_current current WHERE EXISTS (SELECT ((\"current\".id)) FROM unrelated_table WHERE EXISTS (SELECT \"current\".id))";
    let correlated_expected = "SELECT current.id FROM physical_attempt_current current WHERE EXISTS (SELECT ((current.id)) FROM unrelated_table WHERE EXISTS (SELECT current.id))";
    assert_eq!(
        normalize_v40_guard_aliases(correlated),
        normalize(correlated_expected)
    );
}

#[test]
fn v40_shadowing_tracks_quoted_and_unquoted_from_join_aliases() {
    let input = "SELECT current.id FROM physical_attempt_current current WHERE EXISTS (SELECT \"current\".id FROM unrelated AS current) AND EXISTS (SELECT \"current\".id FROM unrelated AS \"current\") AND EXISTS (SELECT \"current\".id FROM unrelated AS [current]) AND EXISTS (SELECT \"current\".id FROM unrelated AS `current`)";
    assert_eq!(normalize_v40_guard_aliases(input), normalize(input));

    let join_source = "SELECT \"conflict\".reservation_id FROM unrelated x JOIN physical_attempt_jit_conflict_recovery_claims AS \"conflict\" ON \"conflict\".reservation_id=x.id";
    let join_expected = "SELECT conflict.reservation_id FROM unrelated x JOIN physical_attempt_jit_conflict_recovery_claims AS conflict ON conflict.reservation_id=x.id";
    assert_eq!(
        normalize_v40_guard_aliases(join_source),
        normalize(join_expected)
    );
}

#[test]
fn v40_aliases_fail_closed_for_compound_cte_and_derived_sources() {
    let unsupported = [
        "WITH target AS (SELECT 1) SELECT \"current\".id FROM physical_attempt_current \"current\"",
        "SELECT \"current\".id FROM physical_attempt_current \"current\" UNION SELECT 1",
        "SELECT \"current\".id FROM (SELECT id FROM physical_attempt_current) current",
    ];
    for sql in unsupported {
        assert_eq!(normalize_v40_guard_aliases(sql), normalize(sql));
    }

    let separate_statements = "SELECT \"current\".id FROM physical_attempt_current \"current\"; SELECT \"current\".id FROM unrelated current";
    let expected = "SELECT current.id FROM physical_attempt_current current; SELECT \"current\".id FROM unrelated current";
    assert_eq!(
        normalize_v40_guard_aliases(separate_statements),
        normalize(expected)
    );
}

#[tokio::test]
async fn operator_spacing_keeps_sql_tokens_and_comment_boundaries_distinct() {
    assert_eq!(normalize("SELECT a=b"), normalize("SELECT a = b"));
    assert_eq!(
        normalize("CHECK(a='x'||'y')"),
        normalize("CHECK(a = 'x' || 'y')")
    );
    assert_eq!(
        normalize("CHECK(a=\"x\"+1)"),
        normalize("CHECK(a = \"x\" + 1)")
    );
    assert_ne!(
        normalize("CREATE TABLE x(a INTEGER CHECK(a - -1 >0));"),
        normalize("CREATE TABLE x(a INTEGER CHECK(a--1 >0\n));")
    );
    assert_ne!(normalize("SELECT a==b"), normalize("SELECT a= =b"));
    assert_ne!(
        normalize("SELECT a/* comment */b"),
        normalize("SELECT a / * comment */b")
    );
    assert_ne!(normalize("SELECT 'a=b'"), normalize("SELECT 'a = b'"));

    let database = turso::Builder::new_local(":memory:")
        .build()
        .await
        .expect("build local SQLite database");
    let connection = database.connect().expect("open local connection");
    connection
        .execute(
            "CREATE TABLE spaced_check (value INTEGER CHECK(value - -1 > 0))",
            (),
        )
        .await
        .expect("create arithmetic check");
    connection
        .execute(
            "CREATE TABLE commented_check (value INTEGER CHECK(value--1 > 0\n))",
            (),
        )
        .await
        .expect("create line-comment check");
    assert!(
        connection
            .execute("INSERT INTO spaced_check VALUES (-1)", ())
            .await
            .is_err()
    );
    connection
        .execute("INSERT INTO commented_check VALUES (-1)", ())
        .await
        .expect("comment removes the apparent check expression");
}
