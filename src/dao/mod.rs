//! The data access layer

use self::models::{
    event::{Schedule, ScheduleFieldValue},
    leagues::League,
    players::Player,
    streams::Stream,
    team_player::{TeamPlayer, TeamPlayerFieldValue},
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
    runtime::futures::future::ok,
};
use chrono::{Days, Local, Utc};
use color_eyre::Result;
use itertools::Itertools;
mod models;

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
        println!(
            "{} - Processing leagues to insert or update on database",
            Local::now().format("%Y-%m-%d %H:%M:%S.%f")
        );
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
        let db_leagues = League::find_all().await;

        let db_tournaments = Tournament::find_all().await;

        println!(
            "{} - Processing tournaments to insert or update on database",
            Local::now().format("%Y-%m-%d %H:%M:%S.%f")
        );

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
        println!(
            "{} - Processing teams to insert or update on database",
            Local::now().format("%Y-%m-%d %H:%M:%S.%f")
        );

        let db_leagues = League::find_all().await;

        let db_teams = Team::find_all().await;

        match (db_leagues, db_teams) {
            (Ok(on_db_leagues), Ok(on_db_teams)) => {
                let fetched_teams = teams
                    .iter()
                    .map(|serde_team| {
                        let mut t = Team::from(serde_team);
                        // t.league
                        t.home_league = on_db_leagues
                            .iter()
                            .find(|db_league| db_league.ext_id.eq(&serde_team.id.0))
                            .map(|l| l.id.into());
                        t
                    })
                    .collect::<Vec<_>>();

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
                Ok(())
            }
            _ => Ok({
                println!("No se pudo recuperar los datos de base de datos");
            }),
        }
    }

    pub async fn bulk_players_in_database(
        &mut self,
        players: &Vec<data_pull::serde_models::Player>,
    ) -> Result<()> {
        println!(
            "{} - Processing players to insert or update on database",
            Local::now().format("%Y-%m-%d %H:%M:%S.%f")
        );

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
        println!(
            "{} - Processing players and teams to insert on database",
            Local::now().format("%Y-%m-%d %H:%M:%S.%f")
        );

        let db_teams = Team::find_all().await;

        let db_players = Player::find_all().await;

        match (db_players, db_teams) {
            (Ok(on_db_players), Ok(on_db_teams)) => {
                let mut vec_team_player: Vec<TeamPlayer> = vec![];

                println!(
                    "{} - Deleting all rows in the table",
                    Local::now().format("%Y-%m-%d %H:%M:%S.%f")
                );

                let _ = TeamPlayer::delete_query()
                    .r#where(TeamPlayerFieldValue::id(&&0), Comp::Gt)
                    .query()
                    .await;

                println!(
                    "{} - All rows delete !\nStarting to process the data to insert",
                    Local::now().format("%Y-%m-%d %H:%M:%S.%f")
                );

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
                println!(
                    "{} - Inserting all Team-Player relation rows",
                    Local::now().format("%Y-%m-%d %H:%M:%S.%f")
                );
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
        println!(
            "{} - Processing schedule data to insert/update on database",
            Local::now().format("%Y-%m-%d %H:%M:%S.%f")
        );

        let db_leagues = League::find_all().await;
        let db_teams = Team::find_all().await;
        let db_events = Schedule::find_all().await;

        match (db_leagues, db_teams, db_events) {
            (Ok(on_db_leagues), Ok(on_db_teams), Ok(on_db_events)) => {
                let fetched_events = events
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

                            db_event.team_right_id = on_db_teams
                                .iter()
                                .find(|db_team| db_team.name.eq(&team_2.name))
                                .map(|l| l.id.into());

                            db_event.team_right_wins = match &team_2.result {
                                Some(result) => Some(result.game_wins.into()),
                                None => None,
                            };

                            // Added because sometimes the state of the match is "unstarted" or "inProgress" even when the match ended hours ago,
                            // but the match result is updated, so we need to manually correct this inconsistency.
                            // Error first seen in EMEA Masters (formerly known as EU Masters) on 04/04/2023, with matches that ended 7 hours ago still having the "unstarted" state.
                            // At some point on 05/04/2023, those matches had their states updated, but the matches of the day (same league) had the same problem.
                            if let (Some(strategy_count), Some(right_wins), Some(left_wins)) = (
                                db_event.strategy_count,
                                db_event.team_right_wins,
                                db_event.team_left_wins,
                            ) {
                                // FIXME This is not right, this only account for BO with all games played
                                if strategy_count == right_wins + left_wins
                                    && db_event.state != "completed"
                                {
                                    db_event.state = "completed".to_string();
                                }
                            }
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

    pub async fn bulk_eventdetails_in_database(
        &mut self,
        events: &Vec<data_pull::serde_models::EventDetails>,
    ) -> Result<()> {
        let db_leagues = League::find_all().await;
        let db_events = Schedule::select_query()
            .r#where(
                ScheduleFieldValue::start_time(
                    &Utc::now().naive_utc().checked_sub_days(Days::new(1)),
                ),
                Comp::Gt,
            )
            .query()
            .await;
        // TODO Should we update the teams IDs ?
        match (db_leagues, db_events) {
            (Ok(on_db_leagues), Ok(mut on_db_events)) => {
                let db_streams = Stream::find_all().await;

                match db_streams {
                    Ok(on_db_streams) => {
                        for event in events {
                            let db_event: Option<&mut Schedule> = match &event.r#match {
                                Some(_event_match) => {
                                    let matching_event = on_db_events
                                        .iter_mut()
                                        .find(|ev| ev.match_id.unwrap_or_default() == event.id.0);
                                    matching_event
                                }
                                None => {
                                    let event_league_on_db =
                                        on_db_leagues.iter().find(|l| l.slug == event.league.slug);

                                    let show_event = on_db_events.iter_mut().find(|ev| {
                                        ev.event_type == event.r#type
                                            && match (ev.league_id, &event_league_on_db) {
                                                (Some(ev_league_id), Some(event_league)) => {
                                                    ev_league_id == event_league.id as i64
                                                }
                                                (None, None) => true,
                                                _ => false,
                                            }
                                            && ev
                                                .start_time
                                                .and_then(|ev_start| {
                                                    event.start_time.map(|event_start| {
                                                        let diff = event_start
                                                            .0
                                                            .signed_duration_since(ev_start);
                                                        diff.num_minutes().abs() <= 20
                                                    })
                                                })
                                                .unwrap_or(false)
                                    });
                                    show_event
                                }
                            };

                            let event_id;

                            match db_event {
                                Some(e) => {
                                    e.merge_with_event_details(event);

                                    println!("\nNew event data from Live - Updating {:?}\n", &e);
                                    let _ = e.update().await;
                                    event_id = e.id;
                                }
                                None => {
                                    println!("\nNew event from Live to insert {:?}\n", &event);
                                    let mut event_to_db = Schedule::from(event);
                                    event_to_db.league_id = on_db_leagues
                                        .iter()
                                        .find(|db_league| db_league.slug.eq(&event.league.slug))
                                        .map(|l| l.id.into());
                                    let _ = event_to_db.insert().await;
                                    event_id = event_to_db.id;
                                }
                            }

                            for stream in event.streams.iter() {
                                let matching_stream = on_db_streams.iter().find(|stm| {
                                    stm.event_id == event_id
                                        && stm.english_name == stream.media_locale.english_name
                                        && stm.locale == stream.media_locale.locale
                                        && stm.parameter == stream.parameter
                                        && stm.provider == stream.provider
                                });
                                if matching_stream.is_some() {
                                    continue;
                                }

                                let mut db_stream = Stream::from(stream);
                                db_stream.event_id = event_id;
                                let result = db_stream.insert().await;
                                if result.is_err() {
                                    println!("\nError inserting stream {:?} \n", &result);
                                }
                            }
                        }

                        Ok(())
                    }
                    Err(error) => Ok({
                        println!(
                            "No se pudo recuperar los eventos  de base de datos. Err {:?}",
                            error
                        );
                    }),
                }
            }
            _ => Ok({
                println!("No se pudo recuperar los datos de base de datos");
            }),
        }
    }
}
