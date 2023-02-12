//! The data access layer

use std::fmt::Error;

use self::models::{leagues::League, tournaments::Tournament, teams::Team, players::Player, team_player::TeamPlayer};
use crate::{data_pull::{serde_models::Leagues, self}, service::OurTournaments};
use canyon_sql::crud::CrudOperations;
use color_eyre::Result;
use itertools::Itertools;
mod models;

#[derive(Debug, Default)]
pub struct DatabaseOps {
    pub leagues: Vec<League>,
    pub tournaments: Vec<Tournament>,
    pub teams: Vec<Team>,
    pub players: Vec<Player>,
}

impl DatabaseOps {
    pub async fn bulk_leagues_in_database(&mut self, leagues: &Leagues) -> Result<()> {
        let mut db_leagues = leagues.leagues.iter().map(League::from).collect::<Vec<_>>();

        League::multi_insert(&mut db_leagues.iter_mut().collect::<Vec<&mut League>>())
            .await
            .map_err(|e| color_eyre::eyre::ErrReport::from(*e.downcast_ref::<Error>().unwrap()))?;

        self.leagues = db_leagues;

        Ok(())
    }

    pub async fn bulk_tournaments_in_database(
        &mut self,
        tournaments: &OurTournaments,
    ) -> Result<()> {
        let mut db_tournaments = tournaments
            .iter()
            .map(|serde_tournament| {
                let mut t = Tournament::from(serde_tournament);
                // t.league
                t.league = self
                    .leagues
                    .iter()
                    .find(|league| serde_tournament.league_id.0 == league.ext_id)
                    .map(|league| league.id)
                    .unwrap_or_default();
                t
            })
            .collect::<Vec<_>>();

        Tournament::multi_insert(&mut db_tournaments.iter_mut().collect::<Vec<&mut Tournament>>())
            .await
            .map_err(|e| color_eyre::eyre::ErrReport::from(*e.downcast_ref::<Error>().unwrap()))?;

        self.tournaments = db_tournaments;

        Ok(())
    }

    pub async fn bulk_teams_in_database(
        &mut self,
        teams: &Vec<data_pull::serde_models::Team>,
    ) -> Result<()> {
        let mut db_teams = teams
            .iter()
            .map(|serde_team| {
                let mut t = Team::from(serde_team);
                // t.league
                t.home_league = self
                    .leagues
                    .iter()
                    .find(|db_league| {
                        db_league.ext_id.eq(&serde_team.id.0)
                    })
                    .map(|l| l.id.into());
                t
            })
            .collect::<Vec<_>>();

        Team::multi_insert(&mut db_teams.iter_mut().collect::<Vec<&mut Team>>())
            .await
            .map_err(|e| color_eyre::eyre::ErrReport::from(*e.downcast_ref::<Error>().unwrap()))?;

        self.teams = db_teams;

        Ok(())
    }

    pub async fn bulk_players_in_database(
        &mut self,
        players: &Vec<data_pull::serde_models::Player>,
    ) -> Result<()> {
        let db_players = &mut players
            .iter()
            .map(|serde_player| Player::from(serde_player) )
            .unique_by(|player|player.ext_id)
            .collect::<Vec<_>>();

        let result = Player::multi_insert(&mut db_players.iter_mut().collect::<Vec<&mut Player>>())
            .await
            .map_err(|e| color_eyre::eyre::ErrReport::from(*e.downcast_ref::<Error>().unwrap()))?;
       

        self.players = db_players.to_vec();

        Ok(())
    }

    pub async fn bulk_team_player_in_database(
        &mut self,
        teams: &Vec<data_pull::serde_models::Team>,
    ) -> Result<()> {
        let mut vec_team_player: Vec<TeamPlayer> = vec![];
        teams.iter()
            .for_each(|serde_team| {
                let team_id = self.teams.iter()
                    .find(|db_team|db_team.ext_id == serde_team.id.0)
                    .map(|db_team|db_team.id).expect("Error matching Team Id");

                for serde_player in &serde_team.players {
                    let player_id = self.players.iter()
                        .find(|db_player| db_player.ext_id == serde_player.id.0)
                        .map(|db_player| db_player.id).expect("Error matching Player Id");

                    let team_player = TeamPlayer { 
                        id: Default::default(),
                        team_id: Some(team_id.into()),
                        player_id: Some(player_id.into()) 
                    };

                    vec_team_player.push(team_player)
                }
            });

        TeamPlayer::multi_insert(&mut vec_team_player.iter_mut().collect::<Vec<&mut TeamPlayer>>())
            .await
            .map_err(|e| color_eyre::eyre::ErrReport::from(*e.downcast_ref::<Error>().unwrap()))
    }

}
