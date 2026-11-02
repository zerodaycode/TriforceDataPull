//! `team_player` from the fetched rosters: new pairs inserted, pairs no longer sent deleted, one
//! statement per chunk of teams so the table is never left empty halfway

use std::collections::{BTreeMap, BTreeSet};

use canyon_sql::{connection::DbConnection, query::QueryParameter};
use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};

use crate::client::{legacy::models as api, log};

/// Postgres takes at most 65535 parameters per statement
const MAX_PARAMS: usize = 60_000;

/// Rows touched by a sync
#[derive(Debug, Default, PartialEq)]
pub struct Synced {
    pub inserted: i64,
    pub deleted: i64,
    /// Pairs whose team or player isn't stored, skipped
    pub unknown: i64,
}

/// Players by team ext_id, each pair once. A team sent with no players is kept (empty roster)
pub fn rosters(teams: &[api::Team]) -> BTreeMap<i64, BTreeSet<i64>> {
    let mut out: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
    for t in teams {
        out.entry(t.id.0)
            .or_default()
            .extend(t.players.iter().map(|p| p.id.0));
    }
    out
}

/// For the teams in scope: insert the pairs we don't have and delete the ones not sent. In the
/// whole table: delete the copies of a pair (the oldest one stays). Returns inserted, deleted,
/// unknown pairs and up to 20 of them (`team/player` ext_ids)
fn sync_sql(pairs: usize, teams: usize) -> String {
    let mut n = 0;
    let mut next = || {
        n += 1;
        format!("${n}::bigint")
    };
    let pairs: Vec<String> = (0..pairs)
        .map(|_| format!("({}, {})", next(), next()))
        .collect();
    let scope: Vec<String> = (0..teams).map(|_| next()).collect();
    let pairs_sql = if pairs.is_empty() {
        "SELECT NULL::bigint, NULL::bigint WHERE false".to_string()
    } else {
        format!("VALUES {}", pairs.join(", "))
    };
    format!(
        "WITH sent (team_ext, player_ext) AS ({pairs_sql}), \
         resolved AS (SELECT DISTINCT t.id::bigint AS team_id, p.id::bigint AS player_id FROM sent s \
           JOIN team t ON t.ext_id = s.team_ext JOIN player p ON p.ext_id = s.player_ext), \
         known AS (SELECT count(*) AS n FROM sent s \
           WHERE EXISTS (SELECT 1 FROM team t WHERE t.ext_id = s.team_ext) \
             AND EXISTS (SELECT 1 FROM player p WHERE p.ext_id = s.player_ext)), \
         keep AS (SELECT min(tp.id) AS id FROM team_player tp \
           JOIN resolved r ON r.team_id = tp.team_id AND r.player_id = tp.player_id \
           GROUP BY tp.team_id, tp.player_id), \
         del AS (DELETE FROM team_player tp \
           WHERE (tp.team_id IN (SELECT id::bigint FROM team WHERE ext_id IN ({})) \
                  AND tp.id NOT IN (SELECT id FROM keep)) \
              OR tp.id NOT IN (SELECT min(id) FROM team_player GROUP BY team_id, player_id) \
           RETURNING 1), \
         ins AS (INSERT INTO team_player (team_id, player_id) SELECT r.team_id, r.player_id FROM resolved r \
           WHERE NOT EXISTS (SELECT 1 FROM team_player tp \
             WHERE tp.team_id = r.team_id AND tp.player_id = r.player_id) RETURNING 1) \
         SELECT (SELECT count(*) FROM ins), (SELECT count(*) FROM del), \
           (SELECT count(*) FROM sent) - (SELECT n FROM known), \
           (SELECT string_agg(team_ext || '/' || player_ext, ', ') FROM (SELECT * FROM sent s \
             WHERE NOT EXISTS (SELECT 1 FROM team t WHERE t.ext_id = s.team_ext) \
                OR NOT EXISTS (SELECT 1 FROM player p WHERE p.ext_id = s.player_ext) LIMIT 20) u)",
        scope.join(", ")
    )
}

/// Teams not in `rosters` keep their pairs. No pairs at all is taken as a broken answer and
/// nothing is touched. No transaction across chunks (Canyon has none), each chunk is atomic.
/// There's no unique key on the pairs: only the `DatabaseOps` lock stops two syncs at once in
/// one process, a copy made by two processes goes on the next sync
pub async fn sync(rosters: &BTreeMap<i64, BTreeSet<i64>>) -> Result<Synced> {
    let mut total = Synced::default();
    if rosters.values().all(BTreeSet::is_empty) {
        log("team_player: the answer has no players, nothing changed");
        return Ok(total);
    }
    let mut chunks: Vec<Vec<(&i64, &BTreeSet<i64>)>> = Vec::new();
    let mut used = 0;
    for (team, players) in rosters {
        let cost = 2 * players.len() + 1;
        if chunks.is_empty() || used + cost > MAX_PARAMS {
            chunks.push(Vec::new());
            used = 0;
        }
        used += cost;
        chunks
            .last_mut()
            .expect("chunk just pushed")
            .push((team, players));
    }
    for chunk in chunks {
        let pairs: Vec<(&i64, &i64)> = chunk
            .iter()
            .flat_map(|(t, ps)| ps.iter().map(move |p| (*t, p)))
            .collect();
        let mut params: Vec<&dyn QueryParameter> =
            Vec::with_capacity(pairs.len() * 2 + chunk.len());
        for (team, player) in &pairs {
            params.push(*team);
            params.push(*player);
        }
        for (team, _) in &chunk {
            params.push(*team);
        }
        let rows = ""
            .query_rows(&sync_sql(pairs.len(), chunk.len()), &params)
            .await
            .map_err(|e| eyre!(e))
            .wrap_err("team_player sync")?;
        let rows = rows
            .get_postgres_rows()
            .map_err(|e| eyre!(e))
            .wrap_err("team_player sync: reading the counts")?;
        let row = rows
            .first()
            .ok_or_else(|| eyre!("team_player sync: no counts"))?;
        total.inserted += row.try_get::<_, i64>(0).wrap_err("inserted")?;
        total.deleted += row.try_get::<_, i64>(1).wrap_err("deleted")?;
        total.unknown += row.try_get::<_, i64>(2).wrap_err("unknown")?;
        if let Some(s) = row.try_get::<_, Option<String>>(3).wrap_err("skipped")? {
            log(format!(
                "team_player: skipped, team or player not stored (team/player ext_id, max 20): {s}"
            ));
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    // LolesportsId only reads from a borrowed str: from_str, not from_value
    fn team(id: i64, players: &[i64]) -> api::Team {
        let players: Vec<String> = players
            .iter()
            .map(|p| {
                format!(
                    r#"{{"id":"{p}","summonerName":"x","firstName":"f","lastName":"l","image":null,"role":"mid"}}"#
                )
            })
            .collect();
        serde_json::from_str(&format!(
            r#"{{"id":"{id}","slug":"s","name":"n","code":"C","image":"i","alternativeImage":null,"backgroundImage":null,"status":"active","homeLeague":null,"players":[{}]}}"#,
            players.join(",")
        ))
        .unwrap()
    }

    #[test]
    fn rosters_dedupe_and_keep_empty_teams() {
        // the old source sent each team once per run, all in the same list
        let r = rosters(&[team(1, &[10, 11]), team(1, &[11, 10]), team(2, &[])]);
        assert_eq!(r[&1], BTreeSet::from([10, 11]));
        assert!(r[&2].is_empty());
    }

    #[test]
    fn sync_sql_numbering() {
        let none = sync_sql(0, 2);
        assert!(none.contains("WHERE false"));
        assert!(none.contains("ext_id IN ($1::bigint, $2::bigint)"));
        let some = sync_sql(2, 1);
        assert!(some.contains("VALUES ($1::bigint, $2::bigint), ($3::bigint, $4::bigint)"));
        assert!(some.contains("ext_id IN ($5::bigint)"));
    }

    async fn count(sql: &str) -> i64 {
        "".query_one_for::<i64>(sql, &[]).await.unwrap()
    }

    async fn exec(sql: &str) {
        "".execute(sql, &[]).await.unwrap();
    }

    const PAIRS: &str = "SELECT count(*) FROM team_player tp JOIN team t ON t.id = tp.team_id WHERE t.ext_id = -901";
    const PAIRS_902: &str = "SELECT count(*) FROM team_player tp JOIN team t ON t.id = tp.team_id WHERE t.ext_id = -902";

    async fn cleanup() {
        exec("DELETE FROM team_player WHERE team_id IN (SELECT id FROM team WHERE ext_id < 0) OR player_id IN (SELECT id FROM player WHERE ext_id < 0)").await;
        exec("DELETE FROM team WHERE ext_id < 0").await;
        exec("DELETE FROM player WHERE ext_id < 0").await;
    }

    #[canyon_sql::macros::canyon_tokio_test]
    #[ignore = "writes to the DB in canyon.toml: cargo test -- --ignored, against the dev DB after the app ran once"]
    fn sync_adds_removes_and_dedupes() {
        let _guard = crate::dao::v2::DB_TEST_LOCK.lock().await;
        cleanup().await;
        exec("INSERT INTO team (ext_id, name, slug, code, image_url) VALUES (-901, 'T', 't', 'T', 'i'), (-902, 'U', 'u', 'U', 'i')").await;
        exec("INSERT INTO player (ext_id, first_name, last_name, summoner_name, role) VALUES (-911, 'a', 'a', 'a', 'top'), (-912, 'b', 'b', 'b', 'mid'), (-913, 'c', 'c', 'c', 'bot')").await;
        // -901: -911 and -912, -902: -913 (a team not in the answer)
        exec("INSERT INTO team_player (team_id, player_id) SELECT t.id, p.id FROM team t, player p WHERE (t.ext_id = -901 AND p.ext_id IN (-911, -912)) OR (t.ext_id = -902 AND p.ext_id = -913)").await;
        // a second copy of -911 and of -902/-913, like after the old runs
        exec("INSERT INTO team_player (team_id, player_id) SELECT t.id, p.id FROM team t, player p WHERE (t.ext_id = -901 AND p.ext_id = -911) OR (t.ext_id = -902 AND p.ext_id = -913)").await;
        assert_eq!(count(PAIRS).await, 3);
        assert_eq!(count(PAIRS_902).await, 2);

        // -912 left, -913 joined, -999 isn't stored
        let got = sync(&rosters(&[team(-901, &[-911, -913, -999])]))
            .await
            .unwrap();
        // `deleted` also counts copies of real rows if the DB has any
        assert_eq!((got.inserted, got.unknown), (1, 1));
        assert!(got.deleted >= 3, "{got:?}");
        assert_eq!(count(PAIRS).await, 2);
        assert_eq!(count("SELECT count(DISTINCT player_id) FROM team_player tp JOIN team t ON t.id = tp.team_id WHERE t.ext_id = -901").await, 2);
        // -902 wasn't in the answer: its link stays, its copy goes
        assert_eq!(count(PAIRS_902).await, 1);

        // the same answer again changes nothing
        let got = sync(&rosters(&[team(-901, &[-911, -913])])).await.unwrap();
        assert_eq!(got, Synced::default());

        // an answer without players touches nothing
        let got = sync(&rosters(&[team(-901, &[]), team(-902, &[])]))
            .await
            .unwrap();
        assert_eq!(got, Synced::default());
        assert_eq!(count(PAIRS).await, 2);

        // -901 sent with no players while others have some: its pairs go. -998 isn't stored
        let got = sync(&rosters(&[
            team(-901, &[]),
            team(-902, &[-913]),
            team(-998, &[-911]),
        ]))
        .await
        .unwrap();
        assert_eq!(
            got,
            Synced {
                inserted: 0,
                deleted: 2,
                unknown: 1
            }
        );
        assert_eq!(count(PAIRS).await, 0);
        assert_eq!(count(PAIRS_902).await, 1);
        cleanup().await;
    }
}
