# Game details (box score)

Per game: which team played blue and red, the lineups with champions and roles, and each team's and player's numbers at the end of the game (the feed's last frame). Filled by the `v2 details` loop from the live feed, and partly by `v2 live` while a game is played. Background: [ADR 0005](../decisions/0005-v2-matches-and-game-details.md). Every frame of the game, not only the last one, is in [game-frames.md](game-frames.md).

## Tables

### `v2_game_team`

One row per team and game, unique `(game_id, team_id)`.

| Column | Type | Meaning |
|---|---|---|
| `game_id`, `team_id` | → `v2_game`, `v2_team` | |
| `side` | text | `blue` or `red` |
| `side_source` | text | `feed` (the game itself, final) or `api` (`watchLiveQuery` before the game, or `watchVodQuery` when there's no feed). A feed side is never replaced by an API one |
| `result` | text, NULL | `win`, `loss`. Known for sweeps (2-0, 3-0, Bo1) and for games that ended while Triforce watched the match live; NULL otherwise |
| `gold`, `kills`, `towers`, `inhibitors`, `barons` | int, NULL | last frame |
| `dragons` | text, NULL | dragon types in order, comma separated (`ocean,infernal,elder`) |

### `v2_game_player`

One row per player slot and game, unique `(game_id, participant_id)`.

| Column | Type | Meaning |
|---|---|---|
| `game_id`, `team_id` | | |
| `player_id` | → `v2_player`, NULL | NULL when the feed has no esports id for the player |
| `summoner_name` | text, NULL | as the feed sends it, with the team code |
| `participant_id` | int | 1-5 blue, 6-10 red |
| `role`, `champion` | text | of this game |
| `level`, `kills`, `deaths`, `assists`, `gold`, `cs`, `wards_placed`, `wards_destroyed` | int, NULL | last frame |
| `damage_share_bp` | int, NULL | share of the team's champion damage × 10,000 (`2144` = 21.44 %) |
| `items` | text, NULL | item ids, comma separated |
| `rune_style`, `rune_sub_style` | int, NULL | |
| `runes` | text, NULL | rune ids, comma separated |
| `skill_order` | text, NULL | `Q,W,E,…` in the order taken |

## How it's filled

- The queue: completed games with `v2_game.details` NULL, untried and newest first, about one game a second. With `TRIFORCE_BACKFILL_DETAILS=off` only games of matches from the last 3 days.
- One write per game: if a team or a player is missing, nothing of that game is written.
- While a game is played, `v2 live` writes the sides and team totals right now (`side_source = 'feed'`); the details replace them with the final numbers.

## `v2_game.details`

| Value | Meaning |
|---|---|
| NULL | not done yet (or failed, waiting: `details_tries`, `details_next_try` 10 min, 40 min, 2 h 40, 10 h 40) |
| `done` | sides, lineups and last-frame numbers stored |
| `no_feed` | the feed has nothing (204) or not 5 players a side: sides from `watchVodQuery`, no lineups or numbers |

After 5 failed tries a game leaves the queue with `details` still NULL (reset by hand, see the README).

## What isn't there

- Games of leagues without feed (`asian_games`, `cacg`, `pcs` seen) have sides only.
- The winner of each game of a non-sweep that nobody watched live.
