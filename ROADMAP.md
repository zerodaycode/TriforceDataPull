# Roadmap

Things we want to do. Priority: High / Medium / Low.

## Reliability
- [ ] [High] Refresh `team_player` in one go. Today we delete all rows, then `.expect()` and `todo!()` can panic, which leaves the table empty and stops the hourly job for good. Build the rows first (skip unknown teams/players with a log), then delete and insert in one SQL statement with `DbConnection::execute`. Canyon's query builder only inserts one row and has no transactions, see [investigation](docs/investigations/2026-10-16-canyon-main-vs-develop.md).
- [ ] [Medium] Stop ignoring DB errors (`let _ = …`, unused `Result`s in `main.rs`)
- [ ] [Medium] Don't hold the `DataPull` lock during HTTP fetches and DB writes

## Data layer
- [ ] [Low] Remove duplicated rows in `canyon_memory`

## Dependencies
- [ ] [Low] Report Canyon-SQL issues upstream ([0.5.1 cfg bug](docs/investigations/2026-10-16-canyon-0.5.1-cfg-bug.md), [`crate::canyon_sql` paths](docs/decisions/0002-extern-crate-canyon-sql.md), [book vs code](docs/investigations/2026-10-16-canyon-main-vs-develop.md), config lookup takes the first `canyon*.toml` it finds). Ask for multi-row insert in the query builder.
- [ ] [Low] Keep a fallback for the Canyon `rev` pin (fork with a tag, or `cargo vendor`) in case upstream rewrites `develop` ([ADR 0001](docs/decisions/0001-use-canyon-sql-develop-branch.md))
- [ ] [Low] Move from the `develop` commit to a Canyon-SQL release when one has the `Crud`/`Read`/`Insert` API

## Security
- [ ] [Low] Bind the dev Postgres port to `127.0.0.1`. Today `docker/docker-compose.yml` publishes `5438` on all interfaces with `postgres`/`postgres`.

## Quality & tests
- [ ] [Medium] Tests for API parsing and cron schedules
- [ ] [Low] Fix the remaining clippy warnings

## Observability & config
- [ ] [Low] Use `tracing` instead of `println!`
- [ ] [Low] Read the API key from config/env
