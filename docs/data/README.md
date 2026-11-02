# v2 data

What Triforce stores in the `v2_*` tables, how each table is filled and how they relate. For anyone reading the DB (WSC-Triforce or anything else): it says what there is, not how to use it.

| File | Covers |
|---|---|
| [reference.md](reference.md) | leagues, seasons, splits, tournaments, stages and their names |
| [teams-players.md](teams-players.md) | teams, players, rosters |
| [matches.md](matches.md) | events (matches and shows), their teams, games, streams |
| [game-details.md](game-details.md) | per game: sides, lineups, champions, final stats (box score) |
| [game-frames.md](game-frames.md) | per game: every feed frame (archive), 10 s series, markers |

The v1 tables (`league`, `team`, `schedule`, …) are not described here: they go away when v2 is complete (ADR 0004).

## Map

```
v2_league ─┬─ v2_league_i18n
           ├─ v2_tournament ─┬─ v2_tournament_i18n
           │   │  │          └─ v2_stage ── v2_stage_i18n
           │   │  └─ v2_split
           │   └─ v2_season ── v2_season_i18n
           ├─ v2_team (home_league_id) ── v2_team_player ── v2_player
           └─ v2_event ─┬─ v2_event_i18n
              (match)   ├─ v2_event_team ── v2_team
                        ├─ v2_stream
                        └─ v2_game ─┬─ v2_game_team ── v2_team        (box score)
                                    ├─ v2_game_player ── v2_player     (box score)
                                    ├─ v2_game_frames                  (archive)
                                    ├─ v2_game_timeline                (series)
                                    ├─ v2_game_team_series ── v2_team  (series)
                                    ├─ v2_game_player_series           (series)
                                    └─ v2_game_marker ── v2_team       (series)
v2_job_run (internal: when long jobs last ran)
```

## Sources

| Data | From | Loop, default schedule |
|---|---|---|
| leagues, seasons, splits, tournaments, teams, players, rosters | GraphQL `GetOtherLeagues`, `GetSeasonTournament`, `GetLoLTeamOverview` | `v2 reference`, every 2 h at :15 |
| `alt_image_url` of teams | old API `getTeams` | same |
| stage names | GraphQL `GetSeasonTournament` (100 tournaments a call) | same |
| events, event teams, games | GraphQL `scheduleEvents` | `v2 matches`, every 30 min at :05 and :35 (last 3 days + next year), full history once a day |
| live score, sides, streams, live game totals | GraphQL `watchLiveQuery` + live feed | `v2 live`, every minute at :30 s |
| box score of finished games | live feed (`window`, `details`), `watchVodQuery` when there's no feed | `v2 details`, a queue, ~1 game a second |
| frames archive and series | live feed (`window`, `details`) | `v2 frames` (a queue) and `v2 frames live` (every 10 s) |

Schedules change with `TRIFORCE_CRON_<LOOP>`. Every loop can be turned off (`TRIFORCE_V2`, `TRIFORCE_V2_DETAILS`, `TRIFORCE_V2_LIVE`, `TRIFORCE_V2_FRAMES`, `TRIFORCE_V2_FRAMES_LIVE`). The jobs that go through the history have a backfill switch: `TRIFORCE_BACKFILL_MATCHES` (on), `TRIFORCE_BACKFILL_DETAILS` (on), `TRIFORCE_BACKFILL_FRAMES` (off). Off means only the last 3 days. See `env/example.env`.

The new API has nothing before Worlds 2023 (2023-10-15): no v2 match is older than that.

## Conventions

- `id` is ours (a Postgres serial), `ext_id` is the API's id. Join on `id`, look things up from outside with `ext_id`. v1 and v2 ids are different numbers for the same league or team.
- Times are `timestamp` in UTC, no time zone in the column.
- Texts in other languages live in `*_i18n` tables, one row per locale (`en-US` always, more with `TRIFORCE_LOCALES`). A missing locale falls back to `en-US` in the reader's query, Triforce doesn't copy rows.
- Teams, players, events and games are never deleted: an id never disappears from under a foreign key. Rows that follow the API are replaced: rosters (`v2_team_player`), streams (`v2_stream`), API sides of a game that ends up `unneeded`, and a game's series and markers when they are built again.
- States only go forward: `unstarted` → `inProgress` → `completed` (ADR 0005). An older or wrong answer never moves a match or game back.
- `NULL` means "we don't know (yet)", never zero.
- Unique keys, checks and indexes not expressed by the Canyon entities are in `src/dao/v2/schema.sql`, applied at every start. The frames tables are only there (Canyon has no `bytea` or arrays).

## Keeping these docs current

A branch that adds or changes a v2 table, column, check, state, fill logic, schedule or switch updates the matching file here in the same branch. Check:

1. Columns and types match `src/dao/v2/models/*.rs` and `src/dao/v2/schema.sql`.
2. How it's filled: source, loop, schedule, switch.
3. States and what each one promises.
4. Relations (the map above).
5. The SQL examples still run (a DB test runs the ones in `game-frames.md`).
6. "What isn't there" is still true.
