//! v2 tables: filled from the new API (only the alternative team image comes from the old one),
//! on their own loop (no DataPull lock)

use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};

use crate::{
    client::{
        adapters::v2::reference::{add_stage_names, from_answers},
        gql::{
            Gql,
            ops::{
                lol_team_overview::GetLoLTeamOverview,
                other_leagues::{self, GetOtherLeagues},
                season_tournament::{self, GetSeasonTournament},
            },
        },
        legacy::{self, endpoints, models::Wrapper},
        log,
    },
    dao::v2::{
        reference::{write_alt_images, write_reference},
        rows::ReferenceRows,
    },
};

/// `TRIFORCE_V2` value: `on` (default when unset) or `off`
pub fn parse_v2(value: Option<&str>) -> Result<bool> {
    match value {
        None => Ok(true),
        Some(v) => match v.trim().to_ascii_lowercase().as_str() {
            "on" => Ok(true),
            "off" => Ok(false),
            _ => Err(eyre!("TRIFORCE_V2 must be on or off, got {v:?}")),
        },
    }
}

pub fn v2_enabled() -> Result<bool> {
    match crate::env::var("TRIFORCE_V2") {
        Err(std::env::VarError::NotPresent) => parse_v2(None),
        Err(e) => Err(eyre!("TRIFORCE_V2: {e}")),
        Ok(v) => parse_v2(Some(&v)),
    }
}

pub fn parse_locales(value: Option<&str>) -> Vec<String> {
    let mut out = vec!["en-US".to_string()];
    for raw in value.unwrap_or("").split(',') {
        let l = raw.trim();
        if l.is_empty() || out.iter().any(|x| x == l) {
            continue;
        }
        let ok = l.len() == 5
            && l.as_bytes()[2] == b'-'
            && l[..2].chars().all(|c| c.is_ascii_lowercase())
            && l[3..].chars().all(|c| c.is_ascii_uppercase());
        if ok {
            out.push(l.to_string())
        } else {
            log(format!(
                "TRIFORCE_LOCALES: {l:?} skipped (expected like es-ES)"
            ))
        }
    }
    out
}

pub fn locales_from_env() -> Vec<String> {
    parse_locales(crate::env::var("TRIFORCE_LOCALES").ok().as_deref())
}

/// The API answers an unknown locale with the en-US texts, those rows would be copies
fn same_texts(a: &other_leagues::Data, b: &other_leagues::Data) -> bool {
    a.leagues.len() == b.leagues.len()
        && a.leagues.iter().all(|la| {
            b.leagues
                .iter()
                .find(|lb| lb.id == la.id)
                .is_some_and(|lb| {
                    la.name == lb.name
                        && la.region == lb.region
                        && la.tournaments.len() == lb.tournaments.len()
                        && la.tournaments.iter().all(|ta| {
                            lb.tournaments
                                .iter()
                                .find(|tb| tb.id == ta.id)
                                .is_some_and(|tb| ta.name == tb.name)
                        })
                })
        })
}

fn drop_english_copies(by_locale: &mut Vec<(String, other_leagues::Data)>) {
    let Some(en) = by_locale.iter().position(|(l, _)| l == "en-US") else {
        return;
    };
    // en-US first, so removing the others never moves it
    by_locale.swap(0, en);
    let mut i = 1;
    while i < by_locale.len() {
        if same_texts(&by_locale[i].1, &by_locale[0].1) {
            let (locale, _) = by_locale.remove(i);
            log(format!(
                "v2 reference: {locale} answered with the en-US texts, skipped"
            ));
            continue;
        }
        i += 1;
    }
}

/// Only what the alt images need from `getTeams`: the full v1 model panics on a bad id
#[derive(serde::Deserialize)]
struct AltTeams {
    teams: Vec<AltTeam>,
}

#[derive(serde::Deserialize)]
struct AltTeam {
    id: String,
    #[serde(rename = "alternativeImage")]
    alternative_image: Option<String>,
}

fn parse_alt_images(body: &str) -> Result<Vec<(i64, Option<String>)>> {
    let parsed: Wrapper<AltTeams> = serde_json::from_str(body)?;
    let mut alts = Vec::with_capacity(parsed.data.teams.len());
    let mut bad = 0;
    for t in parsed.data.teams {
        match t.id.parse::<i64>() {
            Ok(id) => alts.push((id, t.alternative_image)),
            Err(_) => bad += 1,
        }
    }
    if bad > 0 {
        log(format!(
            "v2 reference: {bad} getTeams ids not numeric, skipped"
        ));
    }
    // an old API being shut down could send them all empty: keep the stored ones
    if !alts.iter().any(|(_, alt)| alt.is_some()) {
        return Err(eyre!("getTeams sent no alternativeImage at all"));
    }
    Ok(alts)
}

/// `alternativeImage` per team from the old `getTeams`, the new API doesn't have it
async fn old_alt_images() -> Result<Vec<(i64, Option<String>)>> {
    parse_alt_images(&legacy::get_text(endpoints::TEAMS_AND_LEAGUES_ENDPOINT).await?)
}

/// Stage names for every tournament in `rows`, per locale. A failed call only skips its 100
/// tournaments, 2 in a row skip the rest of that locale. Names stored by an earlier run stay
async fn stage_names(gql: &Gql, rows: &mut ReferenceRows, locales: &[String]) -> usize {
    let ids: Vec<String> = rows
        .tournaments
        .iter()
        .map(|t| t.ext_id.to_string())
        .collect();
    let calls = ids.len().div_ceil(season_tournament::MAX_IDS);
    let mut added = 0;
    for locale in locales {
        let mut failed_in_a_row = 0;
        for (i, chunk) in ids.chunks(season_tournament::MAX_IDS).enumerate() {
            let vars = season_tournament::Vars {
                hl: locale.clone(),
                tournament_id: chunk.to_vec(),
            };
            match gql.call::<GetSeasonTournament>(&vars).await {
                Ok(a) => {
                    added += add_stage_names(rows, locale, a.data);
                    failed_in_a_row = 0;
                }
                Err(e) => {
                    // as a Report, so `{:#}` also prints the causes
                    let e = color_eyre::Report::new(e);
                    log(format!(
                        "v2 reference: stage names {locale}, call {}/{calls} skipped: {e:#}",
                        i + 1
                    ));
                    failed_in_a_row += 1;
                    // the API looks down: don't wait ~55 s on each call left
                    if failed_in_a_row == 2 {
                        break;
                    }
                }
            }
        }
    }
    added
}

pub async fn reference_once(gql: &Gql, locales: &[String]) -> Result<()> {
    let mut by_locale = Vec::new();
    for locale in locales {
        let vars = other_leagues::Vars {
            locale: locale.clone(),
            ..Default::default()
        };
        match gql.call::<GetOtherLeagues>(&vars).await {
            Ok(a) => by_locale.push((locale.clone(), a.data)),
            Err(e) if locale != "en-US" => log(format!("v2 reference: {locale} skipped: {e:#}")),
            Err(e) => return Err(e).wrap_err("GetOtherLeagues en-US"),
        }
    }
    drop_english_copies(&mut by_locale);
    // only locales GetOtherLeagues translated: an unknown one answers with the en-US texts
    let written_locales: Vec<String> = by_locale.iter().map(|(l, _)| l.clone()).collect();
    let teams = gql
        .call::<GetLoLTeamOverview>(&Default::default())
        .await
        .wrap_err("GetLoLTeamOverview")?
        .data;
    let mut rows = from_answers(by_locale, teams).wrap_err("v2 adapter")?;
    let stage_count = stage_names(gql, &mut rows, &written_locales).await;
    let orphans = rows
        .teams
        .iter()
        .filter(|t| {
            t.home_league_ext_id
                .is_some_and(|h| !rows.leagues.iter().any(|l| l.ext_id == h))
        })
        .count();
    if orphans > 0 {
        log(format!(
            "v2 reference: {orphans} teams have a home league missing from GetOtherLeagues (NULL unless an earlier run stored it)"
        ));
    }
    write_reference(&rows).await?;
    // while the old API lives: a failure there only skips the alt images
    let alts = match old_alt_images().await {
        Ok(alts) => match write_alt_images(&alts).await {
            Ok(()) => format!(
                "{} alt images",
                alts.iter().filter(|(_, alt)| alt.is_some()).count()
            ),
            Err(e) => {
                log(format!("v2 reference: alt images not written: {e:#}"));
                "alt images not written".to_string()
            }
        },
        Err(e) => {
            log(format!(
                "v2 reference: alt images from the old API skipped: {e:#}"
            ));
            "alt images skipped".to_string()
        }
    };
    log(format!(
        "v2 reference: {} leagues, {} tournaments, {} teams, {} players, {} locales, {stage_count} stage names, {alts}",
        rows.leagues.len(),
        rows.tournaments.len(),
        rows.teams.len(),
        rows.players.len(),
        written_locales.len()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_images_skip_bad_ids() {
        let body = r#"{"data":{"teams":[
            {"id":"1","alternativeImage":"http://x/a.png","players":[{"id":"oops"}]},
            {"id":"bad","alternativeImage":"http://x/b.png"},
            {"id":"2","alternativeImage":null}]}}"#;
        assert_eq!(
            parse_alt_images(body).unwrap(),
            vec![(1, Some("http://x/a.png".to_string())), (2, None)]
        );
    }

    #[test]
    fn no_alt_image_at_all_is_an_error() {
        let body = r#"{"data":{"teams":[{"id":"1","alternativeImage":null},{"id":"2"}]}}"#;
        assert!(parse_alt_images(body).is_err());
    }

    #[test]
    fn locales() {
        assert_eq!(parse_locales(None), vec!["en-US"]);
        assert_eq!(
            parse_locales(Some("es-ES, en-US,ko-KR,es-ES,bad")),
            vec!["en-US", "es-ES", "ko-KR"]
        );
    }

    fn fixture(body: &str) -> other_leagues::Data {
        crate::client::gql::parse_body::<GetOtherLeagues>(200, body).unwrap()
    }

    #[test]
    fn same_texts_itself_and_other_locale() {
        let en = fixture(include_str!(
            "../client/adapters/v2/testdata/other_leagues_v2_en.json"
        ));
        let es = fixture(include_str!(
            "../client/adapters/v2/testdata/other_leagues_v2_es.json"
        ));
        assert!(same_texts(&en, &en));
        assert!(!same_texts(&en, &es));
    }

    #[test]
    fn english_copies_are_dropped() {
        let body = include_str!("../client/adapters/v2/testdata/other_leagues_v2_en.json");
        let es = include_str!("../client/adapters/v2/testdata/other_leagues_v2_es.json");
        let mut v = vec![
            ("en-US".to_string(), fixture(body)),
            ("zz-ZZ".to_string(), fixture(body)),
            ("es-ES".to_string(), fixture(es)),
        ];
        drop_english_copies(&mut v);
        let names: Vec<_> = v.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(names, ["en-US", "es-ES"]);
    }

    #[test]
    fn v2_switch() {
        assert!(parse_v2(None).unwrap());
        assert!(!parse_v2(Some(" OFF ")).unwrap());
        assert!(parse_v2(Some("on")).unwrap());
        assert!(parse_v2(Some("yes")).is_err());
    }
}
