mod dao;
mod data_pull;
mod service;
mod utils;

use std::str::FromStr;
use std::sync::Arc;
use canyon_sql::date_time::Utc;
use chrono::Local;
use cron::Schedule;
use tokio::time::{sleep, Duration};

use color_eyre::Result;
use tokio::{select, task};
use tokio::sync::Mutex;

#[canyon_sql::main()]
fn main() -> Result<()> {
    let data_pull = Arc::new(Mutex::new(service::DataPull::default()));
    let database_ops = Arc::new(Mutex::new(dao::DatabaseOps::default()));

    

    println!("{} - Initial league fetch", Local::now().format("%Y-%m-%d %H:%M:%S.%f"));
    // Processing the leagues
    data_pull.lock().await.fetch_leagues().await?;

    println!("{} - Initial league database update", Local::now().format("%Y-%m-%d %H:%M:%S.%f"));
    database_ops.lock().await
        .bulk_leagues_in_database(&data_pull.lock().await.leagues)
        .await?;


    // println!("{} - Initial tournaments fetch", Local::now().format("%Y-%m-%d %H:%M:%S.%f"));
    // // Processing the tournaments
    // data_pull.lock().await.fetch_tournaments().await?;
    // database_ops.lock().await
    //     .bulk_tournaments_in_database(&data_pull.lock().await.tournaments)
    //     .await?;

    // println!("{} - Initial teams and players fetch", Local::now().format("%Y-%m-%d %H:%M:%S.%f"));
    // // Processing the teams and players
    // data_pull.lock().await.fetch_teams_and_players().await?;
    // database_ops.lock().await
    //     .bulk_teams_in_database(&data_pull.lock().await.teams)
    //     .await?;
    // database_ops.lock().await
    //     .bulk_players_in_database(&data_pull.lock().await.players)
    //     .await?;
    // database_ops.lock().await
    //     .bulk_team_player_in_database(&data_pull.lock().await.teams)
    //     .await?;


    // println!("{} - Initial schedule fetch", Local::now().format("%Y-%m-%d %H:%M:%S.%f"));
    // // Processing the complete schedule
    // data_pull.lock().await.process_full_schedule().await?;

    // database_ops.lock().await
    // .bulk_schedule_in_database(&data_pull.lock().await.schedule)
    // .await?;

    // println!("{} - Initial data {:?}", Local::now().format("%Y-%m-%d %H:%M:%S.%f"), data_pull);
    //
    // let leagues_schedule = Schedule::from_str("0 0 */2 * *")?; // every 2 day at midnight
    // let tournaments_schedule = Schedule::from_str("0 0 * * * *")?; // every day at midnight
    // let teams_and_players_schedule = Schedule::from_str("0 0 * * * *")?; // every day at midnight
    // let lolschedule_schedule = Schedule::from_str("*/10 * * * *")?; // every 10 minutes


    // let leagues_schedule = Schedule::from_str("0 */30 * ? * *")?; // every 30 minutes
    // let tournaments_schedule = Schedule::from_str("0 */20 * ? * *")?; // every 20 minutes
    // let teams_and_players_schedule = Schedule::from_str("0 */15 * ? * *")?; // every 15 minutes
    // let lolschedule_schedule = Schedule::from_str("0 */5 * ? * *")?; // every 5 minutes




    // // fetch_leagues
    // {
    //     let data_pull = data_pull.clone();
    //     tokio::spawn(async move {
    //         loop {
    //             let now = Utc::now();
    //             if let Some(next) = leagues_schedule.upcoming(Utc).next() {
    //                 let delay = next - now;
    //                 sleep(Duration::from_millis(delay.num_milliseconds() as u64)).await;
    //             }
    //             let _ = data_pull.lock().await.fetch_leagues().await;
    //         }
    //     });
    // }

    // // fetch_tournaments
    // {
    //     let data_pull = data_pull.clone();
    //     tokio::spawn(async move {
    //         loop {
    //             let now = Utc::now();
    //             if let Some(next) = tournaments_schedule.upcoming(Utc).next() {
    //                 let delay = next - now;
    //                 sleep(Duration::from_millis(delay.num_milliseconds() as u64)).await;
    //             }
    //             let _ = data_pull.lock().await.fetch_tournaments().await;

    //         }
    //     });
    // }

    //  // fetch teams and players
    // {
    //     let data_pull = data_pull.clone();
    //     tokio::spawn(async move {
    //         loop {
    //             let now = Utc::now();
    //             if let Some(next) = teams_and_players_schedule.upcoming(Utc).next() {
    //                 let delay = next - now;
    //                 sleep(Duration::from_millis(delay.num_milliseconds() as u64)).await;
    //             }
    //             let _ = data_pull.lock().await.fetch_teams_and_players().await;
    //         }
    //     });
    // }

    //  // fetch schedule (long)
    // // TODO implement the live mechanic to only fetch a portion of the schedule, not the full schedule,
    // // too much request may trigger a shadow ban
    // {
    //     let data_pull = data_pull.clone();
    //     tokio::spawn(async move {
    //         loop {
    //             let now = Utc::now();
    //             if let Some(next) = lolschedule_schedule.upcoming(Utc).next() {
    //                 let delay = next - now;
    //                 sleep(Duration::from_millis(delay.num_milliseconds() as u64)).await;
    //             }
    //             let _ = data_pull.lock().await.process_full_schedule().await;
    //         }
    //     });
    // }

    // // Wait for the tasks to finish
    // tokio::signal::ctrl_c().await?;

    Ok(())
}