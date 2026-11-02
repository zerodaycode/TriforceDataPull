/// A file under `tests/fixtures/`, read at build time. Keep it above the `mod` lines: a
/// `macro_rules!` is only seen by the code after it
#[cfg(test)]
macro_rules! fixture {
    ($path:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/",
            $path
        ))
    };
}

mod backfill;
mod client;
mod dao;
mod env;
mod frames;
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

/// Waits for the first good run of another loop, or 15 min (it's gone or still failing)
async fn wait_other_loop(name: &str, mut w: tokio::sync::watch::Receiver<bool>) {
    let wait = tokio::time::timeout(Duration::from_secs(15 * 60), w.wait_for(|go| *go));
    if !matches!(wait.await, Ok(Ok(_))) {
        client::log(format!(
            "{name}: starting without a good run of the loop it waits for"
        ));
    }
}

/// A v2 loop: first run at start (after `wait_for` says go, or 15 min), then on `schedule`.
/// One task per run: a panic skips this run, not the loop. `ran` gets `true` after a good run
fn spawn_v2_loop<F, Fut>(
    name: &'static str,
    schedule: Schedule,
    gql: Arc<client::gql::Gql>,
    locales: Arc<Vec<String>>,
    wait_for: Option<tokio::sync::watch::Receiver<bool>>,
    ran: Option<tokio::sync::watch::Sender<bool>>,
    once: F,
) where
    F: Fn(Arc<client::gql::Gql>, Arc<Vec<String>>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        if let Some(w) = wait_for {
            wait_other_loop(name, w).await;
        }
        loop {
            let ok = match tokio::spawn(once(gql.clone(), locales.clone())).await {
                Ok(Ok(())) => true,
                Ok(Err(e)) => {
                    client::log(format!("{name} failed, skipped this run: {e:#}"));
                    false
                }
                Err(e) => {
                    client::log(format!("{name} stopped, skipped this run: {e}"));
                    false
                }
            };
            if ok && let Some(ran) = &ran {
                ran.send_replace(true);
            }
            sleep_until_next(&schedule).await;
        }
    });
}

#[canyon_sql::main]
fn main() -> Result<()> {
    // `triforce-data-pull [--env <file>] [refresh-manifest | rebuild-timeline (--all | <game ext id>...)]`
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
    // build the game series again from the archive and exit
    if let Some(which) = &cli.rebuild {
        dao::v2::schema::apply().await?;
        return service::v2_frames::rebuild(which).await;
    }

    let v2 = service::v2::v2_enabled()?;
    let v2_details = service::v2_details::enabled()?;
    let v2_live = service::v2_live::enabled()?;
    let v2_frames = service::v2_frames::enabled()?;
    let v2_frames_live = service::v2_frames_live::enabled()?;
    let schedules::Schedules {
        v2: v2_reference_schedule,
        v2_matches: v2_matches_schedule,
        v2_live: v2_live_schedule,
        leagues: leagues_schedule,
        tournaments: tournaments_schedule,
        teams: teams_and_players_schedule,
        schedule: lolschedule_current_page,
        live: live_schedule,
    } = schedules::all_from_env()?;
    client::log(format!("v2 tables: {}", if v2 { "on" } else { "off" }));
    let backfills = backfill::all_from_env()?;
    client::log(backfill::log_line(&backfills));
    let backfill_matches = backfill::is_on(&backfills, backfill::Backfill::Matches);
    let backfill_details = backfill::is_on(&backfills, backfill::Backfill::Details);
    let backfill_frames = backfill::is_on(&backfills, backfill::Backfill::Frames);
    // both read before anything runs: a bad value stops the start, not a v2 run halfway
    let leagues_source = client::Source::from_env("TRIFORCE_SOURCE_LEAGUES")?;
    let teams_source = client::Source::from_env("TRIFORCE_SOURCE_TEAMS")?;
    // one GraphQL client for the v1 sources set to new and the v2 loop
    let v1_new = leagues_source == client::Source::New || teams_source == client::Source::New;
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

    // v2 loops, no DataPull or DatabaseOps lock. Spawned first so a slow or failing v1 startup
    // fetch doesn't delay them
    // with TRIFORCE_SOURCE_* = new, v1 and v2 send the same queries: accepted while v1 lives (ADR 0004)
    if v2 && let Some(gql) = gql.clone() {
        let locales = Arc::new(service::v2::locales_from_env());
        // matches wait for the first reference run: tournaments, teams and locales come from it
        let (reference_ran, wait_reference) = tokio::sync::watch::channel(false);
        spawn_v2_loop(
            "v2 reference",
            v2_reference_schedule,
            gql.clone(),
            locales.clone(),
            None,
            Some(reference_ran),
            |g, l| async move { service::v2::reference_once(&g, &l).await },
        );
        // game details: drains its queue on its own, after the first good reference run
        if v2_details {
            let gql = gql.clone();
            let wait = wait_reference.clone();
            tokio::spawn(async move {
                wait_other_loop("v2 details", wait).await;
                client::log("v2 details: starting");
                service::v2_details::details_loop(gql, backfill_details).await;
            });
        } else {
            client::log("v2 details: off (TRIFORCE_V2_DETAILS=off)");
        }
        // game frames after the game: its own queue, after the first good reference run
        if v2_frames {
            let wait = wait_reference.clone();
            tokio::spawn(async move {
                wait_other_loop("v2 frames", wait).await;
                client::log("v2 frames: starting");
                service::v2_frames::frames_loop(backfill_frames).await;
            });
        } else {
            client::log("v2 frames: off (TRIFORCE_V2_FRAMES=off)");
        }
        // live series: the games v2 live marks inProgress, every 10 s
        if service::v2_frames_live::live_on(v2_frames, v2_frames_live) {
            let wait = wait_reference.clone();
            tokio::spawn(async move {
                wait_other_loop("v2 frames live", wait).await;
                client::log("v2 frames live: starting (every 10 s, quiet while nothing is live)");
                service::v2_frames_live::live_loop().await;
            });
        } else {
            client::log("v2 frames live: off");
        }
        // live: teams and tournaments come from the reference loop too
        if v2_live {
            client::log("v2 live: on (watchLiveQuery every minute, quiet while nothing is live)");
            spawn_v2_loop(
                "v2 live",
                v2_live_schedule,
                gql.clone(),
                locales.clone(),
                Some(wait_reference.clone()),
                None,
                |g, _| async move { service::v2_live::live_once(&g).await },
            );
        } else {
            client::log("v2 live: off (TRIFORCE_V2_LIVE=off)");
        }
        spawn_v2_loop(
            "v2 matches",
            v2_matches_schedule,
            gql,
            locales,
            Some(wait_reference),
            None,
            move |g, l| async move { service::v2_matches::matches_once(&g, &l, backfill_matches).await },
        );
    }

    let data_pull = Arc::new(Mutex::new(service::DataPull::new(
        gql.clone(),
        leagues_source,
        teams_source,
    )));
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
    if let Err(e) = database_ops
        .lock()
        .await
        .bulk_team_player_in_database(&data_pull.lock().await.teams)
        .await
    {
        client::log(format!(
            "Initial team_player sync failed, the teams loop tries again: {e:#}"
        ));
    }

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
                if let Err(e) = database_ops
                    .lock()
                    .await
                    .bulk_team_player_in_database(&data_pull.lock().await.teams)
                    .await
                {
                    client::log(format!("team_player sync failed: {e:#}"));
                }
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
