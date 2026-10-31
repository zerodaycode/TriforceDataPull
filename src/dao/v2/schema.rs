//! SQL Canyon can't express, run at every start

use canyon_sql::connection::DbConnection;
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};

pub const SQL: &str = include_str!("schema.sql");

/// One entry per statement, each ends at a line ending in `;`. Lines starting with `--` never
/// start or end a statement. Canyon's `execute` runs a single prepared statement
pub fn statements(sql: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut offset = 0;
    for line in sql.split_inclusive('\n') {
        let trimmed = line.trim();
        let comment = trimmed.starts_with("--");
        if start.is_none() && !trimmed.is_empty() && !comment {
            start = Some(offset);
        }
        offset += line.len();
        if let Some(s) = start
            && !comment
            && trimmed.ends_with(';')
        {
            out.push(sql[s..offset].trim());
            start = None;
        }
    }
    // a last statement without `;` still goes out: Postgres reports it
    if let Some(s) = start {
        out.push(sql[s..].trim());
    }
    out
}

pub async fn apply() -> Result<()> {
    for stmt in statements(SQL) {
        "".execute(stmt, &[])
            .await
            .map_err(|e| eyre!(e))
            .wrap_err_with(|| format!("v2 schema: {}", stmt.lines().next().unwrap_or(stmt)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use canyon_sql::connection::DbConnection;

    #[test]
    fn splits_into_statements() {
        let s = statements(
            "-- comment\nCREATE INDEX a ON t (x);\nALTER TABLE t\n  ADD CONSTRAINT c CHECK (x > 0);\n",
        );
        assert_eq!(
            s,
            vec![
                "CREATE INDEX a ON t (x);",
                "ALTER TABLE t\n  ADD CONSTRAINT c CHECK (x > 0);"
            ]
        );
        assert_eq!(statements(SQL).len(), 51);
    }

    #[test]
    fn checks_match_the_rust_lists() {
        use crate::dao::v2::rows::*;
        for (name, values) in [
            ("v2_event_state_check", EVENT_STATES),
            ("v2_game_state_check", GAME_STATES),
            ("v2_event_strategy", STRATEGIES),
            ("v2_event_match_type", MATCH_TYPES),
            ("v2_event_team_outcome", OUTCOMES),
        ] {
            let line = SQL
                .lines()
                .find(|l| l.contains(&format!("ADD CONSTRAINT {name} ")))
                .unwrap();
            let list = &line[line.find(" IN (").unwrap()..];
            assert_eq!(list.matches('\'').count(), values.len() * 2, "{name}");
            for v in values {
                assert!(list.contains(&format!("'{v}'")), "{name}: {v}");
            }
        }
    }

    #[test]
    fn comments_never_split_or_start() {
        let s = statements("ALTER TABLE t\n-- note;\n  ADD CONSTRAINT c CHECK (x > 0);\n");
        assert_eq!(s.len(), 1);
        assert!(s[0].ends_with("CHECK (x > 0);"));
    }

    #[test]
    fn last_statement_without_semicolon() {
        assert_eq!(
            statements("CREATE INDEX a ON t (x)"),
            vec!["CREATE INDEX a ON t (x)"]
        );
    }

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn apply_twice_keeps_everything() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        apply().await.unwrap();
        apply().await.unwrap();
        let n: i64 = "".query_one_for(
            "SELECT count(*) FROM pg_indexes WHERE schemaname = 'public' AND indexname LIKE 'v2\\_%' AND indexname NOT LIKE '%\\_pkey'",
            &[],
        ).await?;
        assert_eq!(n, 32);
        let c: i64 = ""
            .query_one_for(
                "SELECT count(*) FROM pg_constraint WHERE contype = 'c' AND conname LIKE 'v2\\_%'",
                &[],
            )
            .await?;
        assert_eq!(c, 18);
        // the SQL ranks are the Rust ones
        for st in ["unstarted", "inProgress", "completed", "unneeded", "other"] {
            let db: i32 = ""
                .query_one_for("SELECT v2_state_rank($1)", &[&st.to_string()])
                .await?;
            assert_eq!(db, crate::dao::v2::rows::state_rank(st), "{st}");
        }
    }

    /// Every foreign key Canyon made from `#[foreign_key(references = ...)]`, as
    /// `table.column>referenced table`. Catches a Canyon update that resolves them differently
    const FOREIGN_KEYS: &str = "schedule.league_id>league,schedule.team_left_id>team,\
        stream.event_id>schedule,team.home_league>league,team_player.player_id>player,\
        team_player.team_id>team,tournament.league>league,v2_event.league_id>v2_league,\
        v2_event.tournament_id>v2_tournament,v2_event_i18n.event_id>v2_event,\
        v2_event_team.event_id>v2_event,v2_event_team.team_id>v2_team,v2_game.event_id>v2_event,\
        v2_game_player.game_id>v2_game,v2_game_player.player_id>v2_player,\
        v2_game_player.team_id>v2_team,v2_game_team.game_id>v2_game,v2_game_team.team_id>v2_team,\
        v2_league_i18n.league_id>v2_league,v2_season_i18n.season_id>v2_season,\
        v2_stage.tournament_id>v2_tournament,v2_stage_i18n.stage_id>v2_stage,\
        v2_stream.event_id>v2_event,v2_team.home_league_id>v2_league,\
        v2_team_player.player_id>v2_player,v2_team_player.team_id>v2_team,\
        v2_tournament.league_id>v2_league,v2_tournament.season_id>v2_season,\
        v2_tournament.split_id>v2_split,v2_tournament_i18n.tournament_id>v2_tournament";

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "reads the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn foreign_keys_point_at_their_tables() {
        let fks: String = ""
            .query_one_for(
                "SELECT string_agg(k, ',' ORDER BY k) FROM (
                     SELECT c.conrelid::regclass || '.' || a.attname || '>' || c.confrelid::regclass AS k
                     FROM pg_constraint c
                     JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1]
                     WHERE c.contype = 'f') f",
                &[],
            )
            .await?;
        assert_eq!(fks, FOREIGN_KEYS);
    }
}
