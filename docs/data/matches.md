# Matches, games, streams

Filled by two loops. `v2 matches` reads GraphQL `scheduleEvents`: the last 3 days and the next year every 30 min (:05, :35), and the whole history (from 2023-10-01) once a day while `TRIFORCE_BACKFILL_MATCHES=on`. `v2 live` reads `watchLiveQuery` every minute (:30 s) while matches are live. Background: [ADR 0005](../decisions/0005-v2-matches-and-game-details.md).

## Tables

### `v2_event`

A match or a show.

| Column | Type | Meaning |
|---|---|---|
| `id`, `ext_id` | | `ext_id` unique (the API match id) |
| `kind` | text | `match` or `show` |
| `league_id` | → `v2_league` | |
| `tournament_id` | → `v2_tournament`, NULL | |
| `start_time` | timestamp, NULL | planned start |
| `state` | text | `unstarted`, `inProgress`, `completed` |
| `strategy_type` | text, NULL | `bestOf` or `playAll` |
| `strategy_count` | int, NULL | 1, 3, 5 (bestOf) or 2 (playAll) |
| `match_type` | text, NULL | `normal` or `tiebreaker` |

### `v2_event_i18n`

`event_id`, `locale`, `block_name` (`Week 3`, `Semifinals`, …), unique `(event_id, locale)`.

### `v2_event_team`

Two rows per match, `slot` 1 and 2, unique `(event_id, slot)`. A slot's team, wins and outcome always come from the same answer.

| Column | Type | Meaning |
|---|---|---|
| `team_id` | → `v2_team`, NULL | NULL while the slot is TBD |
| `game_wins` | int, NULL | |
| `outcome` | text, NULL | `win`, `loss`, `tie` (ties: `playAll 2` ending 1-1) |

Slot order is the API's: it's game 1's blue team first and changes when game 1's sides are set. Don't read anything into slot 1 vs 2.

### `v2_game`

One row per game of a match, unique `ext_id` and `(event_id, number)`.

| Column | Type | Meaning |
|---|---|---|
| `event_id`, `number` | | game 1, 2, … |
| `state` | text | `unstarted`, `inProgress`, `completed`, `unneeded` (not played: a Bo5 won 3-0) |
| `patch` | text, NULL | from the feed |
| `start_time`, `end_time` | timestamp, NULL | first and last feed frame (wall time) |
| `details`, `details_tries`, `details_next_try` | | see [game-details.md](game-details.md) |
| `frames`, `frames_tries`, `frames_next_try` | | see [game-frames.md](game-frames.md) |

### `v2_stream`

The streams of a live match: `event_id`, `provider` (`youtube`, `twitch`, …), `parameter` (channel or video id), `locale`, `english_name`. Unique `(event_id, provider, parameter)`. Replaced on every live answer: there are streams only while a match is live.

## States and guarantees

- A match or game state never goes back, and wins and outcomes never go down: an answer that would lower them is skipped and logged. A real API correction that lowers a result is skipped too (known gap, ROADMAP).
- `completed` doesn't mean the score is final: the outcome can come a minute later.
- Games and team slots the API stops sending stay (a Bo5 changed to a Bo3 keeps games 4 and 5).
- Live: score and state within about a minute (the API caches 2 min). A match that leaves the live answer is read from the schedule until it's `completed`, up to 20 min.

## Relations

`v2_event` → `v2_league`, `v2_tournament`; ← `v2_event_team` → `v2_team`; ← `v2_game` ← game details and frames.

## What isn't there

- Anything before 2023-10-15 (the API has nothing older).
- Streams of past matches.
- Stage or section of a match (the API's `scheduleEvents` doesn't link them).
