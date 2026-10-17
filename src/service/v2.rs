//! v2 tables: filled only from the new API, on their own loop (no DataPull lock)

use color_eyre::{
    Result,
    eyre::{WrapErr, eyre},
};

use crate::{
    client::{
        adapters::v2::reference::from_answers,
        gql::{
            Gql,
            ops::{
                lol_team_overview::GetLoLTeamOverview,
                other_leagues::{self, GetOtherLeagues},
            },
        },
        log,
    },
    dao::v2::reference::write_reference,
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
    match std::env::var("TRIFORCE_V2") {
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
    parse_locales(std::env::var("TRIFORCE_LOCALES").ok().as_deref())
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
    let written_locales = by_locale.len();
    let teams = gql
        .call::<GetLoLTeamOverview>(&Default::default())
        .await
        .wrap_err("GetLoLTeamOverview")?
        .data;
    let rows = from_answers(by_locale, teams).wrap_err("v2 adapter")?;
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
    log(format!(
        "v2 reference: {} leagues, {} tournaments, {} teams, {} players, {} locales",
        rows.leagues.len(),
        rows.tournaments.len(),
        rows.teams.len(),
        rows.players.len(),
        written_locales
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
