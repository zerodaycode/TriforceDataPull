//! The data access layer

use std::fmt::Error;

use self::models::{
    event::Schedule,
    leagues::League,
    players::Player,
    team_player::{TeamPlayer, TeamPlayerField, TeamPlayerFieldValue},
    teams::Team,
    tournaments::Tournament,
};
use crate::{
    data_pull::{self, serde_models::Leagues},
    service::OurTournaments,
};
use canyon_sql::{
    crud::CrudOperations,
    query::{operators::Comp, ops::QueryBuilder},
};
use color_eyre::Result;
use itertools::Itertools;
mod models;

use super::dao::League as DatabaseLeague;

#[derive(Debug, Default)]
pub struct DatabaseOps {
    pub leagues: Vec<League>,
    pub tournaments: Vec<Tournament>,
    pub teams: Vec<Team>,
    pub players: Vec<Player>,
    pub events: Vec<Schedule>,
}

impl DatabaseOps {
    pub async fn bulk_leagues_in_database(&mut self, leagues: &Leagues) -> Result<()> {
        let db_leagues = League::find_all().await;
        if let Ok(db_lgs) = db_leagues {
            for mut fetched_league in leagues
                .leagues
                .iter()
                .map(|serde_league| League::from(serde_league))
            {
                let db_league = db_lgs
                    .iter()
                    .find(|league| league.ext_id == fetched_league.ext_id);

                match db_league {
                    Some(l) => {
                        fetched_league.id = l.id;
                        let _ = fetched_league.update().await;
                    }
                    None => {
                        let _ = fetched_league.insert().await;
                    }
                }
            }
        } else {
            println!("No se pudo recuperar las ligas de base de datos")
        }

        Ok(())
    }

    pub async fn bulk_tournaments_in_database(
        &mut self,
        tournaments: &OurTournaments,
    ) -> Result<()> {
        // TODO Controlar fallo al recuperar
        let db_leagues = League::find_all().await;

        let db_tournaments = Tournament::find_all().await;

        match (db_tournaments, db_leagues) {
            (Ok(db_tnmts), Ok(db_lgs)) => {
                let processed_fetched_tournaments = tournaments
                    .iter()
                    .map(|serde_tournament| {
                        let mut t = Tournament::from(serde_tournament);
                        // t.league
                        t.league = db_lgs
                            .iter()
                            .find(|league| serde_tournament.league_id.0 == league.ext_id)
                            .map(|league| league.id)
                            .unwrap_or_default();
                        t
                    })
                    .collect::<Vec<_>>();

                for mut fetched_tnmt in processed_fetched_tournaments {
                    let db_tournament = db_tnmts
                        .iter()
                        .find(|tnmt| tnmt.ext_id == fetched_tnmt.ext_id);

                    match db_tournament {
                        Some(t) => {
                            fetched_tnmt.id = t.id;
                            let _ = fetched_tnmt.update().await;
                        }
                        None => {
                            let _ = fetched_tnmt.insert().await;
                        }
                    }
                }
                Ok(())
            }
            _ => Ok({
                println!("No se pudo recuperar los datos ligas y/o torneos de base de datos");
            }),
        }
    }

    pub async fn bulk_teams_in_database(
        &mut self,
        teams: &Vec<data_pull::serde_models::Team>,
    ) -> Result<()> {
        // TODO Controlar fallo al recuperar
        let db_leagues = League::find_all().await.unwrap();

        let db_teams = Team::find_all().await;

        let fetched_teams = teams
            .iter()
            .map(|serde_team| {
                let mut t = Team::from(serde_team);
                // t.league
                t.home_league = db_leagues
                    .iter()
                    .find(|db_league| db_league.ext_id.eq(&serde_team.id.0))
                    .map(|l| l.id.into());
                t
            })
            .collect::<Vec<_>>();

        if let Ok(on_db_teams) = db_teams {
            for mut fetched_team in fetched_teams {
                let db_team = on_db_teams
                    .iter()
                    .find(|team| team.ext_id == fetched_team.ext_id);

                match db_team {
                    Some(t) => {
                        fetched_team.id = t.id;
                        let _ = fetched_team.update().await;
                    }
                    None => {
                        let _ = fetched_team.insert().await;
                    }
                }
            }
        } else {
            println!("No se pudo recuperar los equipos de base de datos")
        }

        Ok(())
    }

    pub async fn bulk_players_in_database(
        &mut self,
        players: &Vec<data_pull::serde_models::Player>,
    ) -> Result<()> {
        let db_players = Player::find_all().await;

        let fetched_players = &mut players
            .iter()
            .map(|serde_player| Player::from(serde_player))
            .unique_by(|player| player.ext_id)
            .collect::<Vec<_>>();

        if let Ok(on_db_players) = db_players {
            for mut fetched_player in fetched_players {
                let db_player = on_db_players
                    .iter()
                    .find(|player| player.ext_id == fetched_player.ext_id);

                match db_player {
                    Some(p) => {
                        fetched_player.id = p.id;
                        let _ = fetched_player.update().await;
                    }
                    None => {
                        let _ = fetched_player.insert().await;
                    }
                }
            }
        } else {
            println!("No se pudo recuperar los jugadores de base de datos")
        }

        Ok(())
    }

    pub async fn bulk_team_player_in_database(
        &mut self,
        fetched_teams: &Vec<data_pull::serde_models::Team>,
    ) -> Result<()> {
        let db_teams = Team::find_all().await;

        let db_players = Player::find_all().await;

        match (db_players, db_teams) {
            (Ok(on_db_players), Ok(on_db_teams)) => {
                let mut vec_team_player: Vec<TeamPlayer> = vec![];

                // let _ = TeamPlayer::query("DELETE * FROM team_player",&[], "").await;

                let _ = TeamPlayer::delete_query()
                    .r#where(TeamPlayerFieldValue::id(&&0), Comp::Gt)
                    .query()
                    .await;

                fetched_teams.iter().for_each(|serde_team| {
                    let team_id = on_db_teams
                        .iter()
                        .find(|db_team| db_team.ext_id == serde_team.id.0)
                        .map(|db_team| db_team.id)
                        .expect("Error matching Team Id");

                    for serde_player in &serde_team.players {
                        let player_id = on_db_players
                            .iter()
                            .find(|db_player| db_player.ext_id == serde_player.id.0)
                            .map(|db_player| db_player.id)
                            .expect("Error matching Player Id");

                        let team_player = TeamPlayer {
                            id: Default::default(),
                            team_id: Some(team_id.into()),
                            player_id: Some(player_id.into()),
                        };

                        vec_team_player.push(team_player)
                    }
                });

                match TeamPlayer::multi_insert(
                    &mut vec_team_player.iter_mut().collect::<Vec<&mut TeamPlayer>>(),
                )
                .await
                {
                    Ok(result) => Ok(result),
                    Err(e) => {
                        println!("{e}");
                        todo!()
                    }
                }
            }
            _ => Ok({
                println!("No se pudo recuperar los datos de base de datos");
            }),
        }
    }

    pub async fn bulk_schedule_in_database(
        &mut self,
        events: &Vec<data_pull::serde_models::Event>,
    ) -> Result<()> {
        let db_leagues = League::find_all().await;
        let db_teams = Team::find_all().await;
        let db_events = Schedule::find_all().await;

        match (db_leagues, db_teams, db_events) {
            (Ok(on_db_leagues), Ok(on_db_teams), Ok(on_db_events)) => {
                let mut fetched_events = events
                    .iter()
                    .map(|serde_event| {
                        let mut db_event = Schedule::from(serde_event);

                        db_event.league_id = on_db_leagues
                            .iter()
                            .find(|db_league| db_league.slug.eq(&serde_event.league.slug))
                            .map(|l| l.id.into());

                        if serde_event.r#match.is_some()
                            && !&serde_event.r#match.as_ref().unwrap().teams.is_empty()
                        {
                            let team_1 = serde_event
                                .r#match
                                .as_ref()
                                .expect("Not match found for event type match")
                                .teams
                                .get(0)
                                .unwrap();

                            let team_2 = serde_event
                                .r#match
                                .as_ref()
                                .expect("Not match found for event type match")
                                .teams
                                .get(1)
                                .unwrap();

                            db_event.team_left_id = on_db_teams
                                .iter()
                                .find(|db_team| db_team.name.eq(&team_1.name))
                                .map(|l| l.id.into());

                            db_event.team_left_wins = match &team_1.result {
                                Some(result) => Some(result.game_wins.into()),
                                None => None,
                            };

                            db_event.team_right_id = self
                                .teams
                                .iter()
                                .find(|db_team| db_team.name.eq(&team_2.name))
                                .map(|l| l.id.into());

                            db_event.team_right_wins = match &team_2.result {
                                Some(result) => Some(result.game_wins.into()),
                                None => None,
                            };
                        }

                        db_event
                    })
                    .collect::<Vec<_>>();

                for mut fetched_event in fetched_events {
                    let db_event = on_db_events.iter().find(|event| {
                        (fetched_event.match_id.is_some()
                            && fetched_event.match_id == event.match_id)
                            || (fetched_event.match_id.is_none()
                                && fetched_event.start_time == event.start_time
                                && fetched_event.league_id == event.league_id)
                    });

                    match db_event {
                        Some(e) => {
                            fetched_event.id = e.id;
                            let _ = fetched_event.update().await;
                        }
                        None => {
                            let _ = fetched_event.insert().await;
                        }
                    }
                }
                Ok(())
            }
            _ => Ok({
                println!("No se pudo recuperar los datos de base de datos");
            }),
        }
    }
}
