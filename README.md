# TriforceDataPull

Pulls LoL Esports data (leagues, tournaments, teams, players, schedule and live events) and keeps it in a PostgreSQL database.

## Requirements

- Rust 1.85+ (edition 2024)
- Docker and Docker Compose

## Setup

Start the database (PostgreSQL 18, port 5438):

```sh
docker compose -f docker/docker-compose.yml up -d
```

Create the Canyon config from the template:

```sh
cp canyon.toml.example canyon.toml
```

`canyon.toml` is not in git, change it there if your database is different.
Canyon reads the first `canyon*.toml` it finds (up to 2 folders deep), so don't add other files with that name.
With `migrations = 'enabled'` building connects to that database and creates `canyon_memory`, and running the app creates and changes its tables. Only point it to a database you own.

## Run

```sh
cargo run
```

Canyon-SQL connects to the database when compiling: it creates `canyon_memory`, compares the models with the tables and puts the migration queries in the binary, which runs them on start.
So Postgres has to be up to build, and the binary only fits the database it was built against.
If you drop or recreate the tables, force a rebuild before running:

```sh
touch src/main.rs && cargo run
```

## Tests

`cargo test` runs the unit tests. The DB tests are ignored by default and write to the DB in `canyon.toml`: run `cargo test -- --ignored` against the dev DB, after the app ran once (the tables must exist).

## Settings

| Variable | Default | Use |
|---|---|---|
| `TRIFORCE_SOURCE_LEAGUES` | `old` | `new` takes leagues and tournaments from the lolesports.com GraphQL API |
| `TRIFORCE_SOURCE_TEAMS` | `old` | `new` takes teams and players from `GetLoLTeamOverview` |
| `TRIFORCE_V2` | `on` | `off` stops filling the `v2_*` tables and skips their startup SQL. Canyon still creates the tables |
| `TRIFORCE_LOCALES` | `en-US` | Extra locales for the v2 names, comma list like `es-ES,ko-KR`. `en-US` is always pulled. A locale the API doesn't know comes back in English and is skipped |
| `TRIFORCE_GQL_MANIFEST` | `gql-manifest.json` | Where the GraphQL query manifest is kept |

`cargo run -- refresh-manifest` downloads the manifest again and exits (it connects to the DB and applies pending Canyon migrations first).

## What runs when

On start everything is fetched and saved once (live events are saved from the first 2 minutes tick), then:

| Data | Every | Change with |
|---|---|---|
| Leagues | 2 hours | `TRIFORCE_CRON_LEAGUES` |
| Tournaments | 2 hours | `TRIFORCE_CRON_TOURNAMENTS` |
| Teams and players | 1 hour | `TRIFORCE_CRON_TEAMS` |
| Schedule (current page) | 30 minutes | `TRIFORCE_CRON_SCHEDULE` |
| Live events | 2 minutes | `TRIFORCE_CRON_LIVE` |
| v2 reference tables (`GetOtherLeagues` per locale + `GetLoLTeamOverview` → `v2_*`, alternative team images from the old `getTeams`) | 2 hours at :15 (first run at start) | `TRIFORCE_CRON_V2` |

Defaults are in `src/schedules.rs`. The `TRIFORCE_CRON_*` variables take a cron expression with seconds (`sec min hour day month weekday`), useful to test a loop without waiting for its tick:

```sh
TRIFORCE_CRON_V2="0 * * ? * *" cargo run   # v2 every minute
```

## Project structure

| Path | What |
|---|---|
| `src/main.rs` | Start up and scheduled loops |
| `src/schedules.rs` | Loop schedules: defaults and `TRIFORCE_CRON_*` |
| `src/client/` | LoL Esports HTTP client (`legacy/`: old API calls, endpoints and models) |
| `src/service/` | `DataPull`: fetches and keeps the API data |
| `src/service/v2.rs` | v2 loop: fetches the new API (alternative team images from the old `getTeams`) and fills the `v2_*` tables |
| `src/client/adapters/v2/` | New API answers → v2 rows |
| `src/dao/` | Database models and writes |
| `src/dao/v2/` | v2 entities, startup SQL and upserts |
| `docker/` | Postgres for local development |
| `bash_alias.sh` | Docker aliases (`. ./bash_alias.sh`) |
| `docs/` | Workflow, decisions and investigations |
| `tools/lolesports-explorer/` | Python tools to study the LoL Esports APIs |

## Docs

- [Roadmap](ROADMAP.md)
- [Changelog](CHANGELOG.md)
- [Development workflow](docs/development-workflow.md)
- [Decisions](docs/decisions/)
- [Investigations](docs/investigations/)
