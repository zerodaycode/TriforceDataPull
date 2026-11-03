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
- A game with a chronobreak gets its box score from its frames: after the restore the feed counts from 0 again, so its last frame isn't the box score. The frames job writes it (see [game-frames.md](game-frames.md)) when its after-game write comes after the details, or when the details are `wait_frames`. Dragons leave out the ones a chronobreak undid, the skill order joins the skills from before and after the pause. Items are NULL there, the feed's list only has what was bought after the restore.
- A feed whose last frame is a pause is `wait_frames`. After a chronobreak or a long pause the feed's `window` can answer the pause frame as the last one even when the game went on. Sides and lineups are stored, the numbers stay NULL and the live totals are cleared, until the frames job writes the box score. `end_time` keeps what `v2 live` stored (NULL when nobody watched the game live), the box score sets it to the last frame of the frames.
- Details written after the frames saw a chronobreak (a `v2_game_pause` row with `rollback_s` or `counters_reset`) are `wait_frames` too, without numbers. The frames loop then writes the box score from the archive.
- A game already `done` whose `end_time` is more than a minute before the end of its frames (the details took a pause frame) gets its box score from the frames too.
- A `wait_frames` game keeps NULL numbers with `TRIFORCE_V2_FRAMES=off`, or when its frames end `no_feed`. With `TRIFORCE_BACKFILL_FRAMES=off` an older game already `done` keeps the feed's last frame until `rebuild-timeline`.
- While a game is played, `v2 live` writes the sides and team totals right now (`side_source = 'feed'`); the details replace them with the final numbers, or clear them when the game is `wait_frames`.

## `v2_game.details`

| Value | Meaning |
|---|---|
| NULL | not done yet (or failed, waiting: `details_tries`, `details_next_try` 10 min, 40 min, 2 h 40, 10 h 40) |
| `done` | sides, lineups and last-frame numbers stored |
| `wait_frames` | the feed's `window` answered a `paused` frame as the last one, or the frames saw a chronobreak first: sides and lineups stored, numbers NULL until the frames job writes the box score, then `done`. A box score from the archive that fails waits like the details (`details_tries`, `details_next_try`), after 5 tries the game stays `wait_frames` |
| `no_feed` | the feed has nothing (204) or not 5 players a side: sides from `watchVodQuery`, no lineups or numbers |

After 5 failed tries a game leaves the queue with `details` still NULL (reset by hand, see the README).

## What isn't there

- Games of leagues without feed (`asian_games`, `cacg`, `pcs` seen) have sides only.
- The winner of each game of a non-sweep that nobody watched live.
