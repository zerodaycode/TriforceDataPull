//! `GetOtherLeagues` → what `getLeagues` + `getTournamentsForLeague` gave us

use color_eyre::Result;

use super::parse_id;
use crate::client::{
    gql::ops::other_leagues::Data,
    legacy::models::{League, Leagues, Tournament},
    log,
};

pub fn from_other_leagues(data: Data) -> Result<(Leagues, Vec<Tournament>)> {
    let mut leagues = Vec::with_capacity(data.leagues.len());
    let mut tournaments = Vec::new();
    for league in data.leagues {
        let league_id = parse_id(&league.id)?;
        for t in league.tournaments {
            // the old API sent dates, ours are the UTC date of these times
            let (Some(start), Some(end)) = (t.start_time, t.end_time) else {
                log(format!(
                    "Tournament {} has no start/end time, skipped",
                    t.slug
                ));
                continue;
            };
            tournaments.push(Tournament {
                id: parse_id(&t.id)?,
                slug: t.slug,
                start_date: start.date_naive(),
                end_date: end.date_naive(),
                league_id,
            });
        }
        leagues.push(League {
            id: league_id,
            slug: league.slug,
            name: league.name,
            region: league.region,
            image: league.image,
        });
    }
    Ok((Leagues { leagues }, tournaments))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::{ops::other_leagues::GetOtherLeagues, parse_body};
    use chrono::NaiveDate;

    #[test]
    fn same_as_the_old_api() {
        let body = fixture!("gql/other_leagues.json");
        let data = parse_body::<GetOtherLeagues>(200, body).unwrap();
        let (leagues, tournaments) = from_other_leagues(data).unwrap();

        assert_eq!(leagues.leagues.len(), 2);
        let lcs = &leagues.leagues[0];
        assert_eq!(lcs.id.0, 98767991299243165);
        assert_eq!(
            (lcs.slug.as_str(), lcs.name.as_str(), lcs.region.as_str()),
            ("lcs", "LCS", "NORTH AMERICA")
        );
        assert_eq!(
            lcs.image,
            "http://static.lolesports.com/leagues/1706356907418_LCSNew-01-FullonDark.png"
        );

        // values stored from the old API on 2026-10-16: the UTC date of startTime/endTime
        assert_eq!(tournaments.len(), 2);
        let t = &tournaments[0];
        assert_eq!(
            (t.id.0, t.slug.as_str(), t.league_id.0),
            (111504625283627681, "lcs_spring_2024", 98767991299243165)
        );
        assert_eq!(t.start_date, NaiveDate::from_ymd_opt(2024, 1, 19).unwrap());
        assert_eq!(t.end_date, NaiveDate::from_ymd_opt(2024, 4, 1).unwrap());
        let t = &tournaments[1];
        assert_eq!(
            (t.id.0, t.slug.as_str(), t.league_id.0),
            (115548681802226458, "lec_split_3_2026", 98767991302996019)
        );
        assert_eq!(t.start_date, NaiveDate::from_ymd_opt(2026, 7, 23).unwrap());
        assert_eq!(t.end_date, NaiveDate::from_ymd_opt(2026, 9, 20).unwrap());
    }

    #[test]
    fn bad_id_is_an_error() {
        let body = fixture!("gql/other_leagues.json").replace("98767991299243165", "lcs");
        let data = parse_body::<GetOtherLeagues>(200, &body).unwrap();
        assert!(from_other_leagues(data).is_err());
    }

    #[test]
    fn tournament_without_times_is_skipped() {
        let body = fixture!("gql/other_leagues.json").replace("\"2024-01-19T23:00:00Z\"", "null");
        let data = parse_body::<GetOtherLeagues>(200, &body).unwrap();
        let (leagues, tournaments) = from_other_leagues(data).unwrap();
        assert_eq!(leagues.leagues.len(), 2);
        assert_eq!(tournaments.len(), 1);
        assert_eq!(tournaments[0].slug, "lec_split_3_2026");
    }
}
