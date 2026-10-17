# Roadmap

Things we want to do. Priority: High / Medium / Low.

## API migration
See [ADR 0003](docs/decisions/0003-migrate-to-lolesports-graphql-api.md) (proposed) and the [migration map](docs/investigations/2026-10-16-lolesports-api-migration-map.md).
- [ ] [High] Close the [ADR 0003 open points](docs/decisions/0003-migrate-to-lolesports-graphql-api.md#open-before-accepting) (Rust spike, hash refresh, tier 2/3, `alt_image_url`, TBD, shows)
- [ ] [High] Leagues and tournaments from `GetOtherLeagues` (one call instead of 51), tournament dates = UTC date of `startTime`/`endTime`
- [ ] [Medium] Teams and players from `GetLoLTeamOverview`, use `homeLeague.id` instead of the name lookup
- [ ] [Medium] Schedule from `scheduleEvents`, use league and team ids instead of slug/name lookups
- [x] [Medium] Keep the query manifest up to date and alert when a hash stops working
- [ ] [Low] Live and event details from `watchLiveQuery` + `watchVodQuery` (checked live on 2026-10-16), the adapter sets `state` itself

## Reliability
- [ ] [High] Refresh `team_player` in one go. Today we delete all rows, then `.expect()` and `todo!()` can panic, which leaves the table empty and stops the hourly job for good. Build the rows first (skip unknown teams/players with a log), then delete and insert in one SQL statement with `DbConnection::execute`. Canyon's query builder only inserts one row and has no transactions, see [investigation](docs/investigations/2026-10-16-canyon-main-vs-develop.md).
- [ ] [High] Replace `tournaments`, `teams` and `players` on every fetch instead of adding to them. Today each run appends a full copy: every hourly run stores each `team_player` link once more, and player changes after start up are never written (the dedupe keeps the first copy)
- [ ] [Medium] Stop ignoring DB errors (`let _ = …`, unused `Result`s in `main.rs`)
- [ ] [Medium] Don't hold the `DataPull` lock during HTTP fetches and DB writes
- [ ] [Medium] Ignore API answers older than the last one we got (keep the time each answer was made). The old `getLive` went `completed` → `inProgress` → `completed` during a live game ([live check](docs/investigations/2026-10-17-old-vs-new-api-comparison.md#the-old-api-went-back))
- [ ] [Medium] A failed `getEventDetails` drops the rest of the batch without a log: update `previous_live` after the loop and log the error ([migration map](docs/investigations/2026-10-16-lolesports-api-migration-map.md#how-triforce-decides-a-match-ended))
- [ ] [Medium] The live match found at start up is never saved: `fetch_change_in_events` runs at start without `bulk_eventdetails_in_database`, and the next tick sees no change, so the match and its streams aren't stored until something changes
- [ ] [Medium] No panics in the scheduled loops (`text().await.unwrap()`, `Deserialize` impls that panic, `leagues[0]`). Keep the `JoinHandle`s and log when a loop stops
- [ ] [Low] Retry policy for the old API: transport errors, 5xx and 429 with backoff, `error_for_status` for the rest
- [ ] [Low] Shut down cleanly on ctrl-c: let in-flight writes finish (today it can stop between the `team_player` delete and the inserts)

## Data layer
- [ ] [Medium] Schedule teams are found by name and 24 matches have the wrong team (49 names shared by 104 teams). With the new API use `teamId`, before that match by name + code + image ([comparison](docs/investigations/2026-10-17-old-vs-new-api-comparison.md#schedule-teams-wrong-team-by-name))
- [ ] [Medium] Copy live wins by team id, not by position in the array (`src/dao/models/event.rs`). FUR-C9 (`115660588583171794`) is stored C9 left, FUR right, and every API sends FUR first ([comparison](docs/investigations/2026-10-17-old-vs-new-api-comparison.md#schedule-teams-wrong-team-by-name))
- [ ] [Low] Remove duplicated rows in `canyon_memory`
- [ ] [Low] Add the foreign key to `schedule.team_right_id` (only `team_left_id` has one). Needed before removing the TBD teams
- [ ] [Low] Save the time each API answer was made (column or `schedule_change` table) for the P3 corrections. Canyon migration + rebuild
- [ ] [Low] Live change detection (`EventDetails::eq`) misses added or removed games and rescheduled shows

## Dependencies
- [ ] [Low] Report Canyon-SQL issues upstream ([0.5.1 cfg bug](docs/investigations/2026-10-16-canyon-0.5.1-cfg-bug.md), [`crate::canyon_sql` paths](docs/decisions/0002-extern-crate-canyon-sql.md), [book vs code](docs/investigations/2026-10-16-canyon-main-vs-develop.md), config lookup takes the first `canyon*.toml` it finds, [features Triforce needs](docs/investigations/2026-10-17-canyon-features-for-triforce.md)). Ask for multi-row insert in the query builder.
- [ ] [Low] Keep a fallback for the Canyon `rev` pin (fork with a tag, or `cargo vendor`) in case upstream rewrites `develop` ([ADR 0001](docs/decisions/0001-use-canyon-sql-develop-branch.md))
- [ ] [Low] Move from the `develop` commit to a Canyon-SQL release when one has the `Crud`/`Read`/`Insert` API

## Security
- [ ] [Low] Bind the dev Postgres port to `127.0.0.1`. Today `docker/docker-compose.yml` publishes `5438` on all interfaces with `postgres`/`postgres`.

## Quality & tests
- [ ] [Medium] Tests for API parsing and cron schedules. Start with a bad id or date (today it panics) and a rescheduled show (`EventDetails::eq`)
- [ ] [Low] Fix the remaining clippy warnings

## Observability & config
- [ ] [Low] Use `tracing` instead of `println!`
- [ ] [Low] Read the API key from config/env
- [ ] [Low] Send an identifying `User-Agent` from the shared HTTP client (`src/client/mod.rs`)
