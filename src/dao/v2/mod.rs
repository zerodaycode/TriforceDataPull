pub mod details;
pub mod events;
pub mod frames;
pub mod models;
pub mod reference;
pub mod rows;
pub mod schema;
pub mod upsert;

/// DB tests change shared constraints/rows, so they run one at a time
#[cfg(test)]
pub(crate) static DB_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Removes every test row (negative ext_ids, `test%` jobs), children first so no FK stops it
#[cfg(test)]
pub(crate) async fn test_cleanup() {
    use canyon_sql::connection::DbConnection;
    const EVENTS: &str = "(SELECT id FROM v2_event WHERE ext_id < 0)";
    const GAMES: &str = "(SELECT id FROM v2_game WHERE event_id IN (SELECT id FROM v2_event WHERE ext_id < 0) OR ext_id < 0)";
    for sql in [
        format!("DELETE FROM v2_game_marker WHERE game_id IN {GAMES} OR team_id IN (SELECT id FROM v2_team WHERE ext_id < 0)"),
        format!("DELETE FROM v2_game_pause WHERE game_id IN {GAMES}"),
        format!("DELETE FROM v2_game_team_series WHERE game_id IN {GAMES} OR team_id IN (SELECT id FROM v2_team WHERE ext_id < 0)"),
        format!("DELETE FROM v2_game_player_series WHERE game_id IN {GAMES}"),
        format!("DELETE FROM v2_game_timeline WHERE game_id IN {GAMES}"),
        format!("DELETE FROM v2_game_frames WHERE game_id IN {GAMES}"),
        format!("DELETE FROM v2_game_player WHERE game_id IN (SELECT id FROM v2_game WHERE event_id IN {EVENTS} OR ext_id < 0) OR player_id IN (SELECT id FROM v2_player WHERE ext_id < 0)"),
        format!("DELETE FROM v2_game_team WHERE game_id IN (SELECT id FROM v2_game WHERE event_id IN {EVENTS} OR ext_id < 0) OR team_id IN (SELECT id FROM v2_team WHERE ext_id < 0)"),
        format!("DELETE FROM v2_stream WHERE event_id IN {EVENTS}"),
        format!("DELETE FROM v2_event_i18n WHERE event_id IN {EVENTS}"),
        format!("DELETE FROM v2_event_team WHERE event_id IN {EVENTS} OR team_id IN (SELECT id FROM v2_team WHERE ext_id < 0)"),
        format!("DELETE FROM v2_game WHERE event_id IN {EVENTS} OR ext_id < 0"),
        "DELETE FROM v2_event WHERE ext_id < 0 OR league_id IN (SELECT id FROM v2_league WHERE ext_id < 0)".into(),
        "DELETE FROM v2_team_player WHERE team_id IN (SELECT id FROM v2_team WHERE ext_id < 0)".into(),
        "DELETE FROM v2_player WHERE ext_id < 0".into(),
        "DELETE FROM v2_team WHERE ext_id < 0".into(),
        "DELETE FROM v2_stage_i18n WHERE stage_id IN (SELECT id FROM v2_stage WHERE ext_id < 0)".into(),
        "DELETE FROM v2_stage WHERE ext_id < 0".into(),
        "DELETE FROM v2_tournament_i18n WHERE tournament_id IN (SELECT id FROM v2_tournament WHERE ext_id < 0)".into(),
        "DELETE FROM v2_tournament WHERE ext_id < 0".into(),
        "DELETE FROM v2_split WHERE ext_id < 0".into(),
        "DELETE FROM v2_season_i18n WHERE season_id IN (SELECT id FROM v2_season WHERE ext_id < 0)".into(),
        "DELETE FROM v2_season WHERE ext_id < 0".into(),
        "DELETE FROM v2_league_i18n WHERE league_id IN (SELECT id FROM v2_league WHERE ext_id < 0)".into(),
        "DELETE FROM v2_league WHERE ext_id < 0".into(),
        "DELETE FROM v2_job_run WHERE job LIKE 'test%'".into(),
    ] {
        "".execute(&sql, &[]).await.unwrap();
    }
}
