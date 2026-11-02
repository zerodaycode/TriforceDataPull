//! Game details: the queue of finished games, and their sides, lineups and last frame

use std::collections::HashMap;

use canyon_sql::{connection::DbConnection, query::QueryParameter};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};

use super::{
    rows::{GameDetailsRow, GameSides, LiveGameRow, PlayerStub},
    upsert::{Col, MAX_PARAMS, Upsert, Val, run, t},
};

/// Tries before a game leaves the queue
pub const MAX_TRIES: i32 = 5;

/// A finished game without details, and the teams of its match (ext_id, code)
#[derive(Debug, Clone, PartialEq)]
pub struct QueueGame {
    pub game_ext_id: i64,
    pub event_ext_id: i64,
    pub teams: Vec<(i64, String)>,
    /// Failed tries so far
    pub tries: i32,
}

/// Feed players we don't have: added with what the feed knows, never changed from here
const PLAYER_STUB: Upsert = Upsert {
    table: "v2_player",
    cols: &[
        Col::Plain("ext_id"),
        Col::Plain("handle"),
        Col::Plain("first_name"),
        Col::Plain("last_name"),
        Col::Plain("role"),
    ],
    conflict: &[0],
    update: false,
    guard: None,
};

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

/// Finished games without details, with tries left and past their wait, untried and newest
/// match first. `backfill` off: only matches of the last `RECENT_DAYS` (or without a start time)
pub async fn next_games(limit: i64, backfill: bool) -> Result<Vec<QueueGame>> {
    let recent_days = crate::backfill::RECENT_DAYS;
    let found = rows(
        "SELECT g.ext_id, e.ext_id, COALESCE(g.details_tries, 0) FROM v2_game g JOIN v2_event e ON e.id = g.event_id \
         WHERE g.state = 'completed' AND g.details IS NULL AND COALESCE(g.details_tries, 0) < $1::int \
           AND (g.details_next_try IS NULL OR g.details_next_try <= now() AT TIME ZONE 'UTC') \
           AND ($3::bool OR e.start_time IS NULL OR e.start_time >= (now() AT TIME ZONE 'UTC') - make_interval(days => $4::int)) \
         ORDER BY COALESCE(g.details_tries, 0), e.start_time DESC NULLS LAST, e.ext_id, g.number \
         LIMIT $2::bigint",
        &[&MAX_TRIES, &limit, &backfill, &recent_days],
        "v2_game queue",
    )
    .await?;
    let mut games = Vec::new();
    for r in found.get_postgres_rows().map_err(|e| eyre!(e))? {
        games.push(QueueGame {
            game_ext_id: r.try_get(0).wrap_err("v2_game.ext_id")?,
            event_ext_id: r.try_get(1).wrap_err("v2_event.ext_id")?,
            teams: vec![],
            tries: r.try_get(2).wrap_err("v2_game.details_tries")?,
        });
    }
    if games.is_empty() {
        return Ok(games);
    }
    let events: Vec<i64> = games.iter().map(|g| g.event_ext_id).collect();
    let list: Vec<String> = (1..=events.len())
        .map(|i| format!("${i}::bigint"))
        .collect();
    let sql = format!(
        "SELECT e.ext_id, t.ext_id, t.code FROM v2_event_team et \
         JOIN v2_event e ON e.id = et.event_id JOIN v2_team t ON t.id = et.team_id \
         WHERE e.ext_id IN ({}) ORDER BY e.ext_id, et.slot",
        list.join(", ")
    );
    let params: Vec<&dyn QueryParameter> =
        events.iter().map(|x| x as &dyn QueryParameter).collect();
    let mut teams: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    for r in rows(&sql, &params, "v2_event_team of the queue")
        .await?
        .get_postgres_rows()
        .map_err(|e| eyre!(e))?
    {
        teams
            .entry(r.try_get(0).wrap_err("v2_event.ext_id")?)
            .or_default()
            .push((
                r.try_get(1).wrap_err("v2_team.ext_id")?,
                r.try_get(2).wrap_err("v2_team.code")?,
            ));
    }
    for g in &mut games {
        g.teams = teams.get(&g.event_ext_id).cloned().unwrap_or_default();
    }
    Ok(games)
}

pub async fn write_player_stubs(stubs: &[PlayerStub]) -> Result<()> {
    run(
        &PLAYER_STUB,
        stubs
            .iter()
            .map(|p| {
                vec![
                    Val::I64(Some(p.ext_id)),
                    t(&p.handle),
                    t(""),
                    t(""),
                    t(&p.role),
                ]
            })
            .collect(),
    )
    .await
    .map(|_| ())
}

const TEAM_COLS: [(&str, &str); 8] = [
    ("team_ext", "bigint"),
    ("side", "text"),
    ("gold", "bigint"),
    ("kills", "int"),
    ("towers", "int"),
    ("inhibitors", "int"),
    ("barons", "int"),
    ("dragons", "text"),
];
const PLAYER_COLS: [(&str, &str); 20] = [
    ("team_ext", "bigint"),
    ("player_ext", "bigint"),
    ("summoner_name", "text"),
    ("participant_id", "int"),
    ("role", "text"),
    ("champion", "text"),
    ("level", "int"),
    ("kills", "int"),
    ("deaths", "int"),
    ("assists", "int"),
    ("gold", "bigint"),
    ("cs", "int"),
    ("wards_placed", "int"),
    ("wards_destroyed", "int"),
    ("damage_share_bp", "int"),
    ("items", "text"),
    ("rune_style", "int"),
    ("rune_sub_style", "int"),
    ("runes", "text"),
    ("skill_order", "text"),
];

/// `VALUES (...), (...)` with casts, numbered from `next`
fn values(rows: usize, cols: &[(&str, &str)], next: &mut usize) -> String {
    (0..rows)
        .map(|_| {
            let one: Vec<String> = cols
                .iter()
                .map(|(_, ty)| {
                    *next += 1;
                    format!("${}::{ty}", *next)
                })
                .collect();
            format!("({})", one.join(", "))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn names(cols: &[(&str, &str)]) -> String {
    cols.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
}

/// Teams, players and the game in one statement. When a team or a player is missing the game
/// gets `incomplete`, which the `v2_game_details` CHECK refuses: the whole statement, rows of
/// the CTEs included, is rolled back. A player the feed has no id for goes in with `player_id` NULL
fn details_sql(players: usize) -> String {
    // $1 game, $2 patch, $3 start, $4 end
    let mut n = 4;
    let teams = values(2, &TEAM_COLS, &mut n);
    let people = values(players, &PLAYER_COLS, &mut n);
    let team_cols: Vec<&str> = TEAM_COLS[1..].iter().map(|(c, _)| *c).collect();
    let team_set: Vec<String> = team_cols
        .iter()
        .map(|c| format!("{c} = EXCLUDED.{c}"))
        .chain(["side_source = 'feed'".to_string()])
        .collect();
    let player_cols: Vec<&str> = PLAYER_COLS[2..].iter().map(|(c, _)| *c).collect();
    let player_set: Vec<String> = ["team_id", "player_id"]
        .iter()
        .chain(player_cols.iter())
        .map(|c| format!("{c} = EXCLUDED.{c}"))
        .collect();
    format!(
        "WITH g AS (SELECT id FROM v2_game WHERE ext_id = $1::bigint FOR UPDATE), \
         gt AS (INSERT INTO v2_game_team (game_id, team_id, side_source, {tcols}) \
           SELECT g.id, t.id, 'feed', {tvcols} \
           FROM g, (VALUES {teams}) AS v ({}) JOIN v2_team t ON t.ext_id = v.team_ext \
           ON CONFLICT (game_id, team_id) DO UPDATE SET {tset} RETURNING 1), \
         gp AS (INSERT INTO v2_game_player (game_id, team_id, player_id, {pcols}) \
           SELECT g.id, t.id, p.id, {vcols} \
           FROM g, (VALUES {people}) AS v ({}) JOIN v2_team t ON t.ext_id = v.team_ext \
           LEFT JOIN v2_player p ON p.ext_id = v.player_ext \
           WHERE v.player_ext IS NULL OR p.id IS NOT NULL \
           ON CONFLICT (game_id, participant_id) DO UPDATE SET {pset} RETURNING 1) \
         UPDATE v2_game SET patch = $2::text, start_time = $3::timestamp, end_time = $4::timestamp, \
           details = CASE WHEN (SELECT count(*) FROM gt) = 2 AND (SELECT count(*) FROM gp) = {players} \
             THEN 'done' ELSE 'incomplete' END \
         FROM g WHERE v2_game.id = g.id",
        names(&TEAM_COLS),
        names(&PLAYER_COLS),
        pcols = player_cols.join(", "),
        vcols = player_cols
            .iter()
            .map(|c| format!("v.{c}"))
            .collect::<Vec<_>>()
            .join(", "),
        pset = player_set.join(", "),
        tcols = team_cols.join(", "),
        tvcols = team_cols
            .iter()
            .map(|c| format!("v.{c}"))
            .collect::<Vec<_>>()
            .join(", "),
        tset = team_set.join(", "),
    )
}

/// One game, all or nothing (one statement). `Err` when the game wasn't marked done
pub async fn write_details(row: &GameDetailsRow) -> Result<()> {
    if row.teams.len() != 2 || row.players.is_empty() {
        return Err(eyre!(
            "game {}: {} teams, {} players",
            row.game_ext_id,
            row.teams.len(),
            row.players.len()
        ));
    }
    let mut vals: Vec<Val> = vec![
        Val::I64(Some(row.game_ext_id)),
        Val::Text(row.patch.clone()),
        Val::Time(row.start_time),
        Val::Time(row.end_time),
    ];
    for x in &row.teams {
        vals.extend([
            Val::I64(Some(x.team_ext_id)),
            t(x.side),
            Val::I64(x.gold),
            Val::I32(x.kills),
            Val::I32(x.towers),
            Val::I32(x.inhibitors),
            Val::I32(x.barons),
            Val::Text(x.dragons.clone()),
        ]);
    }
    for p in &row.players {
        vals.extend([
            Val::I64(Some(p.team_ext_id)),
            Val::I64(p.player_ext_id),
            t(&p.summoner_name),
            Val::I32(Some(p.participant_id)),
            t(&p.role),
            t(&p.champion),
            Val::I32(p.level),
            Val::I32(p.kills),
            Val::I32(p.deaths),
            Val::I32(p.assists),
            Val::I64(p.gold),
            Val::I32(p.cs),
            Val::I32(p.wards_placed),
            Val::I32(p.wards_destroyed),
            Val::I32(p.damage_share_bp),
            Val::Text(p.items.clone()),
            Val::I32(p.rune_style),
            Val::I32(p.rune_sub_style),
            Val::Text(p.runes.clone()),
            Val::Text(p.skill_order.clone()),
        ]);
    }
    let params: Vec<&dyn QueryParameter> = vals.iter().map(Val::param).collect();
    let done = "".execute(&details_sql(row.players.len()), &params).await;
    match done {
        Ok(1) => Ok(()),
        Ok(_) => Err(eyre!("game {}: not stored", row.game_ext_id)),
        Err(e) => {
            let e = eyre!(e);
            // the constraint name is only in the DB error under Canyon's one
            if e.chain().any(|c| c.to_string().contains("v2_game_details")) {
                Err(eyre!(
                    "game {}: nothing written, a team or a player isn't stored",
                    row.game_ext_id
                ))
            } else {
                Err(e).wrap_err_with(|| format!("v2 game details {}", row.game_ext_id))
            }
        }
    }
}

/// `v (game_ext, team_ext, side)` from the params and `r (game_id, team_id, side)`: only teams of
/// the game's match, and `filter` on `g` (the game)
fn sides_cte(rows: usize, filter: &str) -> String {
    let mut n = 0;
    let v = values(
        rows,
        &[
            ("game_ext", "bigint"),
            ("team_ext", "bigint"),
            ("side", "text"),
        ],
        &mut n,
    );
    format!(
        "WITH v (game_ext, team_ext, side) AS (VALUES {v}), \
         r AS (SELECT g.id AS game_id, t.id AS team_id, v.side FROM v \
           JOIN v2_game g ON g.ext_id = v.game_ext JOIN v2_team t ON t.ext_id = v.team_ext \
           JOIN v2_event_team et ON et.event_id = g.event_id AND et.team_id = t.id \
           WHERE {filter})"
    )
}

/// (game, team, side) rows sorted by game and team, each pair once: every writer of
/// `v2_game_team` locks the rows in the same order, and one statement never hits a row twice
fn side_rows(sides: &[GameSides]) -> Vec<(i64, i64, String)> {
    let mut flat: Vec<(i64, i64, String)> = sides
        .iter()
        .flat_map(|g| {
            g.sides
                .iter()
                .map(|(team, side)| (g.game_ext_id, *team, side.to_string()))
        })
        .collect();
    flat.sort_by_key(|(g, t, _)| (*g, *t));
    flat.dedup_by_key(|(g, t, _)| (*g, *t));
    flat
}

/// 2 rows a game and an even chunk size: a game is never split
const SIDE_CHUNK: usize = MAX_PARAMS / 3 / 2 * 2;

fn vod_sql(rows: usize) -> String {
    format!(
        "{}, ins AS ({API_SIDES}) \
         UPDATE v2_game SET details = 'no_feed' \
         WHERE id IN (SELECT game_id FROM r GROUP BY game_id HAVING count(*) = 2) RETURNING ext_id",
        sides_cte(rows, "g.details IS NULL AND g.state = 'completed'")
    )
}

/// Sides from the API from `r (game_id, team_id, side)`: `side_source = 'api'`, never over a
/// `feed` one
const API_SIDES: &str = "INSERT INTO v2_game_team (game_id, team_id, side, side_source) \
    SELECT game_id, team_id, side, 'api' FROM r \
    ON CONFLICT (game_id, team_id) DO UPDATE SET side = EXCLUDED.side, side_source = 'api' \
    WHERE v2_game_team.side_source <> 'feed' AND v2_game_team.side <> EXCLUDED.side";

/// Games that end up `unneeded` (a 2-0 in a Bo3) get no sides: they were never played
fn live_sides_sql(rows: usize) -> String {
    format!("{} {API_SIDES}", sides_cte(rows, "g.state <> 'unneeded'"))
}

/// A game being played (`LiveGameRow`), one statement: start/end times when still empty, and the
/// two teams with `side_source = 'feed'` and their totals right now. Games with details
/// (`done`/`no_feed`) are left alone: the details loop writes the final numbers. The game row is
/// locked first (same order as `write_details`), so details written meanwhile are seen.
/// Never touches `result`. Returns the team rows written (0 = game not stored or with details,
/// 1 = a team not stored)
pub async fn write_live_game(row: &LiveGameRow) -> Result<u64> {
    if row.teams.len() != 2 {
        return Err(eyre!("game {}: {} teams", row.game_ext_id, row.teams.len()));
    }
    let sql = "WITH g AS (SELECT id FROM v2_game WHERE ext_id = $1::bigint AND details IS NULL FOR UPDATE), \
         times AS (UPDATE v2_game SET start_time = COALESCE(v2_game.start_time, $2::timestamp), \
           end_time = COALESCE(v2_game.end_time, $3::timestamp) FROM g WHERE v2_game.id = g.id RETURNING 1) \
         INSERT INTO v2_game_team (game_id, team_id, side_source, side, gold, kills, towers, inhibitors, barons, dragons) \
         SELECT g.id, t.id, 'feed', v.side, v.gold, v.kills, v.towers, v.inhibitors, v.barons, v.dragons \
         FROM g, (VALUES ($4::bigint, $5::text, $6::bigint, $7::int, $8::int, $9::int, $10::int, $11::text), \
                         ($12::bigint, $13::text, $14::bigint, $15::int, $16::int, $17::int, $18::int, $19::text)) \
           AS v (team_ext, side, gold, kills, towers, inhibitors, barons, dragons) \
         JOIN v2_team t ON t.ext_id = v.team_ext \
         ON CONFLICT (game_id, team_id) DO UPDATE SET side = EXCLUDED.side, side_source = 'feed', \
           gold = EXCLUDED.gold, kills = EXCLUDED.kills, towers = EXCLUDED.towers, \
           inhibitors = EXCLUDED.inhibitors, barons = EXCLUDED.barons, dragons = EXCLUDED.dragons";
    let sides: Vec<String> = row.teams.iter().map(|t| t.side.to_string()).collect();
    let mut params: Vec<&dyn QueryParameter> =
        vec![&row.game_ext_id, &row.start_time, &row.end_time];
    for (t, side) in row.teams.iter().zip(&sides) {
        params.push(&t.team_ext_id);
        params.push(side);
        params.push(&t.gold);
        params.push(&t.kills);
        params.push(&t.towers);
        params.push(&t.inhibitors);
        params.push(&t.barons);
        params.push(&t.dragons);
    }
    "".execute(sql, &params)
        .await
        .map_err(|e| eyre!(e))
        .wrap_err_with(|| format!("v2 live game {}", row.game_ext_id))
}

/// Sides from `watchLiveQuery`, for games not played yet too (they can change until the game
/// starts), never for `unneeded` ones. Games or teams we don't have, or teams not in the game's
/// match, are left out. Returns the rows written
pub async fn write_live_sides(sides: &[GameSides]) -> Result<u64> {
    let flat = side_rows(sides);
    let mut written = 0;
    for chunk in flat.chunks(SIDE_CHUNK) {
        let mut params: Vec<&dyn QueryParameter> = Vec::with_capacity(chunk.len() * 3);
        for (g, team, side) in chunk {
            params.push(g);
            params.push(team);
            params.push(side);
        }
        written += ""
            .execute(&live_sides_sql(chunk.len()), &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err("v2 game sides from watchLiveQuery")?;
    }
    Ok(written)
}

/// Sides from `watchVodQuery` for finished games still without details: `side_source = 'api'`
/// (never over a `feed` one), the game becomes `no_feed`. Returns the games the DB marked
pub async fn write_vod_sides(sides: &[GameSides]) -> Result<Vec<i64>> {
    let flat = side_rows(sides);
    let mut marked = Vec::new();
    for chunk in flat.chunks(SIDE_CHUNK) {
        let mut params: Vec<&dyn QueryParameter> = Vec::with_capacity(chunk.len() * 3);
        for (g, team, side) in chunk {
            params.push(g);
            params.push(team);
            params.push(side);
        }
        let r = rows(
            &vod_sql(chunk.len()),
            &params,
            "v2 game sides from watchVodQuery",
        )
        .await?;
        for row in r.get_postgres_rows().map_err(|e| eyre!(e))? {
            marked.push(row.try_get(0).wrap_err("v2_game.ext_id")?);
        }
    }
    Ok(marked)
}

/// One more failed try for this game, and its wait before the next one. Returns the count
pub async fn bump_tries(game_ext_id: i64) -> Result<i32> {
    let r = rows(
        "UPDATE v2_game SET details_tries = COALESCE(details_tries, 0) + 1, \
           details_next_try = (now() AT TIME ZONE 'UTC') + interval '10 minutes' * power(4, COALESCE(details_tries, 0)) \
         WHERE ext_id = $1::bigint RETURNING details_tries",
        &[&game_ext_id],
        "v2_game details_tries",
    )
    .await?;
    let rows = r.get_postgres_rows().map_err(|e| eyre!(e))?;
    let first = rows
        .first()
        .ok_or_else(|| eyre!("game {game_ext_id} not stored"))?;
    first.try_get(0).wrap_err("v2_game.details_tries")
}

/// Who won each game of a sweep (2-0, 3-0, a Bo1): the team with wins took every completed
/// game. Only rows without a result
pub async fn sweep_results() -> Result<u64> {
    ""
        .execute(
            "UPDATE v2_game_team gt SET result = CASE WHEN gt.team_id = w.team_id THEN 'win' ELSE 'loss' END \
             FROM v2_game g JOIN v2_event e ON e.id = g.event_id \
             JOIN v2_event_team w ON w.event_id = e.id AND w.game_wins > 0 \
             JOIN v2_event_team l ON l.event_id = e.id AND l.slot <> w.slot AND l.game_wins = 0 \
             WHERE gt.game_id = g.id AND g.state = 'completed' AND e.state = 'completed' AND gt.result IS NULL",
            &[],
        )
        .await
        .map_err(|e| eyre!(e))
        .wrap_err("v2_game_team sweep results")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::v2::{events::write_events, rows::*};
    use canyon_sql::date_time::NaiveDateTime;

    #[test]
    fn details_sql_numbering() {
        let sql = details_sql(2);
        // 4 game params, 2 × 8 team params, then 2 × 20 player params
        assert!(sql.contains("(VALUES ($5::bigint, $6::text"));
        assert!(sql.contains("($13::bigint, $14::text"));
        assert!(sql.contains("(VALUES ($21::bigint, $22::bigint, $23::text, $24::int"));
        assert!(sql.contains("$60::text)"));
        assert!(!sql.contains("$61"));
        assert!(sql.contains("(SELECT count(*) FROM gp) = 2 THEN 'done' ELSE 'incomplete' END"));
        assert!(
            sql.contains("INSERT INTO v2_game_team (game_id, team_id, side_source, side, gold")
        );
        let v = vod_sql(2);
        assert!(v.contains("($4::bigint, $5::bigint, $6::text)"));
        assert!(v.contains("side_source <> 'feed'"));
        assert!(v.ends_with("RETURNING ext_id"));
    }

    // DB tests: ext ids -399..-300
    async fn count(sql: &str) -> i64 {
        "".query_one_for::<i64>(sql, &[]).await.unwrap()
    }

    fn event(ext: i64, wins: [i32; 2], games: &[(i64, &str)]) -> EventRow {
        EventRow {
            ext_id: ext,
            kind: "match",
            league_ext_id: -301,
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
                    team_ext_id: Some(-302),
                    game_wins: Some(wins[0]),
                    outcome: None,
                },
                EventTeamRow {
                    slot: 2,
                    team_ext_id: Some(-303),
                    game_wins: Some(wins[1]),
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

    async fn setup() {
        crate::dao::v2::schema::apply().await.unwrap();
        crate::dao::v2::test_cleanup().await;
        let rows = EventRows {
            events: vec![
                event(
                    -310,
                    [2, 0],
                    &[(-320, "completed"), (-321, "completed"), (-322, "unneeded")],
                ),
                event(-311, [1, 1], &[(-323, "completed"), (-324, "completed")]),
            ],
            league_stubs: vec![LeagueStub {
                ext_id: -301,
                slug: "l".into(),
                name: "L".into(),
                image_url: "i".into(),
            }],
            team_stubs: vec![
                TeamStub {
                    ext_id: -302,
                    name: "Blue Team".into(),
                    code: "BT".into(),
                    image_url: "i".into(),
                },
                TeamStub {
                    ext_id: -303,
                    name: "Red Team".into(),
                    code: "RT".into(),
                    image_url: "i".into(),
                },
            ],
            skipped: 0,
        };
        write_events(&rows, "en-US").await.unwrap();
    }

    fn details(game: i64, players: i64) -> GameDetailsRow {
        let team = |ext, side| GameTeamRow {
            team_ext_id: ext,
            side,
            gold: Some(60_000),
            kills: Some(10),
            towers: Some(9),
            inhibitors: Some(1),
            barons: Some(1),
            dragons: Some("ocean".into()),
        };
        GameDetailsRow {
            game_ext_id: game,
            patch: Some("16.20".into()),
            start_time: None,
            end_time: None,
            teams: vec![team(-302, "blue"), team(-303, "red")],
            players: (0..players)
                .map(|i| GamePlayerRow {
                    team_ext_id: if i < 5 { -302 } else { -303 },
                    player_ext_id: Some(-330 - i),
                    summoner_name: format!("BT p{i}"),
                    participant_id: i as i32 + 1,
                    role: "top".into(),
                    champion: "Volibear".into(),
                    level: Some(18),
                    kills: Some(1),
                    deaths: Some(2),
                    assists: Some(3),
                    gold: Some(12_000),
                    cs: Some(300),
                    wards_placed: Some(10),
                    wards_destroyed: Some(5),
                    damage_share_bp: Some(2144),
                    items: Some("1,2".into()),
                    rune_style: Some(8400),
                    rune_sub_style: Some(8300),
                    runes: Some("1,2".into()),
                    skill_order: Some("Q,W".into()),
                })
                .collect(),
            player_stubs: (0..players)
                .map(|i| PlayerStub {
                    ext_id: -330 - i,
                    handle: format!("p{i}"),
                    role: "top".into(),
                })
                .collect(),
            remapped: None,
        }
    }

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn details_all_or_nothing() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        setup().await;
        // the queue has the 4 completed test games (and real ones): ours are there with teams
        let q = next_games(100_000, true).await.unwrap();
        let g = q.iter().find(|g| g.game_ext_id == -320).unwrap();
        assert_eq!(
            g.teams,
            [(-302, "BT".to_string()), (-303, "RT".to_string())]
        );
        assert!(!q.iter().any(|g| g.game_ext_id == -322));
        // backfill off: an old match leaves the queue, a recent one stays
        "".execute("UPDATE v2_event SET start_time = (now() AT TIME ZONE 'UTC') - interval '10 days' WHERE ext_id = -310", &[]).await.unwrap();
        "".execute("UPDATE v2_event SET start_time = (now() AT TIME ZONE 'UTC') - interval '1 day' WHERE ext_id = -311", &[]).await.unwrap();
        let recent = next_games(100_000, false).await.unwrap();
        assert!(!recent.iter().any(|g| g.game_ext_id == -320));
        assert!(recent.iter().any(|g| g.game_ext_id == -323));
        assert!(
            next_games(100_000, true)
                .await
                .unwrap()
                .iter()
                .any(|g| g.game_ext_id == -320)
        );
        // a player we don't have: nothing is marked done
        let row = details(-320, 10);
        let err = write_details(&row).await.unwrap_err();
        assert!(format!("{err:#}").contains("nothing written"), "{err:#}");
        // nothing of it stays: the whole statement rolled back
        assert_eq!(count("SELECT count(*) FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -320").await, 0);
        assert_eq!(
            count("SELECT count(*) FROM v2_game WHERE ext_id = -320 AND details IS NULL").await,
            1
        );
        // with the stubs it goes in, twice without doubling
        write_player_stubs(&row.player_stubs).await.unwrap();
        write_details(&row).await.unwrap();
        write_details(&row).await.unwrap();
        assert_eq!(count("SELECT count(*) FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -320 AND gt.side_source = 'feed'").await, 2);
        assert_eq!(count("SELECT count(*) FROM v2_game_player gp JOIN v2_game g ON g.id = gp.game_id WHERE g.ext_id = -320").await, 10);
        assert_eq!(count("SELECT count(*) FROM v2_game WHERE ext_id = -320 AND details = 'done' AND patch = '16.20'").await, 1);
        // a player the feed has no id for: the slot is stored without a player
        let mut no_id = details(-321, 10);
        no_id.players[9].player_ext_id = None;
        write_details(&no_id).await.unwrap();
        assert_eq!(count("SELECT count(*) FROM v2_game_player gp JOIN v2_game g ON g.id = gp.game_id WHERE g.ext_id = -321 AND gp.player_id IS NULL AND gp.summoner_name = 'BT p9'").await, 1);
        "".execute("UPDATE v2_game SET details = NULL WHERE ext_id = -321", &[])
            .await
            .unwrap();
        "".execute("DELETE FROM v2_game_player WHERE game_id = (SELECT id FROM v2_game WHERE ext_id = -321)", &[]).await.unwrap();
        "".execute(
            "DELETE FROM v2_game_team WHERE game_id = (SELECT id FROM v2_game WHERE ext_id = -321)",
            &[],
        )
        .await
        .unwrap();
        // a stub never renames a player we have
        let mut renamed = row.player_stubs.clone();
        renamed[0].handle = "other".into();
        write_player_stubs(&renamed).await.unwrap();
        assert_eq!(
            count("SELECT count(*) FROM v2_player WHERE ext_id = -330 AND handle = 'p0'").await,
            1
        );
        // watchVodQuery sides: never over feed sides, only games still without details are marked
        let vod = vec![
            GameSides {
                game_ext_id: -320,
                sides: vec![(-302, "red"), (-303, "blue")],
            },
            GameSides {
                game_ext_id: -321,
                sides: vec![(-302, "red"), (-303, "blue")],
            },
        ];
        assert_eq!(write_vod_sides(&vod).await.unwrap(), vec![-321]);
        assert_eq!(count("SELECT count(*) FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -320 AND gt.side = 'blue' AND gt.team_id = (SELECT id FROM v2_team WHERE ext_id = -302)").await, 1);
        assert_eq!(
            count("SELECT count(*) FROM v2_game WHERE ext_id = -321 AND details = 'no_feed'").await,
            1
        );
        // a failed try waits before the next one; the 5th takes the game out of the queue
        let in_queue = |q: Vec<QueueGame>| q.iter().any(|g| g.game_ext_id == -323);
        assert!(in_queue(next_games(100_000, true).await.unwrap()));
        for i in 1..=MAX_TRIES {
            assert_eq!(bump_tries(-323).await.unwrap(), i);
            assert!(
                !in_queue(next_games(100_000, true).await.unwrap()),
                "waiting after try {i}"
            );
            "".execute(
                "UPDATE v2_game SET details_next_try = NULL WHERE ext_id = -323",
                &[],
            )
            .await
            .unwrap();
        }
        assert!(!in_queue(next_games(100_000, true).await.unwrap()));
        // sweep: -310 (2-0) gets results, -311 (1-1) doesn't
        write_vod_sides(&[GameSides {
            game_ext_id: -324,
            sides: vec![(-302, "blue"), (-303, "red")],
        }])
        .await
        .unwrap();
        sweep_results().await.unwrap();
        assert_eq!(count("SELECT count(*) FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id IN (-320, -321) AND gt.result = 'win' AND gt.team_id = (SELECT id FROM v2_team WHERE ext_id = -302)").await, 2);
        assert_eq!(count("SELECT count(*) FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id IN (-320, -321) AND gt.result = 'loss'").await, 2);
        assert_eq!(count("SELECT count(*) FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -324").await, 2);
        assert_eq!(count("SELECT count(*) FROM v2_game_team gt JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -324 AND gt.result IS NOT NULL").await, 0);
        crate::dao::v2::test_cleanup().await;
    }

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn live_game_until_its_details() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        setup().await;
        let at = |m: u32| {
            NaiveDateTime::parse_from_str(&format!("2026-10-31 18:{m:02}:00"), "%Y-%m-%d %H:%M:%S")
                .unwrap()
        };
        let row = |gold: i64, start: u32, end: Option<u32>| LiveGameRow {
            game_ext_id: -323,
            start_time: at(start),
            end_time: end.map(at),
            teams: vec![
                GameTeamRow {
                    team_ext_id: -302,
                    side: "blue",
                    gold: Some(gold),
                    kills: Some(1),
                    towers: Some(0),
                    inhibitors: Some(0),
                    barons: Some(0),
                    dragons: None,
                },
                GameTeamRow {
                    team_ext_id: -303,
                    side: "red",
                    gold: Some(gold + 1),
                    kills: Some(2),
                    towers: Some(1),
                    inhibitors: Some(0),
                    barons: Some(0),
                    dragons: Some("ocean".into()),
                },
            ],
        };
        let state = "SELECT string_agg(t.ext_id || ':' || gt.side || ':' || gt.side_source || ':' || gt.gold || ':' || coalesce(gt.result, '-'), ',' ORDER BY t.ext_id) \
            FROM v2_game_team gt JOIN v2_team t ON t.id = gt.team_id JOIN v2_game g ON g.id = gt.game_id WHERE g.ext_id = -323";
        let times = "SELECT to_char(start_time, 'HH24:MI') || '-' || coalesce(to_char(end_time, 'HH24:MI'), '?') FROM v2_game WHERE ext_id = -323";
        assert_eq!(write_live_game(&row(1000, 5, None)).await.unwrap(), 2);
        let s: String = "".query_one_for(state, &[]).await.unwrap();
        assert_eq!(s, "-303:red:feed:1001:-,-302:blue:feed:1000:-");
        let t: String = "".query_one_for(times, &[]).await.unwrap();
        assert_eq!(t, "18:05-?");
        // a winner from the score is kept; totals move on, the start doesn't, the end is set
        "".execute("UPDATE v2_game_team SET result = CASE WHEN team_id = (SELECT id FROM v2_team WHERE ext_id = -302) THEN 'win' ELSE 'loss' END WHERE game_id = (SELECT id FROM v2_game WHERE ext_id = -323)", &[]).await.unwrap();
        assert_eq!(write_live_game(&row(5000, 9, Some(40))).await.unwrap(), 2);
        let s: String = "".query_one_for(state, &[]).await.unwrap();
        assert_eq!(s, "-303:red:feed:5001:loss,-302:blue:feed:5000:win");
        let t: String = "".query_one_for(times, &[]).await.unwrap();
        assert_eq!(t, "18:05-18:40");
        // once the details loop wrote the game, live leaves it alone
        "".execute(
            "UPDATE v2_game SET details = 'done' WHERE ext_id = -323",
            &[],
        )
        .await
        .unwrap();
        assert_eq!(write_live_game(&row(9999, 1, Some(50))).await.unwrap(), 0);
        let s: String = "".query_one_for(state, &[]).await.unwrap();
        assert_eq!(s, "-303:red:feed:5001:loss,-302:blue:feed:5000:win");
        crate::dao::v2::test_cleanup().await;
    }
}
