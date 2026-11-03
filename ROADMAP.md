# Roadmap

Things we want to do. Priority: High / Medium / Low.

## API migration
See [ADR 0003](docs/decisions/0003-migrate-to-lolesports-graphql-api.md) (proposed) and the [migration map](docs/investigations/2026-10-16-lolesports-api-migration-map.md).
- [ ] [High] Close the [ADR 0003 open points](docs/decisions/0003-migrate-to-lolesports-graphql-api.md#open-before-accepting) (Rust spike, hash refresh, tier 2/3, `alt_image_url`, TBD, shows)
- [x] [High] Leagues and tournaments from `GetOtherLeagues`, `TRIFORCE_SOURCE_LEAGUES=new` by default
- [ ] [Medium] Teams and players from `GetLoLTeamOverview`: `TRIFORCE_SOURCE_TEAMS=new` by default. Left: link `homeLeague.id` instead of the name lookup
- [ ] [Medium] Schedule from `scheduleEvents`, use league and team ids instead of slug/name lookups
- [x] [Medium] Keep the query manifest up to date and alert when a hash stops working
- [ ] [Low] Live and event details from `watchLiveQuery` + `watchVodQuery` (checked live on 2026-10-16), the adapter sets `state` itself

## Schema v2
See [ADR 0004](docs/decisions/0004-schema-v2-reference-tables.md) and [ADR 0005](docs/decisions/0005-v2-matches-and-game-details.md) (proposed).
- [x] [High] v2 tables for matches: the match, its games with the blue and red team of each game, and the streams (`scheduleEvents` + live feed, ADR 0005)
- [x] [High] Live loop for v2: `watchLiveQuery` + the feed while a game is played (`TRIFORCE_CRON_V2_LIVE`, ADR 0005). Score and state within a minute, winner of each game (only known when watched live, sweeps aside), streams, sides before the game (`side_source = 'api'`), start/end and team totals from the feed
- [ ] [Medium] Move WSC-Triforce to the v2 tables ([notes](docs/investigations/2026-10-17-wsc-triforce-on-schema-v2.md)), then the app
- [ ] [Medium] Roster history (who played where and when) and the teams that played each tournament, maybe from another API (`GetSeasonTournament.teams` only has them for 263 of 514 tournaments)
- [x] [Medium] In-game stats per game from the live feed: final stats (`v2_game_team`, `v2_game_player`) and every frame (archive + 10 s series and markers, live and after the game, [ADR 0007](docs/decisions/0007-v2-game-frames.md))
- [ ] [Medium] Faster frames backfill: one game takes ~4 min (500 calls, one game at a time), the whole history ~2 months. Walk a few games at once, with a limit on calls per second
- [ ] [Low] Live frames: each tick rewrites every series array of the game (WAL grows with the square of the game length). Write every N ticks if it shows in WAL or autovacuum
- [x] [Medium] Stage names in `v2_stage_i18n` from `GetSeasonTournament` (every locale, 100 tournaments per call)
- [ ] [Low] Stage sections (groups, brackets) and their standings from `GetSeasonStage`, with the match tables
- [ ] [Low] Games, team slots and block names the API stops sending stay in v2 (a Bo5 changed to a Bo3 keeps games 4 and 5). Remove them without breaking the game details that point at them
- [ ] [Low] Split names: the API mostly sends ids (`split_2`), find where the site gets the names
- [ ] [Low] A source for `light_image_url` (`lightImage` comes with the events)
- [ ] [Low] `alt_image_url` comes from the old API: decide what to do when it goes away
- [ ] [Low] Rename `v2_*` when v1 goes away. If Canyon's rename doesn't work, a SQL script and an issue to Canyon-SQL

## Reliability
- [ ] [Medium] v2: API corrections that lower a result are skipped today (an answer with a lower state, score or outcome is taken as an old copy). Design how to accept a real one ([live match state](docs/investigations/2026-10-16-live-match-state-design.md) rule 8)
- [x] [High] v2 game details after a chronobreak: the end time and the final stats are the frame before the pause (G2-MKOI 2025-09-14 game 3 stored 17:30:56 and kills 5-6, the game went on to 17:51:50). After the restore the feed's counters (kills, towers, dragons, KDA, wards, skills) start again at 0 and the items list only has new buys
- [ ] [Low] v2 game details: if the feed goes away for good each game burns its 5 tries (~14 h). Notice it and stop the loop (`TRIFORCE_V2_DETAILS=off` does it by hand)
- [ ] [Low] v2: a Postgres deadlock between the sweep results and a game details write fails one of them. Retry it without counting a try
- [ ] [Low] v2: `vod_sql` writes `v2_game_team` before it updates `v2_game`, the live writer locks `v2_game` first. Lock `v2_game` first in `vod_sql` too (a deadlock costs one try)
- [ ] [Low] Fall back to the old API by itself when a v1 GraphQL fetch fails, per run (today only by hand with `TRIFORCE_SOURCE_*=old`). `keep_alt_image` follows `teams_source`, check it
- [ ] [Low] v1 start up steps log and go on instead of `?`, which stops the app and the v2 loops already running
- [x] [High] Refresh `team_player` in one go: new pairs added, pairs not sent removed, duplicates removed, in one SQL statement. Unknown teams/players are skipped with a log
- [x] [High] Replace `tournaments`, `teams` and `players` on every fetch instead of adding to them
- [ ] [Medium] Stop ignoring DB errors (`let _ = …`, unused `Result`s in `main.rs`)
- [ ] [Medium] Don't hold the `DataPull` lock during HTTP fetches and DB writes (a GraphQL manifest refresh also runs under it)
- [ ] [Medium] Ignore API answers older than the last one we got (keep the time each answer was made). The old `getLive` went `completed` → `inProgress` → `completed` during a live game ([live check](docs/investigations/2026-10-17-old-vs-new-api-comparison.md#the-old-api-went-back))
- [ ] [Medium] A failed `getEventDetails` drops the rest of the batch without a log: update `previous_live` after the loop and log the error ([migration map](docs/investigations/2026-10-16-lolesports-api-migration-map.md#how-triforce-decides-a-match-ended))
- [ ] [Medium] The live match found at start up is never saved: `fetch_change_in_events` runs at start without `bulk_eventdetails_in_database`, and the next tick sees no change, so the match and its streams aren't stored until something changes
- [ ] [Medium] No panics in the scheduled loops (`text().await.unwrap()`, `Deserialize` impls that panic, `leagues[0]`). Run each v1 pass in its own task like v2 does, so a panic skips one run instead of stopping the loop
- [ ] [Low] Retry policy for the old API: transport errors, 5xx and 429 with backoff, `error_for_status` for the rest
- [ ] [Medium] One queue loop for the game details and the game frames (today two copies): the frames one already pauses on feed outages without counting a try and waits between games, the details one should too. Feed pace set from env and `Retry-After` on 429
- [ ] [Low] Shut down cleanly on ctrl-c: let in-flight writes finish (today it can stop between two `team_player` chunks, or halfway through the v1 per-row upserts)
- [x] [Low] Don't delete `team_player` when the team list is empty (an answer without players changes nothing, teams not in the answer keep their pairs)
- [ ] [Low] GraphQL client: retry a timeout while reading the body. Cap the size of the old API answers too (GraphQL, manifest and `getTeams` are capped at 32 MB)
- [ ] [Low] Manifest refresh: one deadline for the whole refresh, retry a chunk that timed out once

## Data layer
- [ ] [Medium] Schedule teams are found by name and 24 matches have the wrong team (49 names shared by 104 teams). With the new API use `teamId`, before that match by name + code + image ([comparison](docs/investigations/2026-10-17-old-vs-new-api-comparison.md#schedule-teams-wrong-team-by-name))
- [ ] [Medium] Copy live wins by team id, not by position in the array (`src/dao/models/event.rs`). FUR-C9 (`115660588583171794`) is stored C9 left, FUR right, and every API sends FUR first ([comparison](docs/investigations/2026-10-17-old-vs-new-api-comparison.md#schedule-teams-wrong-team-by-name))
- [ ] [Low] Remove duplicated rows in `canyon_memory`
- [ ] [Low] Add the foreign key to `schedule.team_right_id` (only `team_left_id` has one). Needed before removing the TBD teams
- [ ] [Low] Save the time each API answer was made (column or `schedule_change` table) for the P3 corrections. Canyon migration + rebuild
- [ ] [Low] Live change detection (`EventDetails::eq`) misses added or removed games and rescheduled shows
- [ ] [Low] Skip and log tournaments whose league isn't stored, instead of saving them with `league = 0`
- [ ] [Low] Store why a game is `no_feed` (a 204 or a feed without 5 players a side), so a reset can pick one kind
- [ ] [Low] Unique `(team_id, player_id)` index on `team_player` and `ON CONFLICT DO NOTHING` in the sync. Today only the `DatabaseOps` lock stops two syncs at once (one process)

## Dependencies
- [ ] [Low] Report Canyon-SQL issues upstream ([0.5.1 cfg bug](docs/investigations/2026-10-16-canyon-0.5.1-cfg-bug.md), [book vs code](docs/investigations/2026-10-16-canyon-main-vs-develop.md), config lookup takes the first `canyon*.toml` it finds, [features Triforce needs](docs/investigations/2026-10-17-canyon-features-for-triforce.md)). Ask for multi-row insert in the query builder.
- [ ] [High] Move the Canyon `rev` off `cc563d7` (branch `fix/typed-fk-old-migrations`) to `develop` once the fix is merged, or to the migrations refactor, before that branch is deleted ([ADR 0006](docs/decisions/0006-canyon-sql-develop-with-typed-fk-fix.md))
- [ ] [Low] Keep a fallback for the Canyon `rev` pin (fork with a tag, or `cargo vendor`) in case upstream rewrites or deletes the pinned commit ([ADR 0006](docs/decisions/0006-canyon-sql-develop-with-typed-fk-fix.md))
- [ ] [Low] Move from the `develop` commit to a Canyon-SQL release when one has the `Crud`/`Read`/`Insert` API

## Security
- [ ] [Low] Bind the dev Postgres port to `127.0.0.1`. Today `docker/docker-compose.yml` publishes `5438` on all interfaces with `postgres`/`postgres`.
- [ ] [Low] Take the Postgres user, password and port in `docker/docker-compose.yml` from env (a file in `env/`, not in git, like `env/docker.env`), with the dev values as defaults

## Quality & tests
- [ ] [Low] Long checks when v2 is complete: a feed outage of 20+ min (games keep their tries), 48 h with the default crons (the full pass every ~24 h, not 24 h 30), a start on an empty DB (matches and details wait for the reference loop, no deadlock)
- [ ] [Medium] Tests for API parsing. Start with a bad id or date (today it panics) and a rescheduled show (`EventDetails::eq`)
- [ ] [Low] Fix the remaining clippy warnings
- [ ] [Low] Base URL for the GraphQL client and the manifest download set from tests, to test unknown hash → refresh → retry without the network
- [ ] [Low] `parse_body` without the full `serde_json::Value` tree (`RawValue`), the big answers are parsed twice
- [ ] [Low] One chunk-by-cost helper with a boundary test for `team_player_sync.rs`, `v2/reference.rs` and `v2/events.rs`
- [ ] [Low] Shared `#[cfg(test)]` DB helpers (lock, `count`, `exec`) for the v1 and v2 tests

## Observability & config
- [ ] [Low] Use `tracing` instead of `println!`
- [ ] [Low] Read the API key from config/env
- [ ] [Low] `refresh-manifest` without Canyon start and migrations (its own binary)
