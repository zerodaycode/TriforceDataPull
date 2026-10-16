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
cp canyon.example.toml canyon.toml
```

`canyon.toml` is not in git, change it there if your database is different.
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

## What runs when

On start everything is fetched and saved once (live events are saved from the first 2 minutes tick), then:

| Data | Every |
|---|---|
| Leagues | 2 hours |
| Tournaments | 2 hours |
| Teams and players | 1 hour |
| Schedule (current page) | 30 minutes |
| Live events | 2 minutes |

Schedules are in `src/main.rs`.

## Project structure

| Path | What |
|---|---|
| `src/main.rs` | Start up and scheduled loops |
| `src/service/` | LoL Esports API client |
| `src/data_pull/` | API models |
| `src/dao/` | Database models and writes |
| `src/utils/` | Constants (API base URL and endpoints) |
| `docker/` | Postgres for local development |
| `bash_alias.sh` | Docker aliases (`. ./bash_alias.sh`) |
| `docs/` | Workflow, decisions and investigations |

## Docs

- [Roadmap](ROADMAP.md)
- [Changelog](CHANGELOG.md)
- [Development workflow](docs/development-workflow.md)
- [Decisions](docs/decisions/)
- [Investigations](docs/investigations/)
