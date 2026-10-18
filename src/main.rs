extern crate canyon_sql;

mod client;
mod dao;
mod env;
mod schedules;
mod service;

use canyon_sql::date_time::Utc;
use chrono::Local;
use cron::Schedule;
use std::sync::Arc;
use tokio::time::{Duration, sleep};

use color_eyre::Result;
use tokio::sync::Mutex;

/// Sleep until the next tick of `schedule`
async fn sleep_until_next(schedule: &Schedule) {
    let Some(next) = schedule.upcoming(Utc).next() else {
        // no tick left (schedules rejects these at start): this loop never runs again
        return std::future::pending().await;
    };
    // the wall clock can jump under a monotonic sleep (suspend, NTP step): check it every minute
    while let Ok(left) = (next - Utc::now()).to_std()
        && !left.is_zero()
    {
        sleep(left.min(Duration::from_secs(60))).await;
    }
}

#[canyon_sql::main]
fn main() -> Result<()> {
    // `triforce-data-pull [--env <file>] [refresh-manifest]`
    let cli = env::parse_args(std::env::args().skip(1))?;
    if let Some(path) = &cli.env_file {
        env::load(path)?;
    }
    // download the site's query manifest and exit
    if cli.refresh_manifest {
        let store = client::gql::manifest::Store::open(client::gql::manifest::path_from_env());
        store.refresh(client::http()).await?;
        return Ok(());
    }

    let v2 = service::v2::v2_enabled()?;
    let schedules::Schedules {
        v2: v2_reference_schedule,
        leagues: leagues_schedule,
        tournaments: tournaments_schedule,
        teams: teams_and_players_schedule,
        schedule: lolschedule_current_page,
        live: live_schedule,
    } = schedules::all_from_env()?;
    client::log(format!("v2 tables: {}", if v2 { "on" } else { "off" }));
    // one GraphQL client for the v1 sources set to new and the v2 loop
    let v1_new = client::Source::from_env("TRIFORCE_SOURCE_LEAGUES")? == client::Source::New
        || client::Source::from_env("TRIFORCE_SOURCE_TEAMS")? == client::Source::New;
    let gql = (v2 || v1_new).then(|| {
        Arc::new(client::gql::Gql::new(
            client::http(),
            client::gql::manifest::Store::open(client::gql::manifest::path_from_env()),
        ))
    });

    if v2 {
        // unique indexes and checks the v2 upserts need: no start without them
        dao::v2::schema::apply().await?;
    }

    // Loop for v2 reference tables, no DataPull or DatabaseOps lock. Spawned first so a slow
    // or failing v1 startup fetch doesn't delay it
    // with TRIFORCE_SOURCE_* = new, v1 and v2 send the same queries: accepted while v1 lives (ADR 0004)
    if v2 && let Some(gql) = gql.clone() {
        let locales = Arc::new(service::v2::locales_from_env());
        tokio::spawn(async move {
            loop {
                let (gql, locales) = (gql.clone(), locales.clone());
                // one task per run: a panic skips this run, not the loop
                let run =
                    tokio::spawn(async move { service::v2::reference_once(&gql, &locales).await });
                match run.await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        client::log(format!("v2 reference failed, skipped this run: {e:#}"))
                    }
                    Err(e) => client::log(format!("v2 reference stopped, skipped this run: {e}")),
                }
                sleep_until_next(&v2_reference_schedule).await;
            }
        });
    }

    let data_pull = Arc::new(Mutex::new(service::DataPull::new(gql.clone())?));
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

    // Loop for leagues
    {
        let data_pull = data_pull.clone();
        let database_ops = database_ops.clone();
        tokio::spawn(async move {
            loop {
                sleep_until_next(&leagues_schedule).await;
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
                sleep_until_next(&tournaments_schedule).await;
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
                sleep_until_next(&teams_and_players_schedule).await;
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
                sleep_until_next(&lolschedule_current_page).await;
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
                sleep_until_next(&live_schedule).await;
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
