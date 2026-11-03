# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- `canyon.toml.example` template, copy it to `canyon.toml`.
- README, roadmap, development workflow, decisions and investigations in `docs/`.
- Investigations of the old LoL Esports API, the live feed and the new lolesports.com GraphQL API, with a migration map, a live check and a data comparison between both APIs, and [ADR 0003](docs/decisions/0003-migrate-to-lolesports-graphql-api.md) (proposed).
- `tools/lolesports-explorer/`: Python tools to call and record both APIs, the query manifest and a Bruno collection.
- `TRIFORCE_SOURCE_LEAGUES` (`new`/`old`): leagues and tournaments from the lolesports.com GraphQL API take one call instead of 52.
- `TRIFORCE_SOURCE_TEAMS` (`new`/`old`): teams and players from the lolesports.com GraphQL API. The stored alternative image is kept, the new API doesn't have it.
- GraphQL query manifest kept in `gql-manifest.json`, downloaded again when a query hash stops working, or with `cargo run -- refresh-manifest`.
- v2 reference tables (`v2_*`), created by the Canyon migrations and filled from the new API at start and every 2 hours. On by default, `TRIFORCE_V2=off` stops the filling and the startup SQL. Names in `en-US` plus `TRIFORCE_LOCALES` (like `es-ES,ko-KR`). Unique indexes and checks Canyon can't create are in `src/dao/v2/schema.sql`, the app doesn't start if they fail. Stage names in every locale come from `GetSeasonTournament`. Alternative team images come from the old API. Player photos that are only a placeholder are stored as `NULL`. See [ADR 0004](docs/decisions/0004-schema-v2-reference-tables.md) (proposed).
- v2 match tables (`v2_event`, `v2_event_i18n`, `v2_event_team`, `v2_game`, `v2_stream`) from `scheduleEvents`: the last 3 days and the next year every 30 minutes (`TRIFORCE_CRON_V2_MATCHES`), the whole history from 2023-10 at most once a day. Teams by id, TBD as `NULL`, an answer with a lower state, score or outcome than what we have is skipped. See [ADR 0005](docs/decisions/0005-v2-matches-and-game-details.md) (proposed).
- v2 game details (`v2_game_team`, `v2_game_player`): for every finished game since 2023-10, the side of each team, who played (champion and role of that game) and the final stats (last frame of the live feed: team gold, kills, towers, dragons, barons; player KDA, gold, CS, items, runes, skill order, damage share, wards). Sides from `watchVodQuery` when the feed has nothing, or still doesn't have 5 players on each side on the second try (some feeds stay broken). Game winners for sweeps. `v2_game` gets `patch`, `start_time`, `end_time` and the queue state (`details`, `details_tries`, `details_next_try`). About 1 game per second, the backfill (~23k games) takes about 6.5 h of run time and can be stopped and started any time. A failed game waits longer after each try and leaves the queue after 5 (README has the reset). `TRIFORCE_V2_DETAILS=off` stops only the game details. When the feed has a team the match doesn't list (a spot that changed hands), it is stored under the match's other team and logged.
- v2 live matches from `watchLiveQuery` once a minute (`TRIFORCE_CRON_V2_LIVE`, `TRIFORCE_V2_LIVE=off` stops it): state, score, games, streams and the side of each game as soon as it's picked. The winner of a game is stored when the score changes by one win, also from the matches loop. For the game being played, the live feed gives the real start and end time and each team's totals (gold, kills, towers, inhibitors, barons, dragons) every minute. A match that leaves the live answer is marked `completed` from the schedule within a few minutes, not on the next 30 min tick. Live sides use the match's teams, and the ones of a game that ends up `unneeded` are removed. See [ADR 0005](docs/decisions/0005-v2-matches-and-game-details.md).
- `--env <file>` to read the settings from a file, `env/example.env` lists them all with their defaults. Shell variables win over the file. The app stops if the file isn't there.
- `TRIFORCE_CRON_<LOOP>` to change the schedule of a loop, like `TRIFORCE_CRON_V2="0 * * ? * *"` to run v2 every minute when testing.
- v2 game frames: every frame of the live feed for each finished game, kept compressed in `v2_game_frames` (~233 KB a game), and a point every 10 s of game time for each team and player in `v2_game_team_series` and `v2_game_player_series` (gold, KDA, CS, items, stats), plus dragons, barons, towers and inhibitors in `v2_game_marker` and every pause in `v2_game_pause`. Chronobreaks: the series follow the game as it went on, what was taken back is kept (`rollback_s`, `v2_game_rollback_team`, `v2_game_rollback_player`, undone markers). Paused time doesn't count. While a game is played the series fill every 10 s, 60-90 s behind the game; the archive replaces them after the game. `TRIFORCE_V2_FRAMES=off` and `TRIFORCE_V2_FRAMES_LIVE=off` stop them, `cargo run -- rebuild-timeline --all` builds the series again from the archive. See [ADR 0007](docs/decisions/0007-v2-game-frames.md) (proposed) and [docs/data/game-frames.md](docs/data/game-frames.md).
- `TRIFORCE_BACKFILL_MATCHES`, `TRIFORCE_BACKFILL_DETAILS` (on) and `TRIFORCE_BACKFILL_FRAMES` (off): jobs that go through the history do only the last 3 days when off.
- `docs/data/`: what every `v2_*` table has, how it's filled and how the tables relate.

### Changed
- Leagues, tournaments, teams and players come from the lolesports.com GraphQL API by default. `TRIFORCE_SOURCE_LEAGUES=old` and `TRIFORCE_SOURCE_TEAMS=old` go back to the old API. With `new`, `team.alt_image_url` keeps the stored value and new teams get none. The first start downloads the query manifest, and the app stops if GraphQL fails at start. `TRIFORCE_V2=off` alone no longer stops the GraphQL calls, set both sources to `old` too.
- Test data moved from `src/**/testdata/` to `tests/fixtures/`.
- Rust edition 2021 → 2024.
- New dependency: sha2 0.11.1, to check the manifest hashes.
- New dependency: dotenvy 0.15.7, to read the `--env` file.
- New dependency: zstd 0.13.4, to compress the game frames archive. It builds libzstd 1.5.7 from source: the build needs a C compiler.
- serde_json `raw_value` feature: the archive keeps the frames byte for byte.
- reqwest `gzip` and `brotli` features: API answers come compressed (a 100 tournaments `GetSeasonTournament` answer goes from 1.2 MB to 193 KB with gzip, 52 KB with brotli, the CDN sends brotli). GraphQL, manifest and `getTeams` answers are read up to 32 MB.
- Dependencies updated: serde 1.0.229, serde_json 1.0.154, reqwest 0.13.5, color-eyre 0.6.5, itertools 0.15.0, chrono 0.4.45, cron 0.17.0, tokio 1.53.3.
- Canyon-SQL 0.4.2 → `develop` branch with the typed foreign key fix, pinned to commit `cc563d7` with `rev`. See [ADR 0001](docs/decisions/0001-use-canyon-sql-develop-branch.md) and [ADR 0006](docs/decisions/0006-canyon-sql-develop-with-typed-fk-fix.md).
- Data access ported to the new Canyon API (`Crud` derive, `Read`/`Insert`/`Update`/`Delete` traits, query builder with `where_value`).
- `team_player` is synced with the fetched rosters instead of deleted and inserted again: new pairs added, pairs no longer sent and duplicated rows removed, all in one SQL statement per chunk of teams. Teams not in the answer keep their pairs, an answer without players changes nothing, and a team or player we don't have is skipped and logged (up to 20 per run) instead of stopping the job. A failed sync is logged and the app keeps running.
- PostgreSQL docker image 15.2 → 18.6 (alpine 3.24). Volume mount moved to `/var/lib/postgresql`, required by 18+. Old 15.x data folders can't be reused. Dump the data **before** upgrading, with the old compose file still running 15.2:
  ```sh
  docker compose -f docker/docker-compose.yml exec -T postgres pg_dumpall -U postgres > dump.sql
  ```
  Then move `docker/postgres-data` away, start 18.6 and restore:
  ```sh
  docker compose -f docker/docker-compose.yml exec -T postgres psql -U postgres < dump.sql
  ```
- One HTTP client for all LoL Esports calls instead of one per request. Connections are reused and HTTP/2 pings drop the dead ones.
- Old API code moved to `src/client/legacy/` (`data_pull` and `utils` are gone).

### Fixed
- v2 game details of a game with a chronobreak: the box score comes from the game's frames instead of the frame before the pause.
  - A feed that ends paused leaves the game in a new `v2_game.details` value, `wait_frames` (sides and lineups, no numbers), until the frames job writes the box score. The end `v2 live` stored is kept meanwhile.
  - A game already `done` whose end is more than a minute before the end of its frames (a long pause, the game went on) gets the box score from the frames too.
  - Games stored before get fixed by the frames job, or from the archive with `cargo run -- rebuild-timeline --all`.
  - Before running an older build (its CHECK refuses `wait_frames`): `UPDATE v2_game SET details = NULL, details_tries = NULL, details_next_try = NULL WHERE details = 'wait_frames';`
- The app stopped at start when a live match had a VOD without `firstFrameTime` (`null`) in the old `getLive`.
- Cron schedules rejected by cron 0.17 (`*/120` and `*/90` minute steps). Leagues and tournaments now really run every 2 hours (before they ran every hour). Teams and players run every hour.
- reqwest `query` feature enabled, it's opt-in since 0.13.
- Each teams run added one more copy of every `team_player` pair (16,308 rows for 5,436 pairs on the dev DB). The first sync removes the copies.
- With the old API, tournaments, teams and players were added to the fetched lists on every run instead of replacing them, so player changes after start up were never written.
