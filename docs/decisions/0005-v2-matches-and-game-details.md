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
- Game details, from the live feed: sides, lineups with champions and roles (of that game), and the final stats of each game, from the last frame. Stored as columns of `v2_game_team` and `v2_game_player` (one row per team and player per game). If we store every frame later, the last one is kept twice on purpose: a cheap box score that doesn't depend on how the frames are stored.
  - A queue in the DB (`v2_game.details` NULL), untried and newest first, ~1 game per second. A failed game waits 10 min, 40 min, 2 h 40, 10 h 40, then leaves the queue (reset in the README). 5 failures in a row pause the loop 5 min. One statement per game: if a team or a player is missing, nothing of it is written. `TRIFORCE_V2_DETAILS=off` stops only this part.
  - No feed (204), or a feed without 5 players on each side after one failed try (some stay broken for good, game 116878159559794392 has 1 blue and 0 red): sides from `watchVodQuery` for that game only, `side_source = 'api'`, the game is `no_feed`, no lineups or stats. The other games of the match get their own feed try. A feed side (`'feed'`) is never replaced by an API one.
  - The feed's (or `watchVodQuery`'s) team ids should be the match's. When one isn't (LIT 2026: the game ran under Aeterna's id, the match lists TEAM AQUA), it takes the match's other team, logged: the schedule is what results and standings use. Both unknown, or the other slot still TBD: an error, nothing written.
  - A player the feed has no id for (missing or `"-"`, national teams, some subs) keeps the slot with `player_id` NULL and the feed name in `summoner_name`.
  - Winner per game (`v2_game_team.result`): sweeps, and a score change while the match is live. Between two answers one team has exactly one more win and the other the same, so game number "total wins" is that team's (it needs the sides of that game). A missed update gives nothing, no guess.
  - Live matches from `watchLiveQuery` once a minute (2026-10-31): the same writes as the schedule, plus the sides of each game as soon as they're picked (`side_source = 'api'`, with the match's teams like the feed's, and only for teams of the match; removed if the game ends up `unneeded`). This query has no event state (all `inProgress`) and no match type (the stored one is kept). A match that leaves it is read again from `scheduleEvents` until it's `completed`. For the game being played, the feed's first and latest windows give the start, the end (`finished`) and the team totals right now, as `side_source = 'feed'` rows; the details loop replaces them with the final numbers, and live never writes a game that has details. The live and matches loops write events one at a time (the never-go-back check reads, then writes). The feed is read 90 s back: it answers 400 for a window that ended less than 50 s ago, and 404 ("Stats are disabled") for the first minutes of some games; both just wait for the next minute.

## Consequences
- Good:
  - Teams by id: no wrong team by name, no wins in the wrong column.
  - Games with their state, the side of each team, the lineups and the final stats per game.
  - A full history pass is ~1 min (115 pages) per locale, the 30 min ticks read 1 page per locale.
  - Old copies of an answer don't undo a result.
- Bad:
  - A real correction that lowers a result is skipped too (logged). Corrections need their own design.
  - A game the API drops (a Bo5 changed to a Bo3) stays in `v2_game`.
  - The per-game winner is only known for sweeps or when the app was running during the match (a winner needs two answers in a row around the end of the game).
  - The feed isn't used by the new site and can go away. A long outage takes games out of the queue after their 5 tries (~14 h), they need the reset.
  - Matches before 2023-10 are not in `scheduleEvents`.
  - Live totals of a game that never gets its details (5 failed tries, or no feed after the game) stay as the last ones seen, not the final ones.

## Evidence
- [Live recording and feed history](../investigations/2026-10-18-live-recording-and-feed-history.md)
- [Live match state](../investigations/2026-10-16-live-match-state-design.md)
- [ADR 0004](0004-schema-v2-reference-tables.md)
- `src/dao/v2/events.rs`, `src/dao/v2/details.rs`, `src/service/v2_matches.rs`, `src/service/v2_details.rs`, `src/service/v2_live.rs`, `src/client/gql/ops/watch_live.rs`
