# Changelog

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- `canyon.example.toml` template, copy it to `canyon.toml`.
- README, roadmap, development workflow, decisions and investigations in `docs/`.

### Changed
- Rust edition 2021 → 2024.
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

### Fixed
- Cron schedules rejected by cron 0.17 (`*/120` and `*/90` minute steps). Leagues and tournaments now really run every 2 hours (before they ran every hour). Teams and players run every hour.
- reqwest `query` feature enabled, it's opt-in since 0.13.
- `team_player` delete errors are logged instead of ignored. The inserts still run after a failed delete, see [ROADMAP](ROADMAP.md).
