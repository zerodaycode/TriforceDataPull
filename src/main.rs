extern crate canyon_sql;

mod client;
mod dao;
mod service;

use canyon_sql::date_time::Utc;
use chrono::Local;
use cron::Schedule;
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{Duration, sleep};

use color_eyre::Result;
use tokio::sync::Mutex;

#[canyon_sql::main]
fn main() -> Result<()> {
    // `triforce-data-pull refresh-manifest`: download the site's query manifest and exit
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("refresh-manifest") => {
            let store = client::gql::manifest::Store::open(client::gql::manifest::path_from_env());
            store.refresh(client::http()).await?;
            return Ok(());
        }
        Some(other) => {
            return Err(color_eyre::eyre::eyre!(
                "unknown command {other:?}, expected refresh-manifest"
            ));
        }
    }

    let data_pull = Arc::new(Mutex::new(service::DataPull::new()?));
    let database_ops = Arc::new(Mutex::new(dao::DatabaseOps::default()));

    println!(
        "{} - Initial league fetch",
        Local::now().format("%Y-%m-%d %H:%M:%S.%f")
    );
    // Processing the leagues
    data_pull.lock().await.fetch_leagues().await?;

    println!(
        "{} - Initial league database update",
        Local::now().format("%Y-%m-%d %H:%M:%S.%f")
    );
    database_ops
        .lock()
        .await
        .bulk_leagues_in_database(&data_pull.lock().await.leagues)
        .await?;

    println!(
        "{} - Initial tournaments fetch",
        Local::now().format("%Y-%m-%d %H:%M:%S.%f")
    );
    // Processing the tournaments
    data_pull.lock().await.fetch_tournaments().await?;
    println!(
        "{} - Initial tournaments database update",
        Local::now().format("%Y-%m-%d %H:%M:%S.%f")
    );
    database_ops
        .lock()
        .await
        .bulk_tournaments_in_database(&data_pull.lock().await.tournaments)
        .await?;

    println!(
        "{} - Initial teams and players fetch",
        Local::now().format("%Y-%m-%d %H:%M:%S.%f")
    );
    data_pull.lock().await.fetch_teams_and_players().await?;
    // Processing the teams and players
    println!(
        "{} - Initial teams and players db update",
        Local::now().format("%Y-%m-%d %H:%M:%S.%f")
    );
    {
        let mut db = database_ops.lock().await;
        let dp = data_pull.lock().await;
        db.bulk_teams_in_database(&dp.teams, dp.teams_source == client::Source::New)
            .await?;
    }
    database_ops
        .lock()
        .await
        .bulk_players_in_database(&data_pull.lock().await.players)
        .await?;
    database_ops
        .lock()
        .await
        .bulk_team_player_in_database(&data_pull.lock().await.teams)
        .await?;

    println!(
        "{} - Initial schedule fetch",
        Local::now().format("%Y-%m-%d %H:%M:%S.%f")
    );
    // Processing the complete schedule
    data_pull.lock().await.process_full_schedule().await?;

    database_ops
        .lock()
        .await
        .bulk_schedule_in_database(&data_pull.lock().await.schedule)
        .await?;

    println!(
        "{} - Live fetch",
        Local::now().format("%Y-%m-%d %H:%M:%S.%f")
    );
    data_pull.lock().await.fetch_live().await?;
    println!(
        "{} - Fetch recent changes in events",
        Local::now().format("%Y-%m-%d %H:%M:%S.%f")
    );
    data_pull.lock().await.fetch_change_in_events().await;

    // println!("{} - Initial live data {:?}", Local::now().format("%Y-%m-%d %H:%M:%S.%f"), data_pull);

    // println!("{} - Initial data {:?}", Local::now().format("%Y-%m-%d %H:%M:%S.%f"), data_pull);
    //
    // let leagues_schedule = Schedule::from_str("0 0 */2 * *")?; // every 2 day at midnight
    // let tournaments_schedule = Schedule::from_str("0 0 * * * *")?; // every day at midnight
    // let teams_and_players_schedule = Schedule::from_str("0 0 * * * *")?; // every day at midnight
    // let lolschedule_schedule = Schedule::from_str("*/10 * * * *")?; // every 10 minutes

    let leagues_schedule = Schedule::from_str("0 0 */2 ? * *")?; // every 2 hours
    let tournaments_schedule = Schedule::from_str("0 0 */2 ? * *")?; // every 2 hours
    let teams_and_players_schedule = Schedule::from_str("0 0 * ? * *")?; // every hour (cron can't express 90 min)
    // let lolschedule_schedule = Schedule::from_str("0 */60 * ? * *")?; // every 1 hour
    let lolschedule_current_page = Schedule::from_str("0 */30 * ? * *")?; // every 30 minutes
    let live_schedule = Schedule::from_str("0 */2 * ? * *")?; // every 2 minutes

    // Loop for leagues
    {
        let data_pull = data_pull.clone();
        let database_ops = database_ops.clone();
        tokio::spawn(async move {
            loop {
                let now = Utc::now();
                if let Some(next) = leagues_schedule.upcoming(Utc).next() {
                    let delay = next - now;
                    sleep(Duration::from_millis(delay.num_milliseconds() as u64)).await;
                }
                if let Err(e) = data_pull.lock().await.fetch_leagues().await {
                    client::log(format!(
                        "Leagues and tournaments fetch failed, skipped this run: {e:#}"
                    ));
                    continue;
                }
                let mut db = database_ops.lock().await;
                let dp = data_pull.lock().await;
                let _ = db.bulk_leagues_in_database(&dp.leagues).await;
                if dp.leagues_source == client::Source::New {
                    // same answer as the leagues, written right after them
                    let _ = db.bulk_tournaments_in_database(&dp.tournaments).await;
                }
            }
        });
    }

    // Loop for tournaments
    {
        let data_pull = data_pull.clone();
        let database_ops = database_ops.clone();
        tokio::spawn(async move {
            loop {
                let now = Utc::now();
                if let Some(next) = tournaments_schedule.upcoming(Utc).next() {
                    let delay = next - now;
                    sleep(Duration::from_millis(delay.num_milliseconds() as u64)).await;
                }
                let _ = data_pull.lock().await.fetch_tournaments().await;
                // under the new API the leagues loop writes them
                if data_pull.lock().await.leagues_source == client::Source::New {
                    continue;
                }

                let _ = database_ops
                    .lock()
                    .await
                    .bulk_tournaments_in_database(&data_pull.lock().await.tournaments)
                    .await;
            }
        });
    }

    // Loop for teams and players
    {
        let data_pull = data_pull.clone();
        let database_ops = database_ops.clone();

        tokio::spawn(async move {
            loop {
                let now = Utc::now();
                if let Some(next) = teams_and_players_schedule.upcoming(Utc).next() {
                    let delay = next - now;
                    sleep(Duration::from_millis(delay.num_milliseconds() as u64)).await;
                }
                if let Err(e) = data_pull.lock().await.fetch_teams_and_players().await {
                    client::log(format!(
                        "Teams and players fetch failed, skipped this run: {e:#}"
                    ));
                    continue;
                }

                {
                    let mut db = database_ops.lock().await;
                    let dp = data_pull.lock().await;
                    let _ = db
                        .bulk_teams_in_database(&dp.teams, dp.teams_source == client::Source::New)
                        .await;
                }
                let _ = database_ops
                    .lock()
                    .await
                    .bulk_players_in_database(&data_pull.lock().await.players)
                    .await;
                let _ = database_ops
                    .lock()
                    .await
                    .bulk_team_player_in_database(&data_pull.lock().await.teams)
                    .await;
            }
        });
    }

    // Loop to normalize schedule
    {
        let data_pull = data_pull.clone();
        let database_ops = database_ops.clone();
        tokio::spawn(async move {
            loop {
                let now = Utc::now();
                if let Some(next) = lolschedule_current_page.upcoming(Utc).next() {
                    let delay = next - now;
                    sleep(Duration::from_millis(delay.num_milliseconds() as u64)).await;
                }
                let _ = data_pull.lock().await.fetch_current_page_schedule().await;

                let _ = database_ops
                    .lock()
                    .await
                    .bulk_schedule_in_database(&data_pull.lock().await.schedule_single_page)
                    .await;
            }
        });
    }

    // Loop for live
    {
        let data_pull = data_pull.clone();
        tokio::spawn(async move {
            loop {
                let now = Utc::now();
                if let Some(next) = live_schedule.upcoming(Utc).next() {
                    let delay = next - now;
                    sleep(Duration::from_millis(delay.num_milliseconds() as u64)).await;
                }
                data_pull.lock().await.fetch_live().await;

                data_pull.lock().await.fetch_change_in_events().await;

                database_ops
                    .lock()
                    .await
                    .bulk_eventdetails_in_database(
                        &data_pull.lock().await.events_with_recent_changes,
                    )
                    .await;
            }
        });
    }
    tokio::signal::ctrl_c().await?;

    Ok(())
}
