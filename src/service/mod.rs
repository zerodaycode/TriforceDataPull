pub mod caller;

use std::time::Duration;

use crate::{
    data_pull::serde_models::{
        Event, EventDetails, EventOutter, LeagueForTournaments, Leagues, LolesportsId, Player,
        ScheduleOutter, Team, TeamsPlayers, Tournament, Wrapper,
    },
    utils::constants::lolesports,
};
use chrono::{format::Parsed, Local};
use color_eyre::{eyre::Context, Result};
use tokio::time::sleep;

/**
 * This type alias are just a communist joke. They are the Lolesports tournaments only?
 * Nope. They are OUR tournaments.
 */
pub type OurTournaments = Vec<Tournament>;

/// Contains the operations against the `LolEsports` API to
/// fetch the content via REST request that `Triforce` needs
/// to pull, parse, handle and store.
#[derive(Debug, Default)]
pub struct DataPull {
    pub leagues: Leagues,
    pub tournaments: OurTournaments,
    pub teams: Vec<Team>,
    pub players: Vec<Player>,
    pub schedule: Vec<Event>,
    pub live: Vec<Event>,
    pub previous_live: Vec<Event>,
    pub recent_ended_events_match: Vec<EventDetails>,
    pub recent_ended_events_show: Vec<Event>,
}

impl DataPull {
    pub async fn fetch_leagues(&mut self) -> Result<()> {
        println!(
            "{} - Fetching Leagues from The LoLEsports API",
            Local::now().format("%Y-%m-%d %H:%M:%S.%f")
        );
        let response = caller::make_get_request::<&[()]>(lolesports::LEAGUES_ENDPOINT, None)
            .await
            .with_context(|| "A failure happened retrieving the Leagues from Lolesports");

        serde_json::from_str::<Wrapper<Leagues>>(&response?.text().await.unwrap())
            .map(|parsed| self.leagues = parsed.data)
            .with_context(|| "A failure happened parsing the Leagues from Lolesports")
    }

    pub async fn fetch_tournaments(&mut self) -> Result<()> {
        println!(
            "{} - Tournaments Leagues from The LoLEsports API",
            Local::now().format("%Y-%m-%d %H:%M:%S.%f")
        );
        for league in &self.leagues.leagues {
            let response = caller::make_get_request(
                lolesports::TOURNAMENTS_ENDPOINT,
                Some(&[("leagueId", &league.id)]),
            )
            .await
            .with_context(|| "A failure happened retrieving the Tournaments from Lolesports");

            serde_json::from_str::<Wrapper<LeagueForTournaments>>(&response?.text().await.unwrap())
                .map(|parsed| {
                    let mut tournaments_in_league = parsed.data.leagues[0].clone();
                    tournaments_in_league
                        .tournaments
                        .iter_mut()
                        .for_each(|e| e.league_id = league.id);
                    self.tournaments.extend(tournaments_in_league.tournaments)
                })
                .with_context(|| "A failure happened parsing the Tournaments from Lolesports")?;
        }

        Ok(())
    }

    pub async fn fetch_teams_and_players(&mut self) -> Result<()> {
        println!(
            "{} - Fetching Teams and Players from The LoLEsports API",
            Local::now().format("%Y-%m-%d %H:%M:%S.%f")
        );
        let response =
            caller::make_get_request::<&[()]>(lolesports::TEAMS_AND_LEAGUES_ENDPOINT, None)
                .await
                .with_context(|| {
                    "A failure happened retrieving the Teams and players from Lolesports"
                });

        serde_json::from_str::<Wrapper<TeamsPlayers>>(&response?.text().await.unwrap())
            .map(|parsed| {
                for mut team in parsed.data.teams {
                    if let Some(home_league) = &mut team.home_league {
                        home_league.league_id = self.search_league_by_name(&home_league.name);
                    }
                    self.teams.push(team.clone());
                    self.players.extend(team.players.into_iter())
                }
            })
            .with_context(|| "A failure happened parsing the Tournaments from Lolesports")
    }

    // pub async fn fetch_schedule(&mut self) -> Result<()> {
    //     let response = caller::make_get_request::<&[()]>(
    //         lolesports::SCHEDULE_ENDPOINT,
    //             None
    //         ).await
    //         .with_context(|| "A failure happened retrieving the schedule from Lolesports");

    //     serde_json::from_str::<Wrapper<ScheduleOutter>>(&response?.text().await.unwrap())
    //         .map(|parsed| self.schedule = parsed.data)
    //         .with_context(|| "A failure happened parsing the Schedule from Lolesports")
    // }

    // FIX ME Right now It only fetch present and future matches, not past
    pub async fn process_full_schedule(&mut self) -> Result<()> {
        println!(
            "{} - Fetching Full schedule from The LoLEsports API",
            Local::now().format("%Y-%m-%d %H:%M:%S.%f")
        );
        self.fetch_full_schedule().await?;
        // process
        for event in self.schedule.iter_mut() {
            event.league.league_id = self
                .leagues
                .leagues
                .iter()
                .find(|league| (*league.slug).eq(&event.league.slug))
                .map(|league| league.id)
                .unwrap_or_default();
        }
        Ok(())
    }

    async fn fetch_full_schedule(&mut self) -> Result<()> {
        let first_response = caller::make_get_request::<&[()]>(lolesports::SCHEDULE_ENDPOINT, None)
            .await
            .with_context(|| "A failure happened retrieving the schedule from Lolesports");

        let schedule_first_page =
            serde_json::from_str::<Wrapper<ScheduleOutter>>(&first_response?.text().await.unwrap())
                .with_context(|| "Error retrieving the first page of the schedule")?;
        // Appending the already downloaded first paginated resource of the schedule
        self.schedule
            .extend(schedule_first_page.data.schedule.events);

        // let mut old_entry_sentinel = schedule_first_page.data.schedule.pages.older;
        let mut newer_entry_sentinel = schedule_first_page.data.schedule.pages.newer;
        // let mut total_old_entries = 1;
        let mut total_new_entries = 1;

        // While the API returns a key with older entries, we will continue fetching the calendar
        // while let Some(older_events) = &old_entry_sentinel {
        //     sleep(Duration::from_millis(3500)).await;
        //     let r = caller::make_get_request(
        //         lolesports::SCHEDULE_ENDPOINT,
        //         Some(&[("pageToken", older_events)]),
        //     )
        //     .await
        //     .with_context(|| "A failure happened retrieving the schedule from Lolesports");
        //     serde_json::from_str::<Wrapper<ScheduleOutter>>(&r?.text().await.unwrap())
        //         .map(|parsed| {
        //             println!("Requesting pages: {:?}", &parsed.data.schedule.pages);
        //             println!("Total old entries fetched: {:?}", &total_old_entries);
        //             total_old_entries += 1;
        //             old_entry_sentinel = parsed.data.schedule.pages.older;
        //             println!("Evento sin id match {:?}", parsed.data.schedule.events.iter().filter(|e|e.r#match.is_none() || e.r#type != "match").collect::<Vec<&Event>>());
        //             self.schedule.extend(parsed.data.schedule.events);

        //         })
        //         .with_context(|| "A failure happened parsing the Schedule from Lolesports")?;
        // }

        // While the API returns a key with newer entries, we will continue fetching the calendar
        while let Some(newer_events) = &newer_entry_sentinel {
            let r = caller::make_get_request(
                lolesports::SCHEDULE_ENDPOINT,
                Some(&[("pageToken", newer_events)]),
            )
            .await
            .with_context(|| "A failure happened retrieving the schedule from Lolesports");

            serde_json::from_str::<Wrapper<ScheduleOutter>>(&r?.text().await.unwrap())
                .map(|parsed| {
                    total_new_entries += 1;
                    println!("Requesting pages: {:?}", &parsed.data.schedule.pages);
                    println!("Total new entries fetched: {:?}", &total_new_entries);
                    newer_entry_sentinel = parsed.data.schedule.pages.newer;
                    self.schedule.extend(parsed.data.schedule.events)
                })
                .with_context(|| "A failure happened parsing the Schedule from Lolesports")?;
        }

        Ok(())
    }

    fn search_league_by_name(&self, name: &str) -> LolesportsId {
        self.leagues
            .leagues
            .iter()
            .find(|league| (*league.name).eq(name))
            .map(|league| league.id)
            .unwrap_or_default()
    }

    pub async fn fetch_live(&mut self) -> Result<()> {
        self.recent_ended_events_match.clear();
        self.recent_ended_events_show.clear();

        let response = caller::make_get_request::<&[()]>(lolesports::LIVE_ENDPOINT, None)
            .await
            .with_context(|| "A failure happened retrieving the Live Events from Lolesports");

        serde_json::from_str::<Wrapper<ScheduleOutter>>(&response?.text().await.unwrap())
            .map(|parsed| {
                println!(
                    "Eventos sin match {:?} ",
                    parsed
                        .data
                        .schedule
                        .events
                        .iter()
                        .filter(|e| e.r#match.is_none())
                        .collect::<Vec<&Event>>()
                );
                self.live = parsed.data.schedule.events;
            })
            .with_context(|| "A failure happened parsing the Live Events from Lolesports")
    }

    pub async fn fetch_recent_ended_events(&mut self) -> Result<()> {
        let ended_events = &self
            .previous_live
            .iter()
            .filter(|event| !self.live.contains(event))
            .cloned()
            .collect::<Vec<Event>>();

        self.previous_live.clone_from(&self.live);

        for ended_event in ended_events {
            match ended_event.r#match {
                Some(event_match) => {
                    let response = caller::make_get_request::<&[()]>(
                        lolesports::EVENT_DETAILS_ENDPOINT,
                        Some(&[("id", event_match.id.into())]),
                    )
                    .await
                    .with_context(|| {
                        "A failure happened retrieving an Ended Event from Lolesports"
                    });

                    serde_json::from_str::<Wrapper<EventOutter>>(&response?.text().await.unwrap())
                        .map(|parsed| {
                            self.recent_ended_events_match.push(parsed.data.event);
                        })
                        .with_context(|| {
                            "A failure happened parsing an ended Event from Lolesports"
                        });
                    Ok(());
                }
                None => {
                    ended_event.state = "completed".to_owned();
                    self.recent_ended_events_show.push(ended_event.to_owned());
                    Ok(());
                }
            }
        }

        Ok(())
    }

    pub async fn fetch_ended_game_test(&mut self) -> Result<()> {
        let r = caller::make_get_request(
            lolesports::EVENT_DETAILS_ENDPOINT,
            Some(&[("id", 110056852358713598 as i64)]),
        )
        .await
        .with_context(|| "A failure happened retrieving the schedule from Lolesports");

        let result = serde_json::from_str::<Wrapper<EventOutter>>(&r?.text().await.unwrap())
            .map(|parsed| {
                println!("Event parseado {:?}", parsed.data);
            })
            .with_context(|| "A failure happened parsing the EventDetails from Lolesports")?;
        Ok(())
    }
}
