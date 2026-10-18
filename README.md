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

Set them in the shell or in a file under `env/`. Copy `env/example.env` (it lists every variable with its default) to a name of your own, like `env/dev.env`, and pick it when you run:

```sh
cargo run -- --env env/dev.env
```

- Only `env/example.env` is in git, keep one file per setup (dev, docker, ...).
- Variables set in the shell win over the file.
- A file given with `--env` must exist, the app stops otherwise. Without `--env` no file is read.
- Format is the `docker compose` `env_file:` one: `KEY=value`, `#` comments, quote values with spaces (the cron ones). Not `docker run --env-file`: it keeps the quotes as part of the value.
- `$NAME` in a value is replaced by that variable (empty when not set), single quotes (`'a$b'`) keep the `$`.
- An unknown `TRIFORCE_*` name in the file is logged and ignored.

| Variable | Default | Use |
|---|---|---|
| `TRIFORCE_SOURCE_LEAGUES` | `old` | `new` takes leagues and tournaments from the lolesports.com GraphQL API |
| `TRIFORCE_SOURCE_TEAMS` | `old` | `new` takes teams and players from `GetLoLTeamOverview` |
| `TRIFORCE_V2` | `on` | `off` stops filling the `v2_*` tables and skips their startup SQL. Canyon still creates the tables |
| `TRIFORCE_V2_DETAILS` | `on` | `off` stops only the game details from the live feed (`TRIFORCE_V2` must be `on` for them) |
| `TRIFORCE_LOCALES` | (none) | Extra locales for the v2 names and match block names (each one reads `scheduleEvents` again), comma list like `es-ES,ko-KR`. `en-US` is always pulled. A locale the API doesn't know comes back in English and is skipped |
| `TRIFORCE_GQL_MANIFEST` | `gql-manifest.json` | Where the GraphQL query manifest is kept |

`cargo run -- refresh-manifest` (also with `--env <file>`) downloads the manifest again and exits (it connects to the DB and applies pending Canyon migrations first).

## What runs when

On start everything is fetched and saved once (live events are saved from the first 2 minutes tick), then:

| Data | Every | Change with |
|---|---|---|
| Leagues | 2 hours | `TRIFORCE_CRON_LEAGUES` |
| Tournaments | 2 hours | `TRIFORCE_CRON_TOURNAMENTS` |
| Teams and players | 1 hour | `TRIFORCE_CRON_TEAMS` |
| Schedule (current page) | 30 minutes | `TRIFORCE_CRON_SCHEDULE` |
| Live events | 2 minutes | `TRIFORCE_CRON_LIVE` |
| v2 reference tables (`GetOtherLeagues` and `GetSeasonTournament` per locale + `GetLoLTeamOverview` → `v2_*`, alternative team images from the old `getTeams`) | 2 hours at :15 (first run at start) | `TRIFORCE_CRON_V2` |
| v2 matches (`scheduleEvents` → `v2_event`, `v2_event_i18n`, `v2_event_team`, `v2_game`, `v2_stream`, game winners of sweeps in `v2_game_team.result`, last full pass in `v2_job_run`): the last 3 days and the next year on every run, the whole history from 2023-10 at most once a day. Starts after the first v2 reference run | 30 minutes at :05 and :35 (first run at start) | `TRIFORCE_CRON_V2_MATCHES` |
| v2 game details (live feed → sides, lineups with champions and roles, and the final stats in `v2_game_team` / `v2_game_player`; `watchVodQuery` sides when the feed has nothing): finished games without details, untried and newest first, about 1 game per second. Starts after the first v2 reference run. Stop and start any time, the queue is in the DB | all the time, 5 min pause when there's nothing left or after 5 failures in a row | `TRIFORCE_V2_DETAILS=off` stops only this |

A game whose details fail waits 10 min, 40 min, 2 h 40 and 10 h 40 between tries, after 5 tries it leaves the queue. After a fix, put those games back:

```sql
UPDATE v2_game SET details_tries = NULL, details_next_try = NULL WHERE details IS NULL;
```

Defaults are in `src/schedules.rs`. The `TRIFORCE_CRON_*` variables take a cron expression with seconds (`sec min hour day month weekday`), useful to test a loop without waiting for its tick:

```sh
TRIFORCE_CRON_V2="0 * * ? * *" cargo run   # v2 every minute
```

## Project structure

| Path | What |
|---|---|
| `src/main.rs` | Start up and scheduled loops |
| `src/schedules.rs` | Loop schedules: defaults and `TRIFORCE_CRON_*` |
| `src/env.rs` | Command line (`--env`, `refresh-manifest`) and settings: shell first, then the `--env` file |
| `env/` | Settings files, only `example.env` is in git |
| `tests/fixtures/` | Test data: saved API answers (`gql/`) and a manifest chunk (`manifest/`), read with `fixture!("<area>/<file>")` |
| `src/client/` | LoL Esports HTTP client (`legacy/`: old API calls, endpoints and models) |
| `src/service/` | `DataPull`: fetches and keeps the API data |
| `src/service/v2.rs` | v2 loop: fetches the new API (alternative team images from the old `getTeams`) and fills the `v2_*` tables |
| `src/service/v2_matches.rs` | v2 matches loop: `scheduleEvents` pages → `v2_event*`, `v2_game`, `v2_stream`, never going back |
| `src/service/v2_details.rs` | v2 game details loop: the queue of finished games → feed → `v2_game_team`, `v2_game_player` |
| `src/client/feed/` | Live feed client (`feed.lolesports.com/livestats/v1`), also for finished games |
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
