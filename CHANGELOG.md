# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- `canyon.toml.example` template, copy it to `canyon.toml`.
- README, roadmap, development workflow, decisions and investigations in `docs/`.
- Investigations of the old LoL Esports API, the live feed and the new lolesports.com GraphQL API, with a migration map, a live check and a data comparison between both APIs, and [ADR 0003](docs/decisions/0003-migrate-to-lolesports-graphql-api.md) (proposed).
- `tools/lolesports-explorer/`: Python tools to call and record both APIs, the query manifest and a Bruno collection.
- Leagues and tournaments can come from the lolesports.com GraphQL API (`TRIFORCE_SOURCE_LEAGUES=new`), one call instead of 52.
- Teams and players can come from the lolesports.com GraphQL API (`TRIFORCE_SOURCE_TEAMS=new`). The stored alternative image is kept, the new API doesn't have it.
- GraphQL query manifest kept in `gql-manifest.json`, downloaded again when a query hash stops working, or with `cargo run -- refresh-manifest`.
- v2 reference tables (`v2_*`), created by the Canyon migrations and filled from the new API at start and every 2 hours. On by default, `TRIFORCE_V2=off` stops the filling and the startup SQL. Names in `en-US` plus `TRIFORCE_LOCALES` (like `es-ES,ko-KR`). Unique indexes and checks Canyon can't create are in `src/dao/v2/schema.sql`, the app doesn't start if they fail. Stage names in every locale come from `GetSeasonTournament`. Alternative team images come from the old API. Player photos that are only a placeholder are stored as `NULL`. See [ADR 0004](docs/decisions/0004-schema-v2-reference-tables.md) (proposed).
- `TRIFORCE_CRON_<LOOP>` to change the schedule of a loop, like `TRIFORCE_CRON_V2="0 * * ? * *"` to run v2 every minute when testing.

### Changed
- Rust edition 2021 → 2024.
- New dependency: sha2 0.11.1, to check the manifest hashes.
- reqwest `gzip` and `brotli` features: API answers come compressed (a 100 tournaments `GetSeasonTournament` answer goes from 1.2 MB to 193 KB with gzip, 52 KB with brotli, the CDN sends brotli). GraphQL, manifest and `getTeams` answers are read up to 32 MB.
- Dependencies updated: serde 1.0.229, serde_json 1.0.154, reqwest 0.13.5, color-eyre 0.6.5, itertools 0.15.0, chrono 0.4.45, cron 0.17.0, tokio 1.53.3.
- Canyon-SQL 0.4.2 → `develop` branch, pinned to commit `1c81b3b` with `rev`. See [ADR 0001](docs/decisions/0001-use-canyon-sql-develop-branch.md).
- Data access ported to the new Canyon API (`Crud` derive, `Read`/`Insert`/`Update`/`Delete` traits, query builder with `where_value`).
- `team_player` rows are inserted one by one (Canyon removed `multi_insert`).
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
- Cron schedules rejected by cron 0.17 (`*/120` and `*/90` minute steps). Leagues and tournaments now really run every 2 hours (before they ran every hour). Teams and players run every hour.
- reqwest `query` feature enabled, it's opt-in since 0.13.
- `team_player` delete errors are logged instead of ignored. The inserts still run after a failed delete, see [ROADMAP](ROADMAP.md).
