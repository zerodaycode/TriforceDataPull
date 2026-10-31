//! `scheduleEvents` page → v2 event rows

use std::collections::BTreeMap;

use crate::{
    client::{
        adapters::{
            ext_id,
            v2::{game_sides, remap_sides},
        },
        gql::ops::{
            schedule_events::{self, Event, EventMatch, EventShow},
            watch_live::{self, LiveEvent},
        },
        log,
    },
    dao::v2::rows::*,
};

fn id(s: &str) -> Option<i64> {
    ext_id(s).ok()
}

/// The TBD slot of a match
const NO_TEAM: &str = "0";

/// `Some(())` when `v` is one of `allowed` (the CHECK lists in `rows.rs`)
fn known(v: &str, allowed: &[&str]) -> Option<()> {
    allowed.contains(&v).then_some(())
}

fn known_opt(v: Option<&str>, allowed: &[&str]) -> Option<()> {
    v.map_or(Some(()), |v| known(v, allowed))
}

/// `None` when an id doesn't parse or a value isn't one we store: that event is skipped, not
/// the page. Its teams are only added when the event is kept
fn from_match(m: EventMatch) -> Option<(EventRow, Vec<TeamStub>)> {
    known(&m.state, EVENT_STATES)?;
    known_opt(m.r#match.r#type.as_deref(), MATCH_TYPES)?;
    if m.match_teams.len() > 2 {
        return None;
    }
    let mut slots = Vec::with_capacity(m.match_teams.len());
    let mut stubs = Vec::new();
    for (slot, t) in (1..).zip(m.match_teams) {
        let result = t.result;
        let outcome = result.as_ref().and_then(|r| r.outcome.clone());
        known_opt(outcome.as_deref(), OUTCOMES)?;
        let team_ext_id = if t.team_id == NO_TEAM {
            None
        } else {
            let ext = id(&t.team_id)?;
            stubs.push(TeamStub {
                ext_id: ext,
                name: t.name,
                code: t.code,
                image_url: t.image.unwrap_or_default(),
            });
            Some(ext)
        };
        slots.push(EventTeamRow {
            slot,
            team_ext_id,
            game_wins: result.and_then(|r| r.game_wins),
            outcome,
        });
    }
    let games = m
        .r#match
        .games
        .into_iter()
        .map(|g| {
            known(&g.state, GAME_STATES)?;
            (g.number >= 1).then_some(())?;
            Some(GameRow {
                ext_id: id(&g.id)?,
                number: g.number,
                state: g.state,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let (strategy_type, strategy_count) = m.r#match.strategy.map(|s| (s.r#type, s.count)).unzip();
    known_opt(strategy_type.as_deref(), STRATEGIES)?;
    let row = EventRow {
        ext_id: id(&m.id)?,
        kind: "match",
        league_ext_id: id(&m.league.id)?,
        tournament_ext_id: match m.tournament {
            Some(t) => Some(id(&t.id)?),
            None => None,
        },
        start_time: m.start_time.map(|t| t.naive_utc()),
        state: m.state,
        strategy_type,
        strategy_count,
        match_type: m.r#match.r#type,
        block_name: m.block_name.filter(|b| !b.trim().is_empty()),
        teams: slots,
        games,
        streams: streams(m.streams),
    };
    Some((row, stubs))
}

fn from_show(s: EventShow) -> Option<EventRow> {
    known(&s.state, EVENT_STATES)?;
    Some(EventRow {
        ext_id: id(&s.id)?,
        kind: "show",
        league_ext_id: id(&s.league.id)?,
        tournament_ext_id: match s.tournament {
            Some(t) => Some(id(&t.id)?),
            None => None,
        },
        start_time: s.start_time.map(|t| t.naive_utc()),
        state: s.state,
        strategy_type: None,
        strategy_count: None,
        match_type: None,
        block_name: None,
        teams: vec![],
        games: vec![],
        streams: streams(s.streams),
    })
}

/// Streams without a language are left out: `locale` and `english_name` are required
fn streams(list: Vec<schedule_events::Stream>) -> Vec<StreamRow> {
    list.into_iter()
        .filter_map(|s| {
            let m = s.media_locale?;
            Some(StreamRow {
                provider: s.provider,
                parameter: s.parameter,
                locale: m.locale,
                english_name: m.english_name,
            })
        })
        .collect()
}

pub fn from_schedule(data: schedule_events::Data) -> EventRows {
    let mut out = EventRows::default();
    let mut leagues: BTreeMap<i64, LeagueStub> = BTreeMap::new();
    let mut teams: BTreeMap<i64, TeamStub> = BTreeMap::new();
    for event in data.esports.events {
        let (row, league) = match event {
            Event::Match(m) => {
                let league = m.league.clone_stub();
                match from_match(m) {
                    Some((row, stubs)) => {
                        for t in stubs {
                            teams.entry(t.ext_id).or_insert(t);
                        }
                        (Some(row), league)
                    }
                    None => (None, None),
                }
            }
            Event::Show(s) => {
                let league = s.league.clone_stub();
                (from_show(s), league)
            }
            Event::Bad(e) => {
                log(format!("v2 matches: event not readable, skipped: {e}"));
                (None, None)
            }
            Event::Other => (None, None),
        };
        match row {
            Some(row) => {
                if let Some(l) = league {
                    leagues.entry(l.ext_id).or_insert(l);
                }
                out.events.push(row);
            }
            None => out.skipped += 1,
        }
    }
    out.league_stubs = leagues.into_values().collect();
    out.team_stubs = teams.into_values().collect();
    out
}

/// `watchLiveQuery` → the rows of a schedule page (everything in it is `inProgress`, no match
/// type), plus the sides of every game that has a blue and a red team, with the match's teams
/// (`remap_sides`; a game whose teams can't be matched is left out)
pub fn from_live(data: watch_live::Data) -> (EventRows, Vec<GameSides>) {
    let mut sides = Vec::new();
    let events = data
        .esports
        .events
        .into_iter()
        .map(|e| match e {
            LiveEvent::Match(m) => {
                let mm = m.r#match;
                let event_teams: Vec<i64> = mm
                    .match_teams
                    .iter()
                    .filter_map(|t| ext_id(t.team_id()).ok())
                    .collect();
                for g in &mm.games {
                    let Some(mut s) = game_sides(&g.id, &g.teams) else {
                        continue;
                    };
                    match remap_sides(&mut s, &event_teams) {
                        Ok(Some((answer, event))) => {
                            log(format!(
                                "v2 live: game {}: team {answer} is not in the match, taken as {event}",
                                s.game_ext_id
                            ));
                            sides.push(s);
                        }
                        Ok(None) => sides.push(s),
                        Err(e) => log(format!("v2 live: sides left out: {e:#}")),
                    }
                }
                Event::Match(EventMatch {
                    id: m.id,
                    block_name: m.block_name,
                    start_time: m.start_time,
                    state: "inProgress".into(),
                    league: m.league,
                    tournament: m.tournament,
                    r#match: schedule_events::Match {
                        r#type: None,
                        strategy: mm.strategy,
                        games: mm
                            .games
                            .into_iter()
                            .map(|g| schedule_events::Game {
                                id: g.id,
                                number: g.number,
                                state: g.state,
                            })
                            .collect(),
                    },
                    match_teams: mm
                        .match_teams
                        .into_iter()
                        .map(|t| schedule_events::MatchTeam {
                            team_id: t.team_id().to_string(),
                            code: t.code,
                            name: t.name,
                            image: t.image,
                            result: t.result,
                        })
                        .collect(),
                    streams: m.streams,
                })
            }
            LiveEvent::Show(s) => Event::Show(EventShow {
                id: s.id,
                start_time: s.start_time,
                state: "inProgress".into(),
                league: s.league,
                tournament: s.tournament,
                streams: s.streams,
            }),
            LiveEvent::Bad(b) => Event::Bad(b),
            LiveEvent::Other => Event::Other,
        })
        .collect();
    let page = schedule_events::Data {
        esports: schedule_events::Esports {
            events,
            pages: schedule_events::Pages { newer: None },
        },
    };
    (from_schedule(page), sides)
}

impl schedule_events::League {
    fn clone_stub(&self) -> Option<LeagueStub> {
        Some(LeagueStub {
            ext_id: id(&self.id)?,
            slug: self.slug.clone(),
            name: self.name.clone(),
            image_url: self.image.clone().unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::{ops::schedule_events::GetScheduleEvents, parse_body};

    fn page(body: &str) -> EventRows {
        from_schedule(parse_body::<GetScheduleEvents>(200, body).unwrap())
    }

    #[test]
    fn live_answer_as_schedule_rows() {
        use crate::client::gql::ops::watch_live::GetWatchLive;
        let data = parse_body::<GetWatchLive>(200, fixture!("gql/watch_live.json")).unwrap();
        let (rows, sides) = from_live(data);
        // the match and the show; the bad event and the unknown type are skipped
        assert_eq!(rows.events.len(), 2);
        assert_eq!(rows.skipped, 2);
        let m = &rows.events[0];
        assert_eq!(
            (m.state.as_str(), m.match_type.as_deref()),
            ("inProgress", None)
        );
        assert_eq!(m.strategy_count, Some(5));
        assert_eq!(
            m.teams
                .iter()
                .map(|t| (t.team_ext_id, t.game_wins))
                .collect::<Vec<_>>(),
            [
                (Some(100205576309502431), Some(1)),
                (Some(98767991877340524), Some(0))
            ]
        );
        assert_eq!(m.games.len(), 5);
        assert_eq!(m.streams.len(), 1);
        assert_eq!(rows.events[1].state, "inProgress");
        // games 1-3 have a blue and a red team, 4 and 5 none yet
        assert_eq!(
            sides.iter().map(|s| s.game_ext_id).collect::<Vec<_>>(),
            [115660588583171795, 115660588583171796, 115660588583171797]
        );
        assert_eq!(
            sides[1].sides,
            [(100205576309502431, "red"), (98767991877340524, "blue")]
        );
    }

    #[test]
    fn live_sides_take_the_match_teams() {
        use crate::client::gql::ops::watch_live::GetWatchLive;
        // game 2: the red team under an id the match doesn't list; game 3: a side not picked
        let body = fixture!("gql/watch_live.json")
            .replace(
                r#""number":2,"state":"inProgress","teams":[{"__typename":"GameTeam","id":"100205576309502431""#,
                r#""number":2,"state":"inProgress","teams":[{"__typename":"GameTeam","id":"1""#,
            )
            .replace(
                r#""number":3,"state":"unstarted","teams":[{"__typename":"GameTeam","id":"100205576309502431","side":"blue"}"#,
                r#""number":3,"state":"unstarted","teams":[{"__typename":"GameTeam","id":"100205576309502431","side":null}"#,
            );
        let (rows, sides) = from_live(parse_body::<GetWatchLive>(200, &body).unwrap());
        // the match still decodes; game 2 takes the match's other team, game 3 is left out
        assert_eq!(rows.events.len(), 2);
        assert_eq!(
            sides.iter().map(|s| s.game_ext_id).collect::<Vec<_>>(),
            [115660588583171795, 115660588583171796]
        );
        assert_eq!(
            sides[1].sides,
            [(100205576309502431, "red"), (98767991877340524, "blue")]
        );
    }

    #[test]
    fn fixture_rows() {
        let rows = page(fixture!("gql/schedule_events_en.json"));
        assert_eq!(rows.events.len(), 5);
        assert_eq!(rows.skipped, 0);
        let tbd = &rows.events[0];
        assert_eq!(tbd.kind, "match");
        assert!(tbd.teams.iter().all(|t| t.team_ext_id.is_none()));
        assert_eq!(tbd.teams.iter().map(|t| t.slot).collect::<Vec<_>>(), [1, 2]);
        let bo5 = &rows.events[2];
        assert_eq!(bo5.ext_id, 115660588583171798);
        assert_eq!(
            (bo5.strategy_type.as_deref(), bo5.strategy_count),
            (Some("bestOf"), Some(5))
        );
        assert_eq!(bo5.teams[0].team_ext_id, Some(107251245690956393));
        assert_eq!(
            (bo5.teams[0].game_wins, bo5.teams[0].outcome.as_deref()),
            (Some(3), Some("win"))
        );
        assert_eq!(bo5.games.len(), 5);
        assert_eq!(bo5.games[4].state, "unneeded");
        assert_eq!(bo5.block_name.as_deref(), Some("Play-Ins"));
        assert_eq!(bo5.start_time.unwrap().to_string(), "2026-10-17 18:00:00");
        assert_eq!(rows.events[3].match_type.as_deref(), Some("tiebreaker"));
        assert_eq!(rows.events[4].strategy_type.as_deref(), Some("playAll"));
        // TBD slots add no team stub
        assert_eq!(rows.team_stubs.len(), 8);
        assert!(rows.league_stubs.iter().any(|l| l.slug == "worlds"));
    }

    #[test]
    fn es_page_has_its_block_names() {
        let rows = page(fixture!("gql/schedule_events_es.json"));
        assert_eq!(rows.events[3].block_name.as_deref(), Some("Semana 8"));
    }

    #[test]
    fn swapped_order_moves_whole_slots() {
        let body = fixture!("gql/schedule_events_en.json");
        let mut v: serde_json::Value = serde_json::from_str(body).unwrap();
        let teams = v["data"]["esports"]["events"][2]["matchTeams"]
            .as_array_mut()
            .unwrap();
        teams.reverse();
        let rows = page(&v.to_string());
        let bo5 = &rows.events[2];
        assert_eq!(bo5.teams[0].team_ext_id, Some(100205576309502431));
        assert_eq!(
            (
                bo5.teams[0].slot,
                bo5.teams[0].game_wins,
                bo5.teams[0].outcome.as_deref()
            ),
            (1, Some(1), Some("loss"))
        );
        assert_eq!(bo5.teams[1].team_ext_id, Some(107251245690956393));
        assert_eq!(bo5.teams[1].game_wins, Some(3));
        assert_eq!(bo5.progress(), page(body).events[2].progress());
    }

    #[test]
    fn unknown_values_skip_only_that_event() {
        let body = fixture!("gql/schedule_events_en.json");
        for (from, to) in [
            (r#""state": "completed""#, r#""state": "canceled""#),
            (r#""outcome": "win""#, r#""outcome": "forfeit""#),
            (r#""type": "bestOf""#, r#""type": "bestOfSomething""#),
        ] {
            assert!(body.contains(from), "{from}");
            let rows = page(&body.replacen(from, to, 1));
            assert_eq!((rows.events.len(), rows.skipped), (4, 1), "{to}");
        }
    }

    #[test]
    fn bad_id_skips_only_that_event() {
        let body = fixture!("gql/schedule_events_en.json")
            .replace("\"115660588583171798\"", "\"not-an-id\"");
        let rows = page(&body);
        assert_eq!(rows.events.len(), 4);
        assert_eq!(rows.skipped, 1);
    }

    #[test]
    fn progress_from_the_recording() {
        let ev = |state: &str, wins: [i32; 2], outcomes: bool| EventRow {
            ext_id: 1,
            kind: "match",
            league_ext_id: 1,
            tournament_ext_id: None,
            start_time: None,
            state: state.into(),
            strategy_type: None,
            strategy_count: None,
            match_type: None,
            block_name: None,
            teams: wins
                .iter()
                .enumerate()
                .map(|(i, w)| EventTeamRow {
                    slot: i as i32 + 1,
                    team_ext_id: Some(i as i64 + 10),
                    game_wins: Some(*w),
                    outcome: outcomes.then(|| if i == 0 { "win" } else { "loss" }.into()),
                })
                .collect(),
            games: vec![],
            streams: vec![],
        };
        let back =
            |new: &EventRow, stored: &EventRow| new.progress().went_back_from(&stored.progress());
        // getSchedule 20:39 completed, 20:40 (a newer answer) says inProgress again
        assert!(back(
            &ev("inProgress", [3, 0], true),
            &ev("completed", [3, 0], true)
        ));
        // getEventDetails 21:38 game 1 done 0-1, 21:39 back to 0-0
        assert!(back(
            &ev("inProgress", [0, 0], false),
            &ev("inProgress", [0, 1], false)
        ));
        // getSchedule 00:15 completed 2-1, 00:17 3-1 with the outcome: forward
        assert!(!back(
            &ev("completed", [3, 1], true),
            &ev("completed", [2, 1], false)
        ));
        // a state going up with fewer wins is an old copy too
        assert!(back(
            &ev("completed", [1, 1], true),
            &ev("inProgress", [2, 1], false)
        ));
        // a TBD slot gets its team before the match: same progress, accepted
        let mut tbd = ev("unstarted", [0, 0], false);
        let known = tbd.clone();
        tbd.teams[0].team_ext_id = None;
        assert_eq!(tbd.progress(), known.progress());
    }
}
