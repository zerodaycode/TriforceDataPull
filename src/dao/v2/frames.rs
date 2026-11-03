//! Game frames: the queue of finished games, the archive and the series built from it, and the
//! live series. Each write is one statement (all or nothing). Canyon only sends single values:
//! the archive goes as hex text, the series as one jsonb document split by the SQL

use std::collections::HashMap;

use canyon_sql::{connection::DbConnection, date_time::NaiveDateTime, query::QueryParameter};
use chrono::{DateTime, Utc};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};
use serde_json::{Value, json};

use crate::frames::{
    box_score::{BoxScore, box_score},
    pack::{Packed, to_hex},
    timeline::{Chunk, LiveState, Side, Timeline},
};

/// Tries before a game is `no_feed`
pub const MAX_TRIES: i32 = 5;

async fn rows(
    sql: &str,
    params: &[&dyn QueryParameter],
    what: &'static str,
) -> Result<canyon_sql::core::CanyonRows> {
    "".query_rows(sql, params)
        .await
        .map_err(|e| eyre!(e))
        .wrap_err(what)
}

/// A finished game whose frames aren't stored yet
#[derive(Debug, Clone, PartialEq)]
pub struct FramesGame {
    pub game_ext_id: i64,
    /// `None` or `live`
    pub frames: Option<String>,
    pub tries: i32,
    /// Last feed frame, when the game details have it
    pub end_time: Option<NaiveDateTime>,
    /// `wait_frames`: the details saw a pause at the end and wait for the frames' box score
    pub details: Option<String>,
    /// The match's teams (ext ids): a feed team not in them is mapped like the details do
    pub event_teams: Vec<i64>,
}

/// Games watched live first (their series are provisional), then untried, then newest.
/// `backfill` off: only matches of the last `RECENT_DAYS` (or without a start), live ones
/// and games whose details wait for the frames
pub async fn next_games(limit: i64, backfill: bool) -> Result<Vec<FramesGame>> {
    let recent_days = crate::backfill::RECENT_DAYS;
    let found = rows(
        "SELECT g.ext_id, g.frames, COALESCE(g.frames_tries, 0), g.end_time, g.details FROM v2_game g JOIN v2_event e ON e.id = g.event_id \
         WHERE g.state = 'completed' AND (g.frames IS NULL OR g.frames = 'live') AND COALESCE(g.frames_tries, 0) < $1::int \
           AND (g.frames_next_try IS NULL OR g.frames_next_try <= now() AT TIME ZONE 'UTC') \
           AND ($3::bool OR g.frames = 'live' OR g.details = 'wait_frames' OR e.start_time IS NULL OR e.start_time >= (now() AT TIME ZONE 'UTC') - make_interval(days => $4::int)) \
           AND (g.end_time IS NULL OR g.end_time <= (now() AT TIME ZONE 'UTC') - interval '2 minutes') \
         ORDER BY CASE WHEN g.frames = 'live' THEN 0 ELSE 1 END, COALESCE(g.frames_tries, 0), e.start_time DESC NULLS LAST, g.ext_id \
         LIMIT $2::bigint",
        &[&MAX_TRIES, &limit, &backfill, &recent_days],
        "v2_game frames queue",
    )
    .await?;
    let mut games = found
        .get_postgres_rows()
        .map_err(|e| eyre!(e))?
        .iter()
        .map(|r| {
            Ok(FramesGame {
                game_ext_id: r.try_get(0).wrap_err("v2_game.ext_id")?,
                frames: r.try_get(1).wrap_err("v2_game.frames")?,
                tries: r.try_get(2).wrap_err("v2_game.frames_tries")?,
                end_time: r.try_get(3).wrap_err("v2_game.end_time")?,
                details: r.try_get(4).wrap_err("v2_game.details")?,
                event_teams: vec![],
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let ids: Vec<i64> = games.iter().map(|g| g.game_ext_id).collect();
    let teams = event_teams(&ids).await?;
    for g in &mut games {
        g.event_teams = teams.get(&g.game_ext_id).cloned().unwrap_or_default();
    }
    Ok(games)
}

/// Games whose details wait for a box score but whose frames were done before (the details
/// came later): the box score comes from the archive, no feed calls
pub async fn box_score_waiting(limit: i64) -> Result<Vec<i64>> {
    let r = rows(
        "SELECT g.ext_id FROM v2_game g WHERE g.details = 'wait_frames' AND g.frames = 'done' \
           AND EXISTS (SELECT 1 FROM v2_game_frames f WHERE f.game_id = g.id) ORDER BY g.ext_id LIMIT $1::bigint",
        &[&limit],
        "v2_game box scores waiting",
    )
    .await?;
    r.get_postgres_rows()
        .map_err(|e| eyre!(e))?
        .iter()
        .map(|row| row.try_get(0).wrap_err("v2_game.ext_id"))
        .collect()
}

/// The match's teams of each game (ext ids, TBD slots left out), by game ext id
pub async fn event_teams(games: &[i64]) -> Result<HashMap<i64, Vec<i64>>> {
    let mut out: HashMap<i64, Vec<i64>> = HashMap::new();
    if games.is_empty() {
        return Ok(out);
    }
    let list: Vec<String> = (1..=games.len()).map(|i| format!("${i}::bigint")).collect();
    let sql = format!(
        "SELECT g.ext_id, t.ext_id FROM v2_game g JOIN v2_event_team et ON et.event_id = g.event_id \
         JOIN v2_team t ON t.id = et.team_id WHERE g.ext_id IN ({}) ORDER BY g.ext_id, et.slot",
        list.join(", ")
    );
    let params: Vec<&dyn QueryParameter> = games.iter().map(|g| g as &dyn QueryParameter).collect();
    let r = rows(&sql, &params, "v2_event_team of frames games").await?;
    for row in r.get_postgres_rows().map_err(|e| eyre!(e))? {
        out.entry(row.try_get(0).wrap_err("v2_game.ext_id")?)
            .or_default()
            .push(row.try_get(1).wrap_err("v2_team.ext_id")?);
    }
    Ok(out)
}

/// One more failed try and its wait; the last one makes the game `no_feed`. Returns the count,
/// 0 when the game isn't waiting for its frames any more (written in the meantime)
pub async fn bump_tries(game_ext_id: i64) -> Result<i32> {
    let r = rows(
        "UPDATE v2_game SET frames_tries = COALESCE(frames_tries, 0) + 1, \
           frames_next_try = (now() AT TIME ZONE 'UTC') + interval '10 minutes' * power(4, COALESCE(frames_tries, 0)), \
           frames = CASE WHEN COALESCE(frames_tries, 0) + 1 >= $2::int THEN 'no_feed' ELSE frames END \
         WHERE ext_id = $1::bigint AND (frames IS NULL OR frames = 'live') RETURNING frames_tries",
        &[&game_ext_id, &MAX_TRIES],
        "v2_game frames_tries",
    )
    .await?;
    let r = r.get_postgres_rows().map_err(|e| eyre!(e))?;
    match r.first() {
        Some(first) => first.try_get(0).wrap_err("v2_game.frames_tries"),
        None => Ok(0),
    }
}

/// The feed has nothing for this game (204)
pub async fn mark_no_feed(game_ext_id: i64) -> Result<()> {
    ""
        .execute(
            "UPDATE v2_game SET frames = 'no_feed' WHERE ext_id = $1::bigint AND (frames IS NULL OR frames = 'live')",
            &[&game_ext_id],
        )
        .await
        .map_err(|e| eyre!(e))
        .wrap_err("v2_game frames no_feed")?;
    Ok(())
}

/// The series of a build as one jsonb document: column names as in the tables
pub fn series_doc(
    blue: i64,
    red: i64,
    tl_head: Option<&TimelineHead>,
    chunk: &Chunk,
) -> Result<Value> {
    let side = |s: Side| match s {
        Side::Blue => "blue",
        Side::Red => "red",
    };
    let teams: Vec<Value> = [(blue, "blue"), (red, "red")]
        .iter()
        .zip(&chunk.teams)
        .map(|((ext, s), c)| {
            let mut v = serde_json::to_value(c).wrap_err("team series")?;
            v["team_ext_id"] = json!(ext);
            v["side"] = json!(s);
            Ok(v)
        })
        .collect::<Result<_>>()?;
    let players: Vec<Value> = chunk
        .players
        .iter()
        .map(|p| {
            let mut v = serde_json::to_value(p).wrap_err("player series")?;
            let skills: Vec<_> = chunk
                .skills
                .iter()
                .filter(|s| s.participant_id == p.participant_id)
                .collect();
            v["skill_order"] = json!(skills.iter().map(|s| &s.skill).collect::<Vec<_>>());
            v["skill_time"] = json!(skills.iter().map(|s| s.game_time_s).collect::<Vec<_>>());
            if let Some(r) = chunk
                .runes
                .iter()
                .find(|r| r.participant_id == p.participant_id)
            {
                v["rune_style"] = json!(r.style);
                v["rune_sub_style"] = json!(r.sub_style);
                v["runes"] = json!(r.perks);
            }
            Ok(v)
        })
        .collect::<Result<_>>()?;
    let markers: Vec<Value> = chunk
        .markers
        .iter()
        .map(|m| json!({"game_time_s": m.game_time_s, "kind": m.kind, "side": side(m.side), "detail": m.detail, "undone_by_pause": m.undone_by_pause}))
        .collect();
    let pauses: Vec<Value> = chunk
        .pauses
        .iter()
        .map(|p| {
            json!({
                "number": p.number,
                "game_time_s": p.game_time_s,
                "started_at": wall(p.started_at),
                "ended_at": p.ended_at.map(wall),
                "rollback_s": p.rollback_s,
                "counters_reset": p.counters_reset,
            })
        })
        .collect();
    let mut rollback_teams: Vec<Value> = vec![];
    let mut rollback_players: Vec<Value> = vec![];
    for p in &chunk.pauses {
        let Some(u) = &p.undone else { continue };
        for (t, s) in u.teams.iter().zip([Side::Blue, Side::Red]) {
            let mut v = serde_json::to_value(t).wrap_err("undone team")?;
            v["number"] = json!(p.number);
            v["side"] = json!(side(s));
            rollback_teams.push(v);
        }
        for pl in &u.players {
            let mut v = serde_json::to_value(pl).wrap_err("undone player")?;
            v["number"] = json!(p.number);
            rollback_players.push(v);
        }
    }
    let wall_time: Vec<String> = chunk
        .wall_time
        .iter()
        .map(|t| t.format(TS).to_string())
        .collect();
    let mut doc = json!({
        "teams": teams,
        "players": players,
        "markers": markers,
        "pauses": pauses,
        "rollback_teams": rollback_teams,
        "rollback_players": rollback_players,
        "wall_time": wall_time,
        "points": chunk.points(),
    });
    if let Some(h) = tl_head {
        doc["step_s"] = json!(h.step_s);
        doc["first_frame"] = json!(h.first_frame.format(TS).to_string());
        doc["duration_s"] = json!(h.duration_s);
    }
    Ok(doc)
}

/// Timestamps in the documents, read back with `::timestamp`
const TS: &str = "%Y-%m-%dT%H:%M:%S%.3f";

fn wall(t: DateTime<Utc>) -> String {
    t.naive_utc().format(TS).to_string()
}

#[derive(Debug, Clone)]
pub struct TimelineHead {
    pub step_s: i32,
    pub first_frame: NaiveDateTime,
    pub duration_s: i64,
}

impl From<&Timeline> for TimelineHead {
    fn from(t: &Timeline) -> Self {
        Self {
            step_s: t.step_s,
            first_frame: t.first_frame,
            duration_s: t.duration_s,
        }
    }
}

const INT_COLS: &[&str] = &[
    "gold",
    "level",
    "kills",
    "deaths",
    "assists",
    "cs",
    "hp",
    "max_hp",
    "gold_earned",
    "wards_placed",
    "wards_destroyed",
    "attack_damage",
    "ability_power",
    "attack_speed",
    "life_steal",
    "armor",
    "magic_resist",
    "items",
    "item_counts",
];
const REAL_COLS: &[&str] = &[
    "kill_participation",
    "damage_share",
    "crit_chance",
    "tenacity",
];
const TEAM_COLS: &[&str] = &["gold", "kills", "towers", "inhibitors", "barons", "dragons"];

/// The CTEs that write a whole build ($1 = game ext id, $2 = the document): timeline, series,
/// markers, pauses and rollbacks replaced, and the box score (`box`) when the game waits for it
/// (`details = 'wait_frames'`) or had a chronobreak. `g` must be defined before. A missing team,
/// player, marker, pause or rollback row, or a team or player row the box score misses, makes the
/// game `incomplete`, which the `v2_game_frames_state` CHECK refuses: the whole statement,
/// archive included, is rolled back. `extra_check` adds conditions to that count (the archive)
fn replace_series_sql(extra_check: &str) -> String {
    let box_set = |cols: &[(&str, &str)]| {
        cols.iter()
            .map(|(c, ty)| format!("{c} = (x->>'{c}')::{ty}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let box_team_set = box_set(&crate::dao::v2::details::TEAM_COLS[2..]);
    let box_player_set = box_set(&crate::dao::v2::details::PLAYER_COLS[6..]);
    let team_cols = TEAM_COLS.join(", ");
    let team_vals = TEAM_COLS
        .iter()
        .map(|c| format!("v2_jsonb_ints(x.j->'{c}')"))
        .collect::<Vec<_>>()
        .join(", ");
    let team_set = TEAM_COLS
        .iter()
        .map(|c| format!("{c} = EXCLUDED.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let player_cols: Vec<&str> = INT_COLS.iter().chain(REAL_COLS).copied().collect();
    let player_vals = INT_COLS
        .iter()
        .map(|c| format!("v2_jsonb_ints(x->'{c}')"))
        .chain(
            REAL_COLS
                .iter()
                .map(|c| format!("v2_jsonb_reals(x->'{c}')")),
        )
        .collect::<Vec<_>>()
        .join(", ");
    let player_set = player_cols
        .iter()
        .chain(&[
            "rune_style",
            "rune_sub_style",
            "runes",
            "skill_order",
            "skill_time",
        ])
        .map(|c| format!("{c} = EXCLUDED.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let player_cols = player_cols.join(", ");
    format!(
        "d AS (SELECT $2::text::jsonb AS j), \
         teams AS (SELECT x->>'side' AS side, t.id AS team_id, x AS j FROM d, jsonb_array_elements(d.j->'teams') x JOIN v2_team t ON t.ext_id = (x->>'team_ext_id')::bigint), \
         tl AS (INSERT INTO v2_game_timeline (game_id, source, step_s, first_frame, duration_s, points, wall_time, live_state, built_at) \
           SELECT g.id, 'archive', (d.j->>'step_s')::smallint, (d.j->>'first_frame')::timestamp, (d.j->>'duration_s')::int, (d.j->>'points')::int, v2_jsonb_times(d.j->'wall_time'), NULL, now() AT TIME ZONE 'UTC' FROM g, d \
           ON CONFLICT (game_id) DO UPDATE SET source = EXCLUDED.source, step_s = EXCLUDED.step_s, first_frame = EXCLUDED.first_frame, duration_s = EXCLUDED.duration_s, points = EXCLUDED.points, wall_time = EXCLUDED.wall_time, live_state = NULL, built_at = EXCLUDED.built_at \
           RETURNING 1), \
         ts AS (INSERT INTO v2_game_team_series (game_id, team_id, side, {team_cols}) \
           SELECT g.id, x.team_id, x.side, {team_vals} FROM g, teams x \
           ON CONFLICT (game_id, team_id) DO UPDATE SET side = EXCLUDED.side, {team_set} RETURNING team_id), \
         ts_old AS (DELETE FROM v2_game_team_series WHERE game_id = (SELECT id FROM g) AND team_id NOT IN (SELECT team_id FROM teams)), \
         ps AS (INSERT INTO v2_game_player_series (game_id, participant_id, {player_cols}, rune_style, rune_sub_style, runes, skill_order, skill_time) \
           SELECT g.id, (x->>'participant_id')::smallint, {player_vals}, (x->>'rune_style')::int, (x->>'rune_sub_style')::int, CASE WHEN x ? 'runes' THEN v2_jsonb_ints(x->'runes') END, v2_jsonb_texts(x->'skill_order'), v2_jsonb_ints(x->'skill_time') \
           FROM g, d, jsonb_array_elements(d.j->'players') x \
           ON CONFLICT (game_id, participant_id) DO UPDATE SET {player_set} RETURNING participant_id), \
         mk_old AS (DELETE FROM v2_game_marker WHERE game_id = (SELECT id FROM g)), \
         pz_old AS (DELETE FROM v2_game_pause WHERE game_id = (SELECT id FROM g) AND number > jsonb_array_length((SELECT j FROM d)->'pauses')), \
         pz AS (INSERT INTO v2_game_pause (game_id, number, game_time_s, started_at, ended_at, rollback_s, counters_reset) \
           SELECT g.id, (x->>'number')::smallint, (x->>'game_time_s')::int, (x->>'started_at')::timestamp, (x->>'ended_at')::timestamp, (x->>'rollback_s')::int, (x->>'counters_reset')::boolean \
           FROM g, d, jsonb_array_elements(d.j->'pauses') x \
           ON CONFLICT (game_id, number) DO UPDATE SET game_time_s = EXCLUDED.game_time_s, started_at = EXCLUDED.started_at, ended_at = EXCLUDED.ended_at, \
             rollback_s = EXCLUDED.rollback_s, counters_reset = EXCLUDED.counters_reset RETURNING 1), \
         rbt_new AS (SELECT (x->>'number')::smallint AS number, teams.team_id, x FROM d, jsonb_array_elements(d.j->'rollback_teams') x JOIN teams ON teams.side = x->>'side'), \
         rbt_old AS (DELETE FROM v2_game_rollback_team WHERE game_id = (SELECT id FROM g) AND (number, team_id) NOT IN (SELECT number, team_id FROM rbt_new)), \
         rbt AS (INSERT INTO v2_game_rollback_team (game_id, number, team_id, kills, gold, towers, inhibitors, barons, dragons) \
           SELECT g.id, r.number, r.team_id, (r.x->>'kills')::int, (r.x->>'gold')::int, (r.x->>'towers')::int, (r.x->>'inhibitors')::int, (r.x->>'barons')::int, (r.x->>'dragons')::int FROM g, rbt_new r \
           ON CONFLICT (game_id, number, team_id) DO UPDATE SET kills = EXCLUDED.kills, gold = EXCLUDED.gold, towers = EXCLUDED.towers, inhibitors = EXCLUDED.inhibitors, \
             barons = EXCLUDED.barons, dragons = EXCLUDED.dragons RETURNING 1), \
         rbp_new AS (SELECT (x->>'number')::smallint AS number, (x->>'participant_id')::smallint AS participant_id, x FROM d, jsonb_array_elements(d.j->'rollback_players') x), \
         rbp_old AS (DELETE FROM v2_game_rollback_player WHERE game_id = (SELECT id FROM g) AND (number, participant_id) NOT IN (SELECT number, participant_id FROM rbp_new)), \
         rbp AS (INSERT INTO v2_game_rollback_player (game_id, number, participant_id, kills, deaths, assists, gold, cs) \
           SELECT g.id, r.number, r.participant_id, (r.x->>'kills')::int, (r.x->>'deaths')::int, (r.x->>'assists')::int, (r.x->>'gold')::int, (r.x->>'cs')::int FROM g, rbp_new r \
           ON CONFLICT (game_id, number, participant_id) DO UPDATE SET kills = EXCLUDED.kills, deaths = EXCLUDED.deaths, assists = EXCLUDED.assists, gold = EXCLUDED.gold, cs = EXCLUDED.cs RETURNING 1), \
         mk AS (INSERT INTO v2_game_marker (game_id, game_time_s, kind, team_id, detail, undone_by_pause) \
           SELECT g.id, (x->>'game_time_s')::int, x->>'kind', teams.team_id, x->>'detail', (x->>'undone_by_pause')::smallint FROM g, d, jsonb_array_elements(d.j->'markers') x JOIN teams ON teams.side = x->>'side' RETURNING 1), \
         box_on AS (SELECT COALESCE((d.j ? 'box') AND v2_game.details IN ('wait_frames', 'done') \
             AND (v2_game.details = 'wait_frames' OR (d.j->'box'->>'chronobreak')::boolean), false) AS on_ \
           FROM d, g JOIN v2_game ON v2_game.id = g.id), \
         bt AS (UPDATE v2_game_team gt SET {box_team_set} \
           FROM d, jsonb_array_elements(d.j->'box'->'teams') x \
           WHERE (SELECT on_ FROM box_on) AND gt.game_id = (SELECT id FROM g) AND gt.side = x->>'side' RETURNING 1), \
         bp AS (UPDATE v2_game_player gp SET {box_player_set} \
           FROM d, jsonb_array_elements(d.j->'box'->'players') x \
           WHERE (SELECT on_ FROM box_on) AND gp.game_id = (SELECT id FROM g) AND gp.participant_id = (x->>'participant_id')::int RETURNING 1) \
         UPDATE v2_game SET \
           end_time = CASE WHEN (SELECT on_ FROM box_on) THEN ((SELECT j FROM d)->'box'->>'end_time')::timestamp ELSE end_time END, \
           details = CASE WHEN (SELECT on_ FROM box_on) THEN 'done' ELSE details END, \
           frames = CASE WHEN {extra_check}(SELECT count(*) FROM tl) = 1 AND (SELECT count(*) FROM ts) = 2 AND (SELECT count(*) FROM ps) = 10 \
           AND (SELECT count(*) FROM mk) = jsonb_array_length((SELECT j FROM d)->'markers') \
           AND (SELECT count(*) FROM pz) = jsonb_array_length((SELECT j FROM d)->'pauses') \
           AND (SELECT count(*) FROM rbt) = jsonb_array_length((SELECT j FROM d)->'rollback_teams') \
           AND (SELECT count(*) FROM rbp) = jsonb_array_length((SELECT j FROM d)->'rollback_players') \
           AND (NOT (SELECT on_ FROM box_on) \
             OR ((SELECT count(*) FROM bt) = 2 \
               AND (SELECT count(*) FROM bp) = (SELECT count(*) FROM v2_game_player WHERE game_id = (SELECT id FROM g)))) \
           THEN 'done' ELSE 'incomplete' END \
         WHERE id = (SELECT id FROM g) RETURNING frames"
    )
}

/// The box score in the series document: `box_on` in the SQL decides if it is written
fn box_doc(b: &BoxScore) -> Result<Value> {
    Ok(json!({
        "end_time": b.end_time.format(TS).to_string(),
        "chronobreak": b.chronobreak,
        "teams": serde_json::to_value(&b.teams).wrap_err("box teams")?,
        "players": serde_json::to_value(&b.players).wrap_err("box players")?,
    }))
}

fn final_sql() -> String {
    format!(
        "WITH g AS (SELECT id FROM v2_game WHERE ext_id = $1::bigint FOR UPDATE), \
         arch AS (INSERT INTO v2_game_frames (game_id, endpoint, format, frame_count, first_frame, last_frame, raw_bytes, fetched_at, data) \
           SELECT g.id, a.endpoint, a.format, a.frame_count, a.first_frame, a.last_frame, a.raw_bytes, now() AT TIME ZONE 'UTC', decode(a.hex, 'hex') \
           FROM g, (VALUES ('window', $3::smallint, $4::int, $5::timestamp, $6::timestamp, $7::int, $8::text), \
                           ('details', $9::smallint, $10::int, $11::timestamp, $12::timestamp, $13::int, $14::text)) \
                AS a(endpoint, format, frame_count, first_frame, last_frame, raw_bytes, hex) \
           ON CONFLICT (game_id, endpoint) DO UPDATE SET format = EXCLUDED.format, frame_count = EXCLUDED.frame_count, first_frame = EXCLUDED.first_frame, \
             last_frame = EXCLUDED.last_frame, raw_bytes = EXCLUDED.raw_bytes, fetched_at = EXCLUDED.fetched_at, data = EXCLUDED.data RETURNING 1), \
         {} ",
        replace_series_sql("(SELECT count(*) FROM arch) = 2 AND ")
    )
}

fn rebuild_sql() -> String {
    format!(
        "WITH g AS (SELECT id FROM v2_game WHERE ext_id = $1::bigint FOR UPDATE), {}",
        replace_series_sql("")
    )
}

/// The `frames` value the write returned must be `done`
fn check_done(r: &canyon_sql::core::CanyonRows, game_ext_id: i64) -> Result<()> {
    let r = r.get_postgres_rows().map_err(|e| eyre!(e))?;
    let Some(row) = r.first() else {
        return Err(eyre!("game {game_ext_id} not stored"));
    };
    let state: Option<String> = row.try_get(0).wrap_err("v2_game.frames")?;
    if state.as_deref() == Some("done") {
        Ok(())
    } else {
        Err(eyre!("game {game_ext_id}: frames not written ({state:?})"))
    }
}

/// The archive (window, details) and the series built from it, `frames = 'done'`. Nothing is
/// written if any part is missing (an unknown team, a statement error)
pub async fn write_final(
    game_ext_id: i64,
    window: &Packed,
    details: &Packed,
    blue: i64,
    red: i64,
    tl: &Timeline,
) -> Result<()> {
    let mut doc = series_doc(blue, red, Some(&tl.into()), &tl.chunk)?;
    doc["box"] = box_doc(&box_score(tl)?)?;
    let doc = doc.to_string();
    let hex = [to_hex(&window.data), to_hex(&details.data)];
    let firsts = [window.first.naive_utc(), details.first.naive_utc()];
    let lasts = [window.last.naive_utc(), details.last.naive_utc()];
    let format = crate::frames::pack::FORMAT;
    let params: Vec<&dyn QueryParameter> = vec![
        &game_ext_id,
        &doc,
        &format,
        &window.frame_count,
        &firsts[0],
        &lasts[0],
        &window.raw_bytes,
        &hex[0],
        &format,
        &details.frame_count,
        &firsts[1],
        &lasts[1],
        &details.raw_bytes,
        &hex[1],
    ];
    let r = rows(&final_sql(), &params, "v2 game frames")
        .await
        .wrap_err_with(|| {
            format!("game {game_ext_id}: nothing written (an unknown team or a row count off)")
        })?;
    check_done(&r, game_ext_id)
}

/// The series again from the archive (another step, a new field): no feed calls
pub async fn write_rebuild(game_ext_id: i64, blue: i64, red: i64, tl: &Timeline) -> Result<()> {
    let mut doc = series_doc(blue, red, Some(&tl.into()), &tl.chunk)?;
    doc["box"] = box_doc(&box_score(tl)?)?;
    let doc = doc.to_string();
    let r = rows(
        &rebuild_sql(),
        &[&game_ext_id, &doc],
        "v2 game series rebuild",
    )
    .await
    .wrap_err_with(|| format!("game {game_ext_id}: nothing written"))?;
    check_done(&r, game_ext_id)
}

/// The archive of a game: (endpoint, format, data)
pub async fn read_archive(game_ext_id: i64) -> Result<Vec<(String, i16, Vec<u8>)>> {
    let r = rows(
        "SELECT f.endpoint, f.format, encode(f.data, 'hex') FROM v2_game_frames f JOIN v2_game g ON g.id = f.game_id \
         WHERE g.ext_id = $1::bigint ORDER BY f.endpoint",
        &[&game_ext_id],
        "v2_game_frames",
    )
    .await?;
    r.get_postgres_rows()
        .map_err(|e| eyre!(e))?
        .iter()
        .map(|row| {
            let hex: String = row.try_get(2).wrap_err("v2_game_frames.data")?;
            Ok((
                row.try_get(0).wrap_err("v2_game_frames.endpoint")?,
                row.try_get(1).wrap_err("v2_game_frames.format")?,
                crate::frames::pack::from_hex(&hex)?,
            ))
        })
        .collect()
}

/// Every game with an archive
pub async fn archived_games() -> Result<Vec<i64>> {
    let r = rows(
        "SELECT g.ext_id FROM v2_game g WHERE EXISTS (SELECT 1 FROM v2_game_frames f WHERE f.game_id = g.id AND f.endpoint = 'window') ORDER BY g.ext_id",
        &[],
        "v2_game_frames games",
    )
    .await?;
    r.get_postgres_rows()
        .map_err(|e| eyre!(e))?
        .iter()
        .map(|row| row.try_get(0).wrap_err("v2_game.ext_id"))
        .collect()
}

/// Games being played whose frames the live loop may write, with where it is
pub async fn live_games() -> Result<Vec<(i64, Option<LiveState>)>> {
    let r = rows(
        "SELECT g.ext_id, t.live_state::text FROM v2_game g LEFT JOIN v2_game_timeline t ON t.game_id = g.id AND t.source = 'live' \
         WHERE g.state = 'inProgress' AND (g.frames IS NULL OR g.frames = 'live') ORDER BY g.ext_id",
        &[],
        "v2_game live frames",
    )
    .await?;
    r.get_postgres_rows()
        .map_err(|e| eyre!(e))?
        .iter()
        .map(|row| {
            let state: Option<String> = row.try_get(1).wrap_err("v2_game_timeline.live_state")?;
            let state = state
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .wrap_err("v2_game_timeline.live_state")?;
            Ok((row.try_get(0).wrap_err("v2_game.ext_id")?, state))
        })
        .collect()
}

fn live_sql() -> String {
    let team_cols = TEAM_COLS.join(", ");
    let team_vals = TEAM_COLS
        .iter()
        .map(|c| format!("v2_jsonb_ints(x.j->'{c}')"))
        .collect::<Vec<_>>()
        .join(", ");
    let team_add = TEAM_COLS
        .iter()
        .map(|c| format!("{c} = v2_game_team_series.{c} || EXCLUDED.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let player_cols: Vec<&str> = INT_COLS.iter().chain(REAL_COLS).copied().collect();
    let player_vals = INT_COLS
        .iter()
        .map(|c| format!("v2_jsonb_ints(x->'{c}')"))
        .chain(
            REAL_COLS
                .iter()
                .map(|c| format!("v2_jsonb_reals(x->'{c}')")),
        )
        .collect::<Vec<_>>()
        .join(", ");
    let player_add = player_cols
        .iter()
        .chain(&["skill_order", "skill_time"])
        .map(|c| format!("{c} = v2_game_player_series.{c} || EXCLUDED.{c}"))
        .chain(
            ["rune_style", "rune_sub_style", "runes"]
                .iter()
                .map(|c| format!("{c} = COALESCE(v2_game_player_series.{c}, EXCLUDED.{c})")),
        )
        .collect::<Vec<_>>()
        .join(", ");
    let player_cols = player_cols.join(", ");
    format!(
        "WITH g AS (SELECT id FROM v2_game WHERE ext_id = $1::bigint AND state = 'inProgress' AND (frames IS NULL OR frames = 'live') FOR UPDATE), \
         d AS (SELECT $2::text::jsonb AS j), \
         teams AS (SELECT x->>'side' AS side, t.id AS team_id, x AS j FROM d, jsonb_array_elements(d.j->'teams') x JOIN v2_team t ON t.ext_id = (x->>'team_ext_id')::bigint), \
         tl AS (INSERT INTO v2_game_timeline (game_id, source, step_s, first_frame, duration_s, points, wall_time, live_state, built_at) \
           SELECT g.id, 'live', (d.j->>'step_s')::smallint, (d.j->>'first_frame')::timestamp, (d.j->>'duration_s')::int, (d.j->>'points')::int, v2_jsonb_times(d.j->'wall_time'), $3::text::jsonb, now() AT TIME ZONE 'UTC' FROM g, d \
           ON CONFLICT (game_id) DO UPDATE SET points = v2_game_timeline.points + EXCLUDED.points, wall_time = v2_game_timeline.wall_time || EXCLUDED.wall_time, \
             duration_s = EXCLUDED.duration_s, live_state = EXCLUDED.live_state, built_at = EXCLUDED.built_at WHERE v2_game_timeline.source = 'live' \
           RETURNING 1), \
         ts AS (INSERT INTO v2_game_team_series (game_id, team_id, side, {team_cols}) \
           SELECT g.id, x.team_id, x.side, {team_vals} FROM g, teams x WHERE EXISTS (SELECT 1 FROM tl) \
           ON CONFLICT (game_id, team_id) DO UPDATE SET {team_add} RETURNING team_id), \
         ps AS (INSERT INTO v2_game_player_series (game_id, participant_id, {player_cols}, rune_style, rune_sub_style, runes, skill_order, skill_time) \
           SELECT g.id, (x->>'participant_id')::smallint, {player_vals}, (x->>'rune_style')::int, (x->>'rune_sub_style')::int, CASE WHEN x ? 'runes' THEN v2_jsonb_ints(x->'runes') END, v2_jsonb_texts(x->'skill_order'), v2_jsonb_ints(x->'skill_time') \
           FROM g, d, jsonb_array_elements(d.j->'players') x WHERE EXISTS (SELECT 1 FROM tl) \
           ON CONFLICT (game_id, participant_id) DO UPDATE SET {player_add} RETURNING participant_id), \
         mk AS (INSERT INTO v2_game_marker (game_id, game_time_s, kind, team_id, detail) \
           SELECT g.id, (x->>'game_time_s')::int, x->>'kind', teams.team_id, x->>'detail' FROM g, d, jsonb_array_elements(d.j->'markers') x JOIN teams ON teams.side = x->>'side' \
           WHERE EXISTS (SELECT 1 FROM tl) RETURNING 1), \
         pz AS (INSERT INTO v2_game_pause (game_id, number, game_time_s, started_at, ended_at, rollback_s, counters_reset) \
           SELECT g.id, (x->>'number')::smallint, (x->>'game_time_s')::int, (x->>'started_at')::timestamp, (x->>'ended_at')::timestamp, (x->>'rollback_s')::int, (x->>'counters_reset')::boolean \
           FROM g, d, jsonb_array_elements(d.j->'pauses') x WHERE EXISTS (SELECT 1 FROM tl) \
           ON CONFLICT (game_id, number) DO UPDATE SET ended_at = EXCLUDED.ended_at, counters_reset = EXCLUDED.counters_reset RETURNING 1) \
         UPDATE v2_game SET frames = CASE WHEN (SELECT count(*) FROM tl) = 0 THEN frames \
           WHEN (SELECT count(*) FROM ts) = 2 AND (SELECT count(*) FROM ps) = 10 AND (SELECT count(*) FROM mk) = jsonb_array_length((SELECT j FROM d)->'markers') \
             AND (SELECT count(*) FROM pz) = jsonb_array_length((SELECT j FROM d)->'pauses') THEN 'live' ELSE 'incomplete' END \
         WHERE id = (SELECT id FROM g) RETURNING (SELECT count(*) FROM tl)"
    )
}

/// New live points appended (the first tick creates the rows), `frames = 'live'`. 0 when the
/// game can't take live points any more (not `inProgress`, `done`, `no_feed`, or built from the
/// archive). Only while `inProgress`: the switch to `completed` takes the same row lock, so a
/// live write always ends before the after-game write can start
pub async fn write_live(
    game_ext_id: i64,
    blue: i64,
    red: i64,
    chunk: &Chunk,
    state: &LiveState,
) -> Result<u64> {
    let first = state
        .first_frame()
        .ok_or_else(|| eyre!("game {game_ext_id}: live state without frames"))?
        .naive_utc();
    let head = TimelineHead {
        step_s: state.step_s(),
        first_frame: first,
        duration_s: state.duration_s(),
    };
    let doc = series_doc(blue, red, Some(&head), chunk)?.to_string();
    let state = serde_json::to_string(state).wrap_err("live state")?;
    let r = rows(
        &live_sql(),
        &[&game_ext_id, &doc, &state],
        "v2 game live frames",
    )
    .await
    .wrap_err_with(|| format!("game {game_ext_id}: live points not written"))?;
    match r.get_postgres_rows().map_err(|e| eyre!(e))?.first() {
        None => Ok(0),
        Some(row) => {
            let n: i64 = row.try_get(0).wrap_err("live rows")?;
            Ok(u64::try_from(n).unwrap_or_default())
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        dao::v2::{events::write_events, rows::*},
        frames::{
            RawFrame, hand,
            pack::pack,
            timeline::{Builder, STEP_S, build},
        },
    };
    use chrono::TimeDelta;

    // DB tests: ext ids -499..-400
    #[test]
    fn frames_sql_shape() {
        let f = final_sql();
        assert!(f.contains(
            "CASE WHEN (SELECT count(*) FROM arch) = 2 AND (SELECT count(*) FROM tl) = 1"
        ));
        assert!(f.contains("$14::text") && !f.contains("$15"));
        let r = rebuild_sql();
        assert!(!r.contains("FROM arch") && r.contains("CASE WHEN (SELECT count(*) FROM tl) = 1"));
        let l = live_sql();
        assert!(l.contains("THEN 'live' ELSE 'incomplete' END"));
        assert!(
            l.contains("state = 'inProgress' AND (frames IS NULL OR frames = 'live') FOR UPDATE")
        );
    }

    /// The columns the SQL reads are the fields the series are built with
    #[test]
    fn sql_columns_match_the_series_structs() {
        use crate::frames::timeline::{PlayerCols, TeamCols};
        let keys = |v: Value, skip: &str| {
            let mut k: Vec<String> = v
                .as_object()
                .unwrap()
                .keys()
                .filter(|k| *k != skip)
                .cloned()
                .collect();
            k.sort_unstable();
            k
        };
        let sorted = |c: Vec<&str>| {
            let mut c: Vec<String> = c.into_iter().map(str::to_string).collect();
            c.sort_unstable();
            c
        };
        assert_eq!(
            keys(
                serde_json::to_value(PlayerCols::default()).unwrap(),
                "participant_id"
            ),
            sorted(INT_COLS.iter().chain(REAL_COLS).copied().collect())
        );
        assert_eq!(
            keys(serde_json::to_value(TeamCols::default()).unwrap(), ""),
            sorted(TEAM_COLS.to_vec())
        );
    }

    /// Any one-number query, read as `bigint`
    pub(crate) async fn count(sql: &str) -> i64 {
        "".query_one_for::<i64>(&format!("SELECT ({sql})::bigint"), &[])
            .await
            .unwrap()
    }

    pub(crate) async fn one(sql: &str) -> String {
        "".query_one_for::<String>(sql, &[]).await.unwrap()
    }

    fn event(ext: i64, games: &[(i64, &str)]) -> EventRow {
        EventRow {
            ext_id: ext,
            kind: "match",
            league_ext_id: -401,
            tournament_ext_id: None,
            start_time: None,
            state: "completed".into(),
            strategy_type: Some("bestOf".into()),
            strategy_count: Some(3),
            match_type: None,
            block_name: None,
            teams: vec![
                EventTeamRow {
                    slot: 1,
                    team_ext_id: Some(-402),
                    game_wins: Some(1),
                    outcome: None,
                },
                EventTeamRow {
                    slot: 2,
                    team_ext_id: Some(-403),
                    game_wins: Some(1),
                    outcome: None,
                },
            ],
            games: games
                .iter()
                .enumerate()
                .map(|(i, (g, s))| GameRow {
                    ext_id: *g,
                    number: i as i32 + 1,
                    state: s.to_string(),
                })
                .collect(),
            streams: vec![],
        }
    }

    pub(crate) async fn setup() {
        crate::dao::v2::schema::apply().await.unwrap();
        crate::dao::v2::test_cleanup().await;
        let rows = EventRows {
            events: vec![
                event(-410, &[(-420, "completed"), (-421, "completed")]),
                event(-411, &[(-422, "completed"), (-423, "inProgress")]),
            ],
            league_stubs: vec![LeagueStub {
                ext_id: -401,
                slug: "l".into(),
                name: "L".into(),
                image_url: "i".into(),
            }],
            team_stubs: vec![
                TeamStub {
                    ext_id: -402,
                    name: "Blue Team".into(),
                    code: "BT".into(),
                    image_url: "i".into(),
                },
                TeamStub {
                    ext_id: -403,
                    name: "Red Team".into(),
                    code: "RT".into(),
                    image_url: "i".into(),
                },
            ],
            skipped: 0,
        };
        write_events(&rows, "en-US").await.unwrap();
    }

    fn walked(seconds: i64) -> (Vec<RawFrame>, Vec<RawFrame>) {
        let (w, d) = hand::game(seconds, true);
        let mut w = w;
        // a tower and a dragon for the markers
        w[30]["blueTeam"]["towers"] = serde_json::json!(1);
        for f in &mut w[31..] {
            f["blueTeam"]["towers"] = serde_json::json!(1);
            f["redTeam"]["dragons"] = serde_json::json!(["ocean"]);
        }
        (
            w.iter().map(hand::raw).collect(),
            d.iter().map(hand::raw).collect(),
        )
    }

    async fn write_game(game: i64, seconds: i64) -> Timeline {
        let (w, d) = walked(seconds);
        let meta = serde_json::value::RawValue::from_string(hand::metadata().to_string()).unwrap();
        let pw = pack(Some(&meta), &w).unwrap();
        let pd = pack(None, &d).unwrap();
        let tl = build(&w, &d, STEP_S).unwrap();
        write_final(game, &pw, &pd, -402, -403, &tl).await.unwrap();
        tl
    }

    pub(crate) fn lineups(game: i64) -> crate::dao::v2::rows::GameDetailsRow {
        use crate::dao::v2::rows::*;
        let team = |ext, side| GameTeamRow {
            team_ext_id: ext,
            side,
            gold: Some(1),
            kills: Some(99),
            towers: None,
            inhibitors: None,
            barons: None,
            dragons: None,
        };
        GameDetailsRow {
            game_ext_id: game,
            patch: Some("16.20".into()),
            start_time: None,
            end_time: None,
            teams: vec![team(-402, "blue"), team(-403, "red")],
            players: (0..10)
                .map(|i| GamePlayerRow {
                    team_ext_id: if i < 5 { -402 } else { -403 },
                    player_ext_id: None,
                    summoner_name: format!("p{i}"),
                    participant_id: i + 1,
                    role: "top".into(),
                    champion: "Volibear".into(),
                    level: None,
                    kills: Some(99),
                    deaths: None,
                    assists: None,
                    gold: None,
                    cs: None,
                    wards_placed: None,
                    wards_destroyed: None,
                    damage_share_bp: None,
                    items: None,
                    rune_style: None,
                    rune_sub_style: None,
                    runes: None,
                    skill_order: None,
                })
                .collect(),
            player_stubs: vec![],
            remapped: None,
        }
    }

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn box_score_for_waiting_and_chronobreak_games() {
        use crate::dao::v2::details::{DetailsState, write_details};
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        setup().await;
        // waiting game: the frames write fills the numbers and marks the details done
        write_details(&lineups(-420).without_numbers(), DetailsState::WaitFrames)
            .await
            .unwrap();
        // backfill off and an old match: a waiting game is still picked
        "".execute("UPDATE v2_event SET start_time = (now() AT TIME ZONE 'UTC') - interval '10 days' WHERE ext_id = -410", &[]).await.unwrap();
        let q = next_games(100_000, false).await.unwrap();
        assert!(
            q.iter()
                .any(|g| g.game_ext_id == -420 && g.details.as_deref() == Some("wait_frames"))
        );
        assert!(!q.iter().any(|g| g.game_ext_id == -421));
        let tl = write_game(-420, 95).await;
        let b = crate::frames::box_score::box_score(&tl).unwrap();
        assert_eq!(
            one("SELECT details FROM v2_game WHERE ext_id = -420").await,
            "done"
        );
        assert_eq!(
            one("SELECT end_time::text FROM v2_game WHERE ext_id = -420").await,
            b.end_time.to_string()
        );
        assert_eq!(count("SELECT gold FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -420 AND gt.side = 'blue'").await, b.teams[0].gold.unwrap());
        assert_eq!(one("SELECT dragons FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -420 AND gt.side = 'red'").await, "ocean");
        assert_eq!(count("SELECT count(*) FROM v2_game_player gp JOIN v2_game g ON g.id = gp.game_id WHERE g.ext_id = -420 AND gp.level IS NOT NULL AND gp.champion = 'Volibear'").await, 10);
        // done details, no chronobreak: left alone
        write_details(&lineups(-421), DetailsState::Done)
            .await
            .unwrap();
        write_game(-421, 95).await;
        assert_eq!(count("SELECT kills FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -421 AND gt.side = 'blue'").await, 99);
        // details not written yet: frames done, nothing else touched
        write_game(-422, 95).await;
        assert_eq!(count("SELECT count(*) FROM v2_game WHERE ext_id = -422 AND details IS NULL AND frames = 'done'").await, 1);
        // details set to wait after the frames were done: listed for a box score from the archive
        write_details(&lineups(-422).without_numbers(), DetailsState::WaitFrames)
            .await
            .unwrap();
        assert!(box_score_waiting(100).await.unwrap().contains(&-422));
        assert!(!box_score_waiting(100).await.unwrap().contains(&-420));
    }

    /// A stored team row missing for the timeline's: the whole write rolls back
    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn box_score_missing_a_team_rolls_back() {
        use crate::dao::v2::details::{DetailsState, write_details};
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        setup().await;
        write_details(&lineups(-421).without_numbers(), DetailsState::WaitFrames)
            .await
            .unwrap();
        "".execute("DELETE FROM v2_game_team WHERE side = 'red' AND game_id = (SELECT id FROM v2_game WHERE ext_id = -421)", &[]).await.unwrap();
        let (w, d) = walked(95);
        let meta = serde_json::value::RawValue::from_string(hand::metadata().to_string()).unwrap();
        let pw = pack(Some(&meta), &w).unwrap();
        let pd = pack(None, &d).unwrap();
        let tl = build(&w, &d, STEP_S).unwrap();
        assert!(write_final(-421, &pw, &pd, -402, -403, &tl).await.is_err());
        assert_eq!(count("SELECT count(*) FROM v2_game_frames f JOIN v2_game g ON g.id = f.game_id WHERE g.ext_id = -421").await, 0);
        assert_eq!(count("SELECT count(*) FROM v2_game WHERE ext_id = -421 AND frames IS NULL AND details = 'wait_frames'").await, 1);
        crate::dao::v2::test_cleanup().await;
    }

    const SERIES: &str = "(SELECT array_length(gold, 1) FROM v2_game_team_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -420 AND s.side = 'blue')";

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn final_write_rebuild_and_queue() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        setup().await;
        // the 3 completed test games are in the queue, the live one isn't
        let q = next_games(100_000, true).await.unwrap();
        for g in [-420, -421, -422] {
            assert!(q.iter().any(|x| x.game_ext_id == g), "{g}");
        }
        assert!(!q.iter().any(|x| x.game_ext_id == -423));
        // archive, timeline, 2 team + 10 player series, markers, done
        let tl = write_game(-420, 95).await;
        assert_eq!(count("SELECT count(*) FROM v2_game_frames f JOIN v2_game g ON g.id = f.game_id WHERE g.ext_id = -420").await, 2);
        assert_eq!(count(SERIES).await, tl.points);
        assert_eq!(count("SELECT count(*) FROM v2_game_player_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -420").await, 10);
        assert_eq!(count("SELECT count(*) FROM v2_game_marker m JOIN v2_game g ON g.id = m.game_id WHERE g.ext_id = -420").await, 2);
        assert_eq!(
            one("SELECT frames FROM v2_game WHERE ext_id = -420").await,
            "done"
        );
        assert_eq!(one("SELECT source || ' ' || points || ' ' || duration_s FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -420").await, format!("archive {} 95", tl.points));
        // the arrays keep their values, nulls and the flat items
        assert_eq!(one("SELECT (gold[1])::text || ',' || (gold[11])::text FROM v2_game_team_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -420 AND s.side = 'blue'").await, "2500,3450");
        assert_eq!(count("SELECT array_length(item_counts, 1) FROM v2_game_player_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -420 AND participant_id = 1").await, tl.points);
        assert_eq!(count("SELECT array_length(items, 1) FROM v2_game_player_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -420 AND participant_id = 1").await, count("SELECT sum(c) FROM v2_game_player_series s JOIN v2_game g ON g.id = s.game_id, unnest(s.item_counts) c WHERE g.ext_id = -420 AND participant_id = 1").await);
        assert_eq!(one("SELECT array_to_string(runes, ',') || ' ' || rune_style FROM v2_game_player_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -420 AND participant_id = 7").await, "8005,9111 8000");
        assert_eq!(one("SELECT kind || ' ' || detail || ' ' || game_time_s FROM v2_game_marker m JOIN v2_game g ON g.id = m.game_id JOIN v2_team tm ON tm.id = m.team_id WHERE g.ext_id = -420 AND tm.ext_id = -403").await, "dragon ocean 31");
        // the archive reads back to the same frames
        let arch = read_archive(-420).await.unwrap();
        assert_eq!(arch.len(), 2);
        let (_, w2) = crate::frames::pack::unpack(arch[1].1, &arch[1].2).unwrap();
        assert_eq!(w2, walked(95).0);
        assert!(archived_games().await.unwrap().contains(&-420));
        // done games leave the queue
        assert!(
            !next_games(100_000, true)
                .await
                .unwrap()
                .iter()
                .any(|x| x.game_ext_id == -420)
        );
        // a rebuild with the same frames gives the same rows
        let before = one("SELECT md5(string_agg(s::text, '|' ORDER BY participant_id)) FROM v2_game_player_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -420").await;
        write_rebuild(-420, -402, -403, &tl).await.unwrap();
        assert_eq!(one("SELECT md5(string_agg(s::text, '|' ORDER BY participant_id)) FROM v2_game_player_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -420").await, before);
        assert_eq!(count("SELECT count(*) FROM v2_game_marker m JOIN v2_game g ON g.id = m.game_id WHERE g.ext_id = -420").await, 2);
        // another step replaces them
        let (w, d) = walked(95);
        let tl5 = build(&w, &d, 5).unwrap();
        write_rebuild(-420, -402, -403, &tl5).await.unwrap();
        assert_eq!(count(SERIES).await, tl5.points);
        assert_eq!(count("SELECT step_s FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -420").await, 5);
        // an unknown team: nothing written at all
        let e = write_final(
            -421,
            &pack(None, &w).unwrap(),
            &pack(None, &d).unwrap(),
            -402,
            -499,
            &tl,
        )
        .await
        .unwrap_err();
        assert!(
            format!("{e:#}").contains("frames not written")
                || format!("{e:#}").contains("nothing written"),
            "{e:#}"
        );
        assert_eq!(count("SELECT count(*) FROM v2_game_frames f JOIN v2_game g ON g.id = f.game_id WHERE g.ext_id = -421").await, 0);
        assert_eq!(count("SELECT count(*) FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -421").await, 0);
        assert_eq!(
            count("SELECT count(*) FROM v2_game WHERE ext_id = -421 AND frames IS NULL").await,
            1
        );
        // an unknown game
        assert!(write_rebuild(-498, -402, -403, &tl).await.is_err());
        // tries wait, the 5th makes it no_feed
        let in_q = |q: Vec<FramesGame>| q.iter().any(|g| g.game_ext_id == -421);
        assert!(in_q(next_games(100_000, true).await.unwrap()));
        for i in 1..=MAX_TRIES {
            assert_eq!(bump_tries(-421).await.unwrap(), i);
            assert!(!in_q(next_games(100_000, true).await.unwrap()));
            "".execute(
                "UPDATE v2_game SET frames_next_try = NULL WHERE ext_id = -421",
                &[],
            )
            .await
            .unwrap();
        }
        assert_eq!(
            one("SELECT frames FROM v2_game WHERE ext_id = -421").await,
            "no_feed"
        );
        mark_no_feed(-422).await.unwrap();
        assert_eq!(
            one("SELECT frames FROM v2_game WHERE ext_id = -422").await,
            "no_feed"
        );
        // mark_no_feed never undoes done
        mark_no_feed(-420).await.unwrap();
        assert_eq!(
            one("SELECT frames FROM v2_game WHERE ext_id = -420").await,
            "done"
        );
        crate::dao::v2::test_cleanup().await;
    }

    /// The real chronobreak (G2-MKOI 2025-09-14 game 3): live writes its pause first, then the
    /// archive writes the pause again with the rollback, then a rebuild writes it all a third
    /// time. Pauses and rollback rows are updated in place, never twice
    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn pauses_and_a_chronobreak_written_again() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        setup().await;
        let (meta, w) =
            crate::frames::parse_frames(fixture!("feed/frames/chronobreak_window.json")).unwrap();
        let (_, d) =
            crate::frames::parse_frames(fixture!("feed/frames/chronobreak_details.json")).unwrap();
        // live: everything up to a minute after the restart, one push
        let mut b = Builder::new(STEP_S);
        let chunk = b.push(&w, &d).unwrap();
        assert_eq!(
            write_live(-423, -402, -403, &chunk, b.state())
                .await
                .unwrap(),
            1
        );
        let pause = "SELECT p.number || ' ' || p.game_time_s || ' ' || coalesce(p.rollback_s::text, '-') || ' ' || p.counters_reset || ' ' || (p.ended_at IS NOT NULL) FROM v2_game_pause p JOIN v2_game g ON g.id = p.game_id WHERE g.ext_id = -423";
        assert_eq!(one(pause).await, "1 1587 - true true");
        "".execute(
            "UPDATE v2_game SET state = 'completed' WHERE ext_id = -423",
            &[],
        )
        .await
        .unwrap();
        let meta = meta.unwrap();
        let pw = pack(Some(&meta), &w).unwrap();
        let pd = pack(None, &d).unwrap();
        let tl = build(&w, &d, STEP_S).unwrap();
        for _ in 0..2 {
            write_final(-423, &pw, &pd, -402, -403, &tl).await.unwrap();
            assert_eq!(
                one("SELECT frames FROM v2_game WHERE ext_id = -423").await,
                "done"
            );
            assert_eq!(one(pause).await, "1 1587 8 true true");
            assert_eq!(count("SELECT count(*) FROM v2_game_pause p JOIN v2_game g ON g.id = p.game_id WHERE g.ext_id = -423").await, 1);
            assert_eq!(one("SELECT kills || ' ' || gold FROM v2_game_rollback_team r JOIN v2_game g ON g.id = r.game_id JOIN v2_team t ON t.id = r.team_id WHERE g.ext_id = -423 AND t.ext_id = -402").await, "1 805");
            assert_eq!(count("SELECT count(*) FROM v2_game_rollback_player r JOIN v2_game g ON g.id = r.game_id WHERE g.ext_id = -423").await, 10);
        }
        // a rebuild without the pause (a fix in how pauses are found) takes its rows away
        let mut no_pause = tl.clone();
        no_pause.chunk.pauses.clear();
        write_final(-423, &pw, &pd, -402, -403, &no_pause)
            .await
            .unwrap();
        assert_eq!(count("SELECT count(*) FROM v2_game_pause p JOIN v2_game g ON g.id = p.game_id WHERE g.ext_id = -423").await, 0);
        assert_eq!(count("SELECT count(*) FROM v2_game_rollback_team r JOIN v2_game g ON g.id = r.game_id WHERE g.ext_id = -423").await, 0);
        crate::dao::v2::test_cleanup().await;
    }

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn live_points_then_the_archive() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        setup().await;
        "".execute("UPDATE v2_event SET start_time = (now() AT TIME ZONE 'UTC') - interval '10 days' WHERE ext_id = -410", &[]).await.unwrap();
        // backfill off keeps old matches out
        assert!(
            !next_games(100_000, false)
                .await
                .unwrap()
                .iter()
                .any(|g| g.game_ext_id == -421)
        );
        // the in-progress game is the live one
        let live = live_games().await.unwrap();
        assert_eq!(live, vec![(-423, None)]);
        let (w, d) = walked(95);
        let at = |s: i64| hand::start() + TimeDelta::seconds(s);
        let mut b = Builder::new(STEP_S);
        let mut written = 0;
        // ticks of 10 s, the one at 40-50 missed
        for slice in 0..9 {
            if slice == 4 {
                continue;
            }
            let pick = |f: &&RawFrame| f.ts >= at(slice * 10) && f.ts < at(slice * 10 + 10);
            let ws: Vec<RawFrame> = w.iter().filter(pick).cloned().collect();
            let ds: Vec<RawFrame> = d.iter().filter(pick).cloned().collect();
            let chunk = b.push(&ws, &ds).unwrap();
            written += chunk.points();
            assert_eq!(
                write_live(-423, -402, -403, &chunk, b.state())
                    .await
                    .unwrap(),
                1
            );
            // the same tick again adds nothing
            let again = b.push(&ws, &ds).unwrap();
            assert_eq!(again.points(), 0);
            assert_eq!(
                write_live(-423, -402, -403, &again, b.state())
                    .await
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            one("SELECT frames FROM v2_game WHERE ext_id = -423").await,
            "live"
        );
        let len = |sql: &'static str| async move { count(sql).await };
        assert_eq!(len("SELECT array_length(gold, 1) FROM v2_game_team_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -423 AND side = 'red'").await, written as i64);
        assert_eq!(len("SELECT points FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -423").await, written as i64);
        assert_eq!(len("SELECT array_length(wall_time, 1) FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -423").await, written as i64);
        assert_eq!(len("SELECT array_length(skill_order, 1) FROM v2_game_player_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -423 AND participant_id = 1").await, 1);
        // the state saved in the DB goes on where it was
        let saved = live_games().await.unwrap();
        assert_eq!(saved[0].1.as_ref(), Some(b.state()));
        // the game ends: the archive replaces the live points
        "".execute(
            "UPDATE v2_game SET state = 'completed' WHERE ext_id = -423",
            &[],
        )
        .await
        .unwrap();
        let q = next_games(100_000, false).await.unwrap();
        assert_eq!(
            q.first().map(|g| (g.game_ext_id, g.frames.clone())),
            Some((-423, Some("live".to_string())))
        );
        let tl = write_game(-423, 95).await;
        assert_eq!(len("SELECT array_length(gold, 1) FROM v2_game_team_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -423 AND side = 'red'").await, tl.points);
        assert_eq!(one("SELECT source FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -423").await, "archive");
        assert_eq!(count("SELECT count(*) FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -423 AND live_state IS NULL").await, 1);
        assert_eq!(count("SELECT count(*) FROM v2_game_team_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -423 AND array_position(gold, NULL) IS NOT NULL").await, 0);
        assert_eq!(count("SELECT count(*) FROM v2_game_marker m JOIN v2_game g ON g.id = m.game_id WHERE g.ext_id = -423").await, 2);
        // a done game never takes live points again
        "".execute(
            "UPDATE v2_game SET state = 'inProgress' WHERE ext_id = -423",
            &[],
        )
        .await
        .unwrap();
        assert!(live_games().await.unwrap().is_empty());
        let chunk = Builder::new(STEP_S).push(&w[..5], &d[..5]).unwrap();
        let mut b2 = Builder::new(STEP_S);
        b2.push(&w[..5], &d[..5]).unwrap();
        assert_eq!(
            write_live(-423, -402, -403, &chunk, b2.state())
                .await
                .unwrap(),
            0
        );
        assert_eq!(one("SELECT source FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -423").await, "archive");
        crate::dao::v2::test_cleanup().await;
    }

    /// The live loop and the end of the game at the same time: both lock `v2_game` first, no
    /// deadlock, `done` always wins and no live marker stays
    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn live_and_final_at_once() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        setup().await;
        let (w, d) = walked(95);
        let meta = serde_json::value::RawValue::from_string(hand::metadata().to_string()).unwrap();
        let pw = pack(Some(&meta), &w).unwrap();
        let pd = pack(None, &d).unwrap();
        let tl = build(&w, &d, STEP_S).unwrap();
        for _ in 0..5 {
            "".execute("DELETE FROM v2_game_marker WHERE game_id = (SELECT id FROM v2_game WHERE ext_id = -423)", &[]).await.unwrap();
            for t in [
                "v2_game_team_series",
                "v2_game_player_series",
                "v2_game_timeline",
                "v2_game_frames",
            ] {
                "".execute(&format!("DELETE FROM {t} WHERE game_id = (SELECT id FROM v2_game WHERE ext_id = -423)"), &[]).await.unwrap();
            }
            "".execute("UPDATE v2_game SET frames = NULL WHERE ext_id = -423", &[])
                .await
                .unwrap();
            "".execute(
                "UPDATE v2_game SET state = 'inProgress' WHERE ext_id = -423",
                &[],
            )
            .await
            .unwrap();
            let mut b = Builder::new(STEP_S);
            let chunk = b.push(&w[..40], &d[..40]).unwrap();
            // the game ends (the events writer takes the row lock) while a live tick writes
            let finish = async {
                "".execute(
                    "UPDATE v2_game SET state = 'completed' WHERE ext_id = -423",
                    &[],
                )
                .await
                .unwrap();
                write_final(-423, &pw, &pd, -402, -403, &tl).await
            };
            let (live, fin) = tokio::join!(write_live(-423, -402, -403, &chunk, b.state()), finish);
            live.unwrap();
            fin.unwrap();
            assert_eq!(
                one("SELECT frames FROM v2_game WHERE ext_id = -423").await,
                "done"
            );
            assert_eq!(one("SELECT source FROM v2_game_timeline t JOIN v2_game g ON g.id = t.game_id WHERE g.ext_id = -423").await, "archive");
            assert_eq!(count("SELECT array_length(gold, 1) FROM v2_game_team_series s JOIN v2_game g ON g.id = s.game_id WHERE g.ext_id = -423 AND side = 'blue'").await, tl.points);
            // no live marker left next to the archive's
            assert_eq!(count("SELECT count(*) FROM v2_game_marker m JOIN v2_game g ON g.id = m.game_id WHERE g.ext_id = -423").await, tl.chunk.markers.len() as i64);
        }
        crate::dao::v2::test_cleanup().await;
    }

    /// every SQL example in docs/data/game-frames.md runs
    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "reads the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn data_doc_examples_run() {
        let doc = include_str!("../../../docs/data/game-frames.md");
        let examples: Vec<&str> = doc
            .split("```sql\n")
            .skip(1)
            .map(|b| b.split("```").next().unwrap())
            .collect();
        assert_eq!(examples.len(), 7);
        for sql in examples {
            // inside FROM only a query runs: a write in the doc fails instead of running
            let wrapped = format!(
                "SELECT count(*) FROM ({}) q",
                sql.trim().trim_end_matches(';')
            );
            let r = "".query_rows(&wrapped, &[]).await;
            assert!(r.is_ok(), "{sql}\n{:?}", r.err());
        }
    }
}
