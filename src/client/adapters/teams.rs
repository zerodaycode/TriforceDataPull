//! `GetLoLTeamOverview` → what `getTeams` gave us

use color_eyre::{Result, eyre::eyre};

use super::parse_id;
use crate::client::{
    gql::ops::lol_team_overview::Data,
    legacy::models::{HomeLeague, Player, Team},
};

/// The teams and, flattened in order, their players
pub fn from_team_overview(data: Data) -> Result<(Vec<Team>, Vec<Player>)> {
    if data.teams.is_empty() {
        return Err(eyre!(
            "GetLoLTeamOverview answered with no teams, keeping the previous ones"
        ));
    }
    let teams = data
        .teams
        .into_iter()
        .map(|t| {
            let players = t
                .players
                .into_iter()
                .map(|p| {
                    Ok(Player {
                        id: parse_id(&p.id)?,
                        first_name: p.first_name,
                        last_name: p.last_name,
                        summoner_name: p.summoner_name,
                        image: p.image,
                        role: p.role,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let home_league = t
                .home_league
                .map(|hl| {
                    Ok::<_, color_eyre::Report>(HomeLeague {
                        league_id: parse_id(&hl.id)?,
                        name: hl.name,
                        // not in GetLoLTeamOverview, nothing reads it
                        region: String::new(),
                    })
                })
                .transpose()?;
            Ok(Team {
                id: parse_id(&t.id)?,
                name: t.name,
                slug: t.slug,
                code: t.code,
                image: t.image,
                alternative_image: None,
                background_image: t.background_image,
                // not in GetLoLTeamOverview, nothing reads it
                status: String::new(),
                players,
                home_league,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let players = teams
        .iter()
        .flat_map(|t| t.players.iter().cloned())
        .collect();
    Ok((teams, players))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gql::{ops::lol_team_overview::GetLoLTeamOverview, parse_body};

    #[test]
    fn same_as_the_old_api() {
        let body = include_str!("testdata/lol_team_overview.json");
        let (teams, players) =
            from_team_overview(parse_body::<GetLoLTeamOverview>(200, body).unwrap()).unwrap();
        assert_eq!(teams.len(), 3);
        assert_eq!(players.len(), 10);
        assert_eq!(players[2].summoner_name, "N0way");
        let t1 = &teams[0];
        assert_eq!(
            (t1.id.0, t1.code.as_str(), t1.slug.as_str()),
            (98767991853197861, "T1", "t1")
        );
        // no alternativeImage in the new API, the DAO keeps the stored one
        assert_eq!(t1.alternative_image, None);
        // the new API sends the league id, the DAO still links by name (ROADMAP)
        let hl = t1.home_league.as_ref().unwrap();
        assert_eq!(
            (hl.league_id.0, hl.name.as_str()),
            (98767991310872058, "LCK")
        );
        assert_eq!(t1.players.len(), 2);
        assert_eq!(
            (t1.players[0].id.0, t1.players[0].summoner_name.as_str()),
            (115643633313408264, "Guardian")
        );
        assert!(teams[2].home_league.is_none());
    }

    #[test]
    fn bad_id_is_an_error() {
        let body =
            include_str!("testdata/lol_team_overview.json").replace("98767991853197861", "t1");
        let data = parse_body::<GetLoLTeamOverview>(200, &body).unwrap();
        assert!(from_team_overview(data).is_err());
    }

    #[test]
    fn null_players_is_empty() {
        let body = include_str!("testdata/lol_team_overview.json")
            .replace(r#""players": []"#, r#""players": null"#);
        let data = parse_body::<GetLoLTeamOverview>(200, &body).unwrap();
        let (teams, _) = from_team_overview(data).unwrap();
        assert_eq!(teams[1].players.len(), 0);
    }

    #[test]
    fn no_teams_is_an_error() {
        let data = parse_body::<GetLoLTeamOverview>(200, r#"{"data":{"teams":[]}}"#).unwrap();
        assert!(from_team_overview(data).is_err());
    }
}
