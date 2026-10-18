# 0005 - v2 matches and game details

- Status: Proposed
- Date: 2026-10-18

## Context
v1 keeps a match in `schedule` with one left and one right team, found by name, and copies live wins by position. No games, no sides per game, no lineups. The new API has team ids, games and states, and the live feed has the sides, the players and the final stats of each game, also for past games ([recording and feed history](../investigations/2026-10-18-live-recording-and-feed-history.md)). Answers can go back even when they are newer.

## Decision
- Matches (`v2_event`, `v2_event_i18n`, `v2_event_team`, `v2_game`, `v2_stream`) from `scheduleEvents`, on their own loop, after the first good reference run (or 15 min): the last 3 days and the next year on every tick (`TRIFORCE_CRON_V2_MATCHES`, every 30 min), then the whole history (from 2023-10-01) at most once a day (`v2_job_run`). A full pass that fails is tried again 6 h later, the ticks go on.
- A team slot keeps its team, wins and outcome together, from the same answer. TBD is `NULL`.
- Never go back: an answer whose state, total wins or outcomes are lower than what we have is skipped and logged (once per app run). A game's state never goes back either.
- An event with a value we don't store (a new state, strategy or outcome) or that doesn't parse is skipped, the rest of the page is written. A game id the API moved to another event or number is skipped and logged, the stored game stays (two games that swap ids stay as they were).
- Streams are replaced on every answer. Games, team slots and block names are never deleted: one the API stops sending stays as it was.
- Unknown leagues and teams in an event are added with the little the event has. They are never changed from here, the reference loop owns them. Nothing deletes players or teams.
- Game details (planned, second branch): sides, lineups with champions and roles, and the final stats of each game come from the live feed, from the last frame. They are stored as columns of `v2_game_team` and `v2_game_player` (one row per team and player per game). If we store every frame later, the last one is kept twice on purpose: a cheap box score that doesn't depend on how the frames are stored.

## Consequences
- Good:
  - Teams by id: no wrong team by name, no wins in the wrong column.
  - Games with their state, and later sides and lineups per game.
  - A full history pass is ~1 min (115 pages) per locale, the 30 min ticks read 1 page per locale.
  - Old copies of an answer don't undo a result.
- Bad:
  - A real correction that lowers a result is skipped too (logged). Corrections need their own design.
  - A game the API drops (a Bo5 changed to a Bo3) stays in `v2_game`.
  - The per-game winner is only known for sweeps or when watched live.
  - The feed isn't used by the new site and can go away.
  - Matches before 2023-10 are not in `scheduleEvents`.

## Evidence
- [Live recording and feed history](../investigations/2026-10-18-live-recording-and-feed-history.md)
- [Live match state](../investigations/2026-10-16-live-match-state-design.md)
- [ADR 0004](0004-schema-v2-reference-tables.md)
- `src/dao/v2/events.rs`, `src/service/v2_matches.rs`
