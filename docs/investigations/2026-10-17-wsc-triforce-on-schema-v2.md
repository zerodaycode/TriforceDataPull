# WSC-Triforce on schema v2

Date: 2026-10-17

WSC-Triforce is the Rocket API that reads the Triforce DB (`src/main.rs`, `src/models/*.rs`). This is what each endpoint reads today and the SQL to move it to the v2 reference tables.

Event endpoints wait for the v2 event tables (not started yet). Reference endpoints can move now, except `/teams`, which moves together with `/team/<id>/schedule` (see the `id` note under Endpoints).

## Locale

Names live in `v2_*_i18n` (one row per locale). Every endpoint takes a `locale` parameter (default `en-US`) and falls back to `en-US` when the row is missing:

```sql
LEFT JOIN v2_league_i18n li ON li.league_id = l.id AND li.locale = $1
LEFT JOIN v2_league_i18n lf ON lf.league_id = l.id AND lf.locale = 'en-US'
-- COALESCE(li.name, lf.name)
```

`$1` is always a bound parameter. Teams and players have no i18n table, their fields are plain columns.

## Endpoints

v1 and v2 `id`s are different numbers for the same league or team. Until the v2 event tables exist, an endpoint that mixes both joins on `ext_id`, and `/teams` moves together with `/team/<id>/schedule`.

| Endpoint | Reads today | Moves to |
|---|---|---|
| `/leagues` | `league` (`find_all`) | `v2_league` + `v2_league_i18n` |
| `/leagues-with-streams` | `league`, `stream`, `schedule` | leagues now, streams after events |
| `/tournaments` | `tournament` (`find_all_unchecked`) | `v2_tournament` + `v2_tournament_i18n` |
| `/teams` | `team` | `v2_team`, together with `/team/<id>/schedule` |
| `/players` | `player` | `v2_player` |
| `/search-bar-data/<query>` | `team`, `player` | `v2_team`, `v2_player` |
| `/preview-incoming-events` | `schedule`, `team`, `league` | after the v2 event tables |
| `/team/<id>/schedule` | `schedule`, `team`, `league` | after the v2 event tables |

### `/leagues`

Today: `id, ext_id, slug, name, region, image_url`.

```sql
SELECT l.id, l.ext_id, l.slug,
       COALESCE(li.name, lf.name)     AS name,
       COALESCE(li.region, lf.region) AS region,
       l.image_url, l.dark_image_url
FROM v2_league l
LEFT JOIN v2_league_i18n li ON li.league_id = l.id AND li.locale = $1
LEFT JOIN v2_league_i18n lf ON lf.league_id = l.id AND lf.locale = 'en-US'
ORDER BY COALESCE(li.position, lf.position), l.id;
```

### `/leagues-with-streams`

Today it calls `League::find_all()` and joins `stream` to `schedule` with `state = 'inProgress'`, then matches `stream.league_id` in Rust. The league part is the query above. The stream part stays as is until the v2 event tables have their own schedule and stream tables. Until then the v2 league joins back to the v1 one on `ext_id`:

```sql
FROM v2_league l2
JOIN league l1 ON l1.ext_id = l2.ext_id
JOIN schedule s ON s.league_id = l1.id AND s.state = 'inProgress'
```

### `/tournaments`

Today: `id, ext_id, slug, start_date, end_date, league`. v2 has `start_time` / `end_time` (timestamps, UTC) and the league FK is `league_id`.

```sql
SELECT t.id, t.ext_id, t.slug,
       COALESCE(ti.name, tf.name) AS name,
       t.start_time, t.end_time, t.league_id
FROM v2_tournament t
LEFT JOIN v2_tournament_i18n ti ON ti.tournament_id = t.id AND ti.locale = $1
LEFT JOIN v2_tournament_i18n tf ON tf.tournament_id = t.id AND tf.locale = 'en-US'
ORDER BY t.start_time DESC NULLS LAST;
```

### `/teams`

Today: `id, ext_id, name, slug, code, image_url, alt_image_url, bg_image_url`.

```sql
SELECT id, ext_id, name, slug, code, image_url, light_image_url,
       alt_image_url, bg_image_url, home_league_id
FROM v2_team
ORDER BY id;
```

`light_image_url` and `alt_image_url` are `NULL` for now: the new API has no source for them yet (ADR 0004).

### `/players`

Today: `id, ext_id, first_name, last_name, summoner_name, image_url, role`. `summoner_name` is `handle` in v2.

```sql
SELECT id, ext_id, handle AS summoner_name, first_name, last_name, image_url, role
FROM v2_player
ORDER BY id;
```

### `/search-bar-data/<query>`

Two queries, the term is bound once as `$1 = '%' || term || '%'`. Escape `%` and `_` in the term before.

```sql
SELECT id, name, image_url, slug
FROM v2_team
WHERE name ILIKE $1 OR slug ILIKE $1 OR code ILIKE $1
ORDER BY id DESC;

SELECT id, handle, first_name, last_name, image_url, role
FROM v2_player
WHERE handle ILIKE $1 OR first_name ILIKE $1 OR last_name ILIKE $1;
```

### Event endpoints

`/preview-incoming-events` and `/team/<id>/schedule` read `schedule` joined to two teams and the league. They move after the v2 event tables. Write them with `LEFT JOIN` on both teams (see issue 4).

## Issues

| # | Issue | Fix |
|---|---|---|
| 1 | `canyon.toml` handling in WSC-Triforce | Keep only a `canyon.toml.example` in git |
| 2 | `/search-bar-data` and `/team/<id>/schedule` query building | Bound parameters, as in the queries above |
| 3 | It uses Canyon 0.4.2 and its entities (`League`, `Team`, `Tournament`, ...) don't match our tables or the `v2_*` ones | Not checked: whether Canyon 0.4.2 with only the `postgres` feature runs migrations at start. Check it before starting WSC-Triforce on the shared DB. Read with explicit SQL, or turn migrations off for it |
| 4 | `JOIN team` drops matches with a TBD team once TBD is `NULL` in v2 | `LEFT JOIN` for both teams, and show `TBD` when the code is `NULL` |

On issue 3: Canyon (the version we use) computes and runs migrations from the entities at startup, and WSC-Triforce's structs don't match our tables. We haven't checked whether Canyon 0.4.2 with only the `postgres` feature (WSC-Triforce's `Cargo.toml`) does the same. Check it before starting it on the shared DB.
