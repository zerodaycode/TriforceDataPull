# Leagues, seasons, tournaments, stages

Filled by the `v2 reference` loop (every 2 h at :15, first run at start) from GraphQL `GetOtherLeagues` and `GetSeasonTournament`, once per locale. Structure from `en-US`, texts from every locale. Rows are upserted, never deleted. Background: [ADR 0004](../decisions/0004-schema-v2-reference-tables.md).

## Tables

### `v2_league`

| Column | Type | Meaning |
|---|---|---|
| `id` | int | ours |
| `ext_id` | bigint, unique | API league id |
| `slug` | text | `lec`, `worlds`, … |
| `image_url` | text | logo |
| `dark_image_url` | text, NULL | logo for dark backgrounds |

### `v2_league_i18n`

One row per league and locale, unique `(league_id, locale)`.

| Column | Type | Meaning |
|---|---|---|
| `league_id` | → `v2_league` | |
| `locale` | text | `en-US`, `es-ES`, … (checked `xx-XX`) |
| `name` | text | |
| `region` | text | as the site shows it (`EMEA`, `INTERNATIONAL`, …) |
| `position` | int, NULL | order on the site |

### `v2_season`, `v2_season_i18n`

`v2_season`: `id`, `ext_id`, `start_time`, `end_time` (NULL when the API has none). `v2_season_i18n`: `season_id`, `locale`, `name`.

### `v2_split`

`id`, `ext_id`. Only the id: the API sends split ids like `split_2`, not names.

### `v2_tournament`, `v2_tournament_i18n`

| Column | Type | Meaning |
|---|---|---|
| `id`, `ext_id` | | |
| `league_id` | → `v2_league` | |
| `season_id` | → `v2_season`, NULL | |
| `split_id` | → `v2_split`, NULL | |
| `slug` | text | |
| `start_time`, `end_time` | timestamp, NULL | |

`v2_tournament_i18n`: `tournament_id`, `locale`, `name`.

### `v2_stage`, `v2_stage_i18n`

`v2_stage`: `id`, `ext_id`, `tournament_id`. `v2_stage_i18n`: `stage_id`, `locale`, `name` (from `GetSeasonTournament`, 100 tournaments a call; a failed call keeps the names stored before).

## Relations

`v2_league` ← `v2_tournament` ← `v2_stage`. A tournament can point at a season and a split. Matches point at their league and tournament (`v2_event`), teams at their home league (`v2_team.home_league_id`).

## What isn't there

- Split names (only ids).
- Stage sections (groups, brackets) and standings (`GetSeasonStage`, not read yet).
- Leagues and tournaments that the API doesn't list any more stay as they were.
