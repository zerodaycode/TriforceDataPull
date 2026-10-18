//! New API answers → v2 reference rows (structure from en-US, texts from every locale)

use std::collections::{BTreeMap, HashMap};

use color_eyre::{Result, eyre::eyre};

use crate::{
    client::{
        adapters::parse_id,
        gql::ops::{lol_team_overview, other_leagues, season_tournament},
        log,
    },
    dao::v2::rows::*,
};

fn id(s: &str) -> Result<i64> {
    parse_id(s).map(|x| x.0)
}

/// File names the API uses for "no photo", with or without a `<timestamp>_` prefix
const PLACEHOLDER_NAMES: [&str; 7] = [
    "silhouette",
    "silhouette_transparent",
    "placeholder",
    "placeholder_",
    "default-headshot",
    "empphotoplaceholderimage",
    "esports_male_noplayer",
];
/// A photo shared by this many players is a placeholder, not a person
const SHARED_PHOTO: usize = 3;
/// Shared photos we already know about, not logged again
const KNOWN_SHARED: [&str; 3] = ["download.png", "pobrane1.png", "pobrane2.png"];

fn is_placeholder_name(url: &str) -> bool {
    let file = url.rsplit('/').next().unwrap_or(url);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let stem = match stem.split_once('_') {
        Some((ts, rest)) if !ts.is_empty() && ts.bytes().all(|b| b.is_ascii_digit()) => rest,
        _ => stem,
    };
    PLACEHOLDER_NAMES.contains(&stem.to_ascii_lowercase().as_str())
}

/// No photo is stored as NULL, so readers pick their own fallback
fn drop_placeholders(players: &mut BTreeMap<i64, PlayerRow>) {
    let mut uses: HashMap<String, usize> = HashMap::new();
    for url in players.values().filter_map(|p| p.image_url.as_ref()) {
        *uses.entry(url.clone()).or_default() += 1;
    }
    // so new placeholder names can go into PLACEHOLDER_NAMES
    let mut shared: Vec<&String> = uses
        .iter()
        .filter(|(u, n)| {
            **n >= SHARED_PHOTO
                && !u.trim().is_empty()
                && !is_placeholder_name(u)
                && !KNOWN_SHARED.contains(&u.rsplit('/').next().unwrap_or(u))
        })
        .map(|(u, _)| u)
        .collect();
    if !shared.is_empty() {
        shared.sort();
        log(format!(
            "v2 reference: player photos shared by {SHARED_PHOTO}+ players, stored as NULL: {shared:?}"
        ));
    }
    for p in players.values_mut() {
        if p.image_url.as_deref().is_some_and(|u| {
            u.trim().is_empty() || is_placeholder_name(u) || uses[u] >= SHARED_PHOTO
        }) {
            p.image_url = None;
        }
    }
}

pub fn from_answers(
    leagues_by_locale: Vec<(String, other_leagues::Data)>,
    teams: lol_team_overview::Data,
) -> Result<ReferenceRows> {
    let Some((first, _)) = leagues_by_locale.first() else {
        return Err(eyre!("no GetOtherLeagues answer"));
    };
    if first != "en-US" {
        return Err(eyre!("the first locale must be en-US, got {first}"));
    }
    let mut leagues: BTreeMap<i64, LeagueRow> = BTreeMap::new();
    let mut seasons: BTreeMap<i64, SeasonRow> = BTreeMap::new();
    let mut tournaments: BTreeMap<i64, TournamentRow> = BTreeMap::new();
    let mut stages: BTreeMap<i64, StageRow> = BTreeMap::new();
    let mut splits: Vec<i64> = Vec::new();
    for (i, (locale, data)) in leagues_by_locale.into_iter().enumerate() {
        let structure = i == 0;
        for l in data.leagues {
            let lid = id(&l.id)?;
            if structure {
                leagues.insert(
                    lid,
                    LeagueRow {
                        ext_id: lid,
                        slug: l.slug.clone(),
                        image_url: l.image.clone(),
                        dark_image_url: l.dark_image.clone(),
                        texts: vec![],
                    },
                );
            }
            let Some(league) = leagues.get_mut(&lid) else {
                continue;
            };
            league.texts.push(LeagueText {
                locale: locale.clone(),
                name: l.name.clone(),
                region: l.region.clone(),
                position: l.display_priority.as_ref().and_then(|p| p.position),
            });
            for t in l.tournaments {
                let tid = id(&t.id)?;
                if structure {
                    let season_ext_id = t.season.as_ref().map(|s| id(&s.id)).transpose()?;
                    let split_ext_id = t.split.as_ref().map(|s| id(&s.id)).transpose()?;
                    if let Some(s) = &t.season {
                        let sid = id(&s.id)?;
                        seasons.entry(sid).or_insert(SeasonRow {
                            ext_id: sid,
                            start_time: s.start_time.map(|x| x.naive_utc()),
                            end_time: s.end_time.map(|x| x.naive_utc()),
                            texts: vec![],
                        });
                    }
                    if let Some(sp) = split_ext_id
                        && !splits.contains(&sp)
                    {
                        splits.push(sp);
                    }
                    for st in &t.stages {
                        let stid = id(&st.id)?;
                        stages.insert(
                            stid,
                            StageRow {
                                ext_id: stid,
                                tournament_ext_id: tid,
                                // GetOtherLeagues has no stage names, they come from GetSeasonTournament (add_stage_names)
                                texts: vec![],
                            },
                        );
                    }
                    tournaments.insert(
                        tid,
                        TournamentRow {
                            ext_id: tid,
                            league_ext_id: lid,
                            season_ext_id,
                            split_ext_id,
                            slug: t.slug.clone(),
                            start_time: t.start_time.map(|x| x.naive_utc()),
                            end_time: t.end_time.map(|x| x.naive_utc()),
                            texts: vec![],
                        },
                    );
                }
                if let (Some(row), Some(name)) = (tournaments.get_mut(&tid), &t.name) {
                    row.texts.push(Text {
                        locale: locale.clone(),
                        name: name.clone(),
                    });
                }
                if let Some(s) = &t.season
                    && let (Some(row), Some(name)) = (seasons.get_mut(&id(&s.id)?), &s.name)
                    && !row.texts.iter().any(|x| x.locale == locale)
                {
                    row.texts.push(Text {
                        locale: locale.clone(),
                        name: name.clone(),
                    });
                }
            }
        }
    }
    let mut team_rows = Vec::new();
    let mut players: BTreeMap<i64, PlayerRow> = BTreeMap::new();
    let mut team_players = Vec::new();
    for t in teams.teams {
        let tid = id(&t.id)?;
        for p in t.players {
            let pid = id(&p.id)?;
            team_players.push((tid, pid));
            players.insert(
                pid,
                PlayerRow {
                    ext_id: pid,
                    // handle is newer than summoner_name and can be missing
                    handle: p.handle.unwrap_or_else(|| p.summoner_name.clone()),
                    first_name: p.first_name,
                    last_name: p.last_name,
                    image_url: p.image,
                    role: p.role,
                },
            );
        }
        team_rows.push(TeamRow {
            ext_id: tid,
            slug: t.slug,
            name: t.name,
            code: t.code,
            image_url: t.image,
            bg_image_url: t.background_image,
            home_league_ext_id: t.home_league.as_ref().map(|h| id(&h.id)).transpose()?,
        });
    }
    drop_placeholders(&mut players);
    Ok(ReferenceRows {
        leagues: leagues.into_values().collect(),
        seasons: seasons.into_values().collect(),
        splits,
        tournaments: tournaments.into_values().collect(),
        stages: stages.into_values().collect(),
        teams: team_rows,
        players: players.into_values().collect(),
        team_players,
    })
}

/// Stage names of one locale from a `GetSeasonTournament` answer. Stages not in `rows` are skipped.
/// Returns how many names were added
pub fn add_stage_names(
    rows: &mut ReferenceRows,
    locale: &str,
    data: season_tournament::Data,
) -> usize {
    let mut by_id: HashMap<i64, &mut StageRow> =
        rows.stages.iter_mut().map(|s| (s.ext_id, s)).collect();
    let mut added = 0;
    for st in data.tournaments.into_iter().flat_map(|t| t.stages) {
        let Some(name) = st.name.filter(|n| !n.trim().is_empty()) else {
            continue;
        };
        // every stage of ours came from GetOtherLeagues with a numeric id
        if let Some(stage) = id(&st.id).ok().and_then(|sid| by_id.get_mut(&sid))
            && !stage.texts.iter().any(|t| t.locale == locale)
        {
            stage.texts.push(Text {
                locale: locale.to_string(),
                name,
            });
            added += 1;
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::{
        ops::{
            lol_team_overview::GetLoLTeamOverview, other_leagues::GetOtherLeagues,
            season_tournament::GetSeasonTournament,
        },
        parse_body,
    };

    #[test]
    fn stage_names_per_locale() {
        let en =
            parse_body::<GetOtherLeagues>(200, include_str!("testdata/other_leagues_v2_en.json"))
                .unwrap();
        let teams = parse_body::<GetLoLTeamOverview>(
            200,
            include_str!("../testdata/lol_team_overview.json"),
        )
        .unwrap();
        let mut rows = from_answers(vec![("en-US".into(), en)], teams).unwrap();
        let answer = |name: &str| {
            let body = serde_json::json!({"data": {"tournaments": [{"id": "115548681802226458", "stages": [
                {"id": "115548681802750747", "name": name},
                {"id": "115548681802750748", "name": null},
                {"id": "999", "name": "Not ours"}]}]}});
            parse_body::<GetSeasonTournament>(200, &body.to_string()).unwrap()
        };
        assert_eq!(add_stage_names(&mut rows, "en-US", answer("Playoffs")), 1);
        assert_eq!(
            add_stage_names(&mut rows, "es-ES", answer("Eliminatorias")),
            1
        );
        // same locale twice: no second text
        assert_eq!(add_stage_names(&mut rows, "es-ES", answer("Otra")), 0);
        let stage = rows
            .stages
            .iter()
            .find(|s| s.ext_id == 115548681802750747)
            .unwrap();
        assert_eq!(
            stage
                .texts
                .iter()
                .map(|t| (t.locale.as_str(), t.name.as_str()))
                .collect::<Vec<_>>(),
            vec![("en-US", "Playoffs"), ("es-ES", "Eliminatorias")]
        );
        let other = rows
            .stages
            .iter()
            .find(|s| s.ext_id == 115548681802750748)
            .unwrap();
        assert!(other.texts.is_empty());
    }

    #[test]
    fn stage_names_skip_blank_and_bad_ids() {
        let en =
            parse_body::<GetOtherLeagues>(200, include_str!("testdata/other_leagues_v2_en.json"))
                .unwrap();
        let teams = parse_body::<GetLoLTeamOverview>(
            200,
            include_str!("../testdata/lol_team_overview.json"),
        )
        .unwrap();
        let mut rows = from_answers(vec![("en-US".into(), en)], teams).unwrap();
        let body =
            serde_json::json!({"data": {"tournaments": [{"id": "115548681802226458", "stages": [
            {"id": "not-a-number", "name": "Bad"},
            {"id": "115548681802750747", "name": "  "},
            {"id": "115548681802750748", "name": "Groups"}]}]}})
            .to_string();
        let data = parse_body::<GetSeasonTournament>(200, &body).unwrap();
        assert_eq!(add_stage_names(&mut rows, "en-US", data), 1);
        let blank = rows
            .stages
            .iter()
            .find(|s| s.ext_id == 115548681802750747)
            .unwrap();
        assert!(blank.texts.is_empty());
    }

    #[test]
    fn placeholder_names() {
        for url in [
            "http://static.lolesports.com/players/default-headshot.png",
            "http://static.lolesports.com/players/1633033031951_silhouette.png",
            "http://static.lolesports.com/players/1643734200321_silhouette_transparent.png",
            "http://static.lolesports.com/players/placeholder_.jpg",
            "http://static.lolesports.com/players/1590681292084_EMPPhotoPlaceholderImage.png",
            "http://static.lolesports.com/players/eSports_male_NoPlayer.png",
            "http://static.lolesports.com/players/placeholder.jpg",
            "http://static.lolesports.com/players/silhouette.png",
            "http://static.lolesports.com/players/1590681292084_placeholder.png",
        ] {
            assert!(is_placeholder_name(url), "{url}");
        }
        for url in [
            "http://static.lolesports.com/players/1718125212677_Grell.png",
            "http://x/1718125212677_placeholder_art.png",
            "http://x/abc_silhouette.png",
        ] {
            assert!(!is_placeholder_name(url), "{url}");
        }
    }

    #[test]
    fn shared_photo_is_a_placeholder() {
        let player = |ext_id: i64, url: &str| PlayerRow {
            ext_id,
            handle: "h".into(),
            first_name: "f".into(),
            last_name: "l".into(),
            image_url: Some(url.into()),
            role: "top".into(),
        };
        let mut players: BTreeMap<i64, PlayerRow> = [
            player(1, "http://x/download.png"),
            player(2, "http://x/download.png"),
            player(3, "http://x/download.png"),
            player(4, "http://x/1_twice.png"),
            player(5, "http://x/1_twice.png"),
            player(6, "http://x/1_silhouette.png"),
            player(7, " "),
        ]
        .into_iter()
        .map(|p| (p.ext_id, p))
        .collect();
        drop_placeholders(&mut players);
        let urls: Vec<Option<&str>> = players.values().map(|p| p.image_url.as_deref()).collect();
        assert_eq!(
            urls,
            vec![
                None,
                None,
                None,
                Some("http://x/1_twice.png"),
                Some("http://x/1_twice.png"),
                None,
                None
            ]
        );
    }

    #[test]
    fn two_locales() {
        let en =
            parse_body::<GetOtherLeagues>(200, include_str!("testdata/other_leagues_v2_en.json"))
                .unwrap();
        let es =
            parse_body::<GetOtherLeagues>(200, include_str!("testdata/other_leagues_v2_es.json"))
                .unwrap();
        let teams = parse_body::<GetLoLTeamOverview>(
            200,
            include_str!("../testdata/lol_team_overview.json"),
        )
        .unwrap();
        let rows = from_answers(vec![("en-US".into(), en), ("es-ES".into(), es)], teams).unwrap();

        let lcs = rows.leagues.iter().find(|l| l.slug == "lcs").unwrap();
        assert!(lcs.dark_image_url.is_some());
        assert_eq!(lcs.texts.len(), 2);
        assert_eq!(
            (
                lcs.texts[0].locale.as_str(),
                lcs.texts[0].region.as_str(),
                lcs.texts[0].position
            ),
            ("en-US", "NORTH AMERICA", Some(0))
        );
        assert_eq!(
            (
                lcs.texts[1].locale.as_str(),
                lcs.texts[1].region.as_str(),
                lcs.texts[1].position
            ),
            ("es-ES", "NORTEAMÉRICA", Some(2))
        );

        let lec = rows
            .tournaments
            .iter()
            .find(|t| t.slug == "lec_split_3_2026")
            .unwrap();
        assert_eq!(lec.split_ext_id, Some(115548016979679447));
        assert_eq!(
            lec.texts
                .iter()
                .map(|x| (x.locale.as_str(), x.name.as_str()))
                .collect::<Vec<_>>(),
            vec![("en-US", "Summer 2026"), ("es-ES", "Verano de 2026")]
        );
        assert_eq!(lec.start_time.unwrap().to_string(), "2026-07-23 22:00:00");
        assert_eq!(
            rows.stages
                .iter()
                .filter(|s| s.tournament_ext_id == lec.ext_id)
                .count(),
            2
        );
        // seasons and splits deduped by id
        assert_eq!(rows.splits.len(), 2);
        assert!(rows.seasons.iter().all(|s| !s.texts.is_empty()));

        assert_eq!(rows.teams.len(), 3);
        let t1 = rows.teams.iter().find(|t| t.code == "T1").unwrap();
        assert_eq!(t1.home_league_ext_id, Some(98767991310872058));
        assert_eq!(rows.players.len(), 10);
        assert_eq!(rows.team_players.len(), 10);
        assert_eq!(
            rows.team_players[..2],
            [
                (98767991853197861, 115643633313408264),
                (98767991853197861, 113560604011392262)
            ]
        );
    }

    #[test]
    fn missing_handle_falls_back_to_summoner_name() {
        let en =
            parse_body::<GetOtherLeagues>(200, include_str!("testdata/other_leagues_v2_en.json"))
                .unwrap();
        let body = include_str!("../testdata/lol_team_overview.json").replacen(
            "\"handle\": \"Guardian\", ",
            "",
            1,
        );
        assert!(!body.contains("\"handle\": \"Guardian\""));
        let teams = parse_body::<GetLoLTeamOverview>(200, &body).unwrap();
        let rows = from_answers(vec![("en-US".into(), en)], teams).unwrap();
        let p = rows
            .players
            .iter()
            .find(|p| p.ext_id == 115643633313408264)
            .unwrap();
        assert_eq!(p.handle, "Guardian");
    }

    #[test]
    fn en_us_must_be_first() {
        let es =
            parse_body::<GetOtherLeagues>(200, include_str!("testdata/other_leagues_v2_es.json"))
                .unwrap();
        let teams = parse_body::<GetLoLTeamOverview>(
            200,
            include_str!("../testdata/lol_team_overview.json"),
        )
        .unwrap();
        assert!(from_answers(vec![("es-ES".into(), es)], teams).is_err());
    }
}
