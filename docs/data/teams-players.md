# Teams, players, rosters

Filled by the `v2 reference` loop (every 2 h at :15) from GraphQL `GetLoLTeamOverview`, plus `alt_image_url` from the old API's `getTeams`. Teams and players are upserted and never deleted. The match and details loops can add a team or player they don't know with the little they have (a stub); they never change a known one. Background: [ADR 0004](../decisions/0004-schema-v2-reference-tables.md).

## Tables

### `v2_team`

| Column | Type | Meaning |
|---|---|---|
| `id`, `ext_id` | | `ext_id` unique |
| `slug`, `name`, `code` | text | `code` is the short name (`G2`, `T1`) |
| `image_url` | text | logo |
| `light_image_url` | text, NULL | always NULL for now: no source |
| `bg_image_url` | text, NULL | team page background |
| `alt_image_url` | text, NULL | from the old API while it lives; values stay when it goes away |
| `home_league_id` | → `v2_league`, NULL | |

### `v2_player`

| Column | Type | Meaning |
|---|---|---|
| `id`, `ext_id` | | `ext_id` unique |
| `handle` | text | in-game name |
| `first_name`, `last_name` | text | |
| `image_url` | text, NULL | NULL when the API only has a placeholder photo (known file names, or a photo shared by 3+ players) |
| `role` | text | as the API sends it (`top`, `jungle`, `mid`, `bottom`, `support`, …) |

### `v2_team_player`

The current roster: one row per `(team_id, player_id)`, unique. Replaced team by team on every run: players the API no longer lists in a team leave it.

## Relations

`v2_team` ← `v2_team_player` → `v2_player`. Teams are in matches (`v2_event_team`) and games (`v2_game_team`, `v2_game_team_series`); players in games (`v2_game_player`).

## What isn't there

- Roster history (who played where and when): only the current roster.
- A player's role in a given game is in `v2_game_player.role`, not here.
