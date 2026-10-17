# Migration map: old API → lolesports.com GraphQL

Date: 2026-10-16

For every call Triforce makes to the old API (`esports-api.lolesports.com/persisted/gw`), the new operation and fields. Details of both APIs: [old API and live feed](2026-10-16-lolesports-old-api-and-live-feed.md), [GraphQL API](2026-10-16-lolesports-graphql-api.md). Decision: [ADR 0003](../decisions/0003-migrate-to-lolesports-graphql-api.md) (proposed).

Every old call has a new equivalent, all called from a script (test 3). All of them returned real data. `watchLiveQuery` first returned `events: []` (nothing live), and was checked during a live match on 2026-10-16 ([live check](2026-10-17-old-vs-new-api-comparison.md)). Still open: live in-game stats (see [Still open](#still-open)). Fields marked *(community docs)* come from the public, unofficial docs of the old API and weren't seen in our recordings.

## What Triforce reads today

`DataPull` is in `src/service/mod.rs`, the API models in `src/data_pull/`, the HTTP call in `src/service/caller.rs` and the endpoints in `src/utils/constants.rs`.

| `DataPull` method | Old endpoint | Params | Used for |
|---|---|---|---|
| `fetch_leagues` | `getLeagues` | - | `leagues[]`, `id`, `slug`, `name` are used later to find ids |
| `fetch_tournaments` | `getTournamentsForLeague` | `leagueId` (one call per league) | `leagues[0].tournaments[]`, tagged with `league_id` |
| `fetch_teams_and_players` | `getTeams` | - (all teams) | `teams[]` + `players[]`, `home_league.name` → `league_id` by name |
| `fetch_current_page_schedule` | `getSchedule` | - | one page of `events[]` |
| `process_full_schedule` | `getSchedule` | `pageToken` | every page (≤ 25 older, all newer), `event.league.slug` → `league_id` by slug, `match.teams[].name` → `team_left_id`/`team_right_id` by name |
| `fetch_live` | `getLive` | - | `events[]` (`EventDetails`) |
| `fetch_change_in_events` | `getEventDetails` | `id` | called when an event leaves `getLive` |

All calls use `hl=en-US` and the public `x-api-key`, 15 s timeout, 2 retries on timeout.

Old fields *(community docs)*:

- `getLeagues` → `leagues[] { id, slug, name, region, image, priority, displayPriority { position, status } }`
- `getTournamentsForLeague` → `leagues[] { tournaments[] { id, slug, startDate, endDate } }`
- `getTeams` → `teams[] { id, slug, name, code, image, alternativeImage, backgroundImage, status, homeLeague { name, region }, players[] { id, summonerName, firstName, lastName, image, role } }`
- `getSchedule`, `getLive`, `getEventDetails`: see [the old API investigation](2026-10-16-lolesports-old-api-and-live-feed.md#old-api-esports-apilolesportscompersistedgw).

### How Triforce decides a match ended

From `fetch_change_in_events`:

1. An event is changed when it isn't exactly equal to its previous `getLive` copy, and new when its id wasn't in the previous `getLive`.
2. An event that left `getLive` is taken as ended. Triforce calls `getEventDetails` and sets `state = "completed"` itself (`src/service/mod.rs`), whatever the API says. Shows are also forced to `completed`.
3. If `getEventDetails` fails to parse (also a non-200 answer), the event is skipped without a log.
4. If the `getEventDetails` request fails (3 timeouts or any other send error), `fetch_change_in_events` returns early. `previous_live` was already replaced (`src/service/mod.rs:280`) and `src/main.rs` ignores the error, so that event and the rest of the batch never get their final state, and nothing is logged. The start up call has the same problem.
5. Wins are copied by array position (`teams.first()` / `teams.get(1)`, `src/dao/models/event.rs`), not by team id, although the team order can change (see [the old API investigation](2026-10-16-lolesports-old-api-and-live-feed.md#getschedulehlen-uspagetoken)). The 30 min schedule job rewrites ids and wins from the same array, so a wrong value can last until then.

Points 4 and 5 are confirmed in the code, but no recording ties them to a real P1-P3 case yet. Another possible link (idea, not checked). If a tier 2/3 event drops out of `getLive` for one poll and comes back:

- First drop: it's saved as `completed` with the score at that moment.
- When it comes back: it's processed as a new live event (P1).
- The final score: only arrives if a later change is caught (P2/P3).

Batch 3 had no such drop at Worlds, a tier 2/3 recording would tell.

## Ids are the same (checked)

| Entity | Example id | Old API | New GraphQL |
|---|---|---|---|
| League | `98767975604431411` (Worlds) | `league.id` | `league.id` |
| Tournament | `115660540725177488` (Worlds 2026) | `tournament.id` | `tournament.id` |
| Match / event | `115660588583040710` (MVK-FUR) | `match.id` = event id | `match.id` = `EventMatch.id` |
| Team | `107251245690956393` (MVK) | `teams[].id` | `MatchTeam.teamId`, `Team.id` (`MatchTeam.id` is `"<matchId>:<teamId>"`) |
| Game | `115660588583040711` (MVK-FUR game 1) | `games[].id`, feed URLs | `match.games[].id` |

Existing rows keep their ids: `league`, `tournament`, `team` and `player` by `ext_id`, `schedule` by `match_id`. Shows have no stored id, they are matched by `start_time` + `league_id` (`src/dao/mod.rs`), the spike has to check that `EventShow.startTime` gives the same values. The live feed takes the same game ids.

## Call by call

Operation details: [GraphQL API](2026-10-16-lolesports-graphql-api.md#operations-for-lol-esports-data). "tested" = found and called with data, "to check" = found, details to check.

### `getLeagues` → `getLeagues` / `homeLeagues` / `GetOtherLeagues` (tested)

| Old field | New field | Note |
|---|---|---|
| `id`, `slug`, `name`, `image` | same (`getLeagues`) | |
| `region` | `region` (`homeLeagues`, `GetOtherLeagues`) | Translated by `hl` (`$locale` in `GetOtherLeagues`): pass `en-US` to keep the stored values. `regionSlug` exists on `Tournament.league` |
| `displayPriority { position, status }` | same (`homeLeagues`) | `status` has a new value `hidden` |
| `priority` | none | Not in the new API, `displayPriority.position` is the closest |

`flags: ["excludeHidden","excludeWithoutTournaments"]` gives what the site shows. Test 3: without flags 51 leagues, 9 of them `hidden` (LTA, LLA, LCO, LCL…), with flags 41 (42 when we checked again the same evening).

Triforce stores every league `getLeagues` returns (no filter, 51 on 2026-10-16, `SELECT count(*) FROM league`), so to keep the same data use no flags, or `GetOtherLeagues` which also has the tournaments.

### `getTournamentsForLeague(leagueId)` → `GetOtherLeagues` (tested)

- One call returns every league with its tournaments: `{ id, name, slug, startTime, endTime, season { id, name, startTime, endTime }, split { id }, stages { id, displayState } }`. Today we do one call per league (51 calls).
- No filter per league, filter on our side.
- `startDate`/`endDate` (dates) become `startTime`/`endTime` (datetimes). Our `tournament.start_date`/`end_date` are `date` columns. Checked on all 513: our date is the UTC date of `startTime`/`endTime` ([comparison](2026-10-17-old-vs-new-api-comparison.md#data-comparison-our-db-vs-the-new-api)), so keep the columns and take the UTC date.
- New: season, split, stages.
- Other options: `Season` / `GetSplitLeague` (per season), `GetGprSeasons` (all seasons).

### `getTeams` → `GetLoLTeamOverview` (tested)

| Old field | New field |
|---|---|
| `id`, `slug`, `name`, `code`, `image` | same |
| `backgroundImage` | `backgroundImage` |
| `alternativeImage` | none (closest: `lightImage` on `MatchTeam` in events) |
| `status` | `status` (`SettingsTeams`) |
| `homeLeague { name, region }` | `homeLeague { id, name, region, slug, image }`: has the league id, so no more lookup by name |
| `players { id, summonerName, firstName, lastName, image, role }` | same, plus `handle`. `GetTeams` has `handle`, `status`, `positions`, `homeTeam` instead |

`GetLoLTeamOverview` without `teamId` returns all teams like the old `getTeams`: 1,586 teams, 963 with players. They have 5,431 player entries but only 4,526 different players, a player can be in more than one team. That's exactly our `team_player` (5,431) and `player` (4,526) tables, checked on 2026-10-16. 737 teams have no `homeLeague`, 623 have no players (inactive or academy teams probably). The answer is 1.7 MB and cached only 7 s. `GetTeams`/`SettingsTeams` aren't needed.

Canyon's `update()` writes the whole row, so a straight adapter would set `team.alt_image_url` to NULL on the first run: 1,325 of our 1,586 teams have one (2026-10-16). Keep the stored value or drop the column on purpose (open point in [ADR 0003](../decisions/0003-migrate-to-lolesports-graphql-api.md#open-before-accepting)).

### `getSchedule(pageToken)` → `scheduleEvents` (tested)

| Old | New | Note |
|---|---|---|
| `events[]` | `esports.events[]` (`EventMatch` / `EventShow`) | |
| `startTime`, `state`, `type`, `blockName` | same | |
| `league { name, slug }` | `league { id, name, slug, image, displayPriority }` | League id included, no more lookup by slug |
| `match { id, flags, strategy }` | same, plus `match.state`, `match.type` | |
| `match.teams[] { name, code, image, result, record }` | `matchTeams[] { teamId, code, name, image, lightImage, homeLeague, result }` | Team ids included (the old schedule had none). Today we find the teams by name and 49 names are used by more than one team: 24 matches are linked to the wrong team, `teamId` fixes them ([comparison](2026-10-17-old-vs-new-api-comparison.md#schedule-teams-wrong-team-by-name)). TBD slots have `teamId: "0"` (today we link a fake "TBD" team). Team order can differ from ours, map wins by `teamId`, not by position. `record` isn't in this query (it is in `watchLiveQuery`/`GetTeamEvents`) |
| - | `match.games[] { id, number, state, vods, recaps }` | New: game ids and states in the schedule |
| - | `tournament { id, name }`, `streams` | New |
| `pages { older, newer }` + `pageToken` | same | |

New filters: `leagues`, `leaguesExclude`, `eventDateStart`/`eventDateEnd`, `eventState`, `eventType`, `pageSize` (the site uses up to 300). Our backfill stops after 25 pages (`src/service/mod.rs`), with 80 events per page that's ~2,000 events: keep `pageSize: 80` to cover the same window, or use `eventDateStart` instead of counting pages. `homeEvents` is the same query plus a `tournamentIds` filter.

### `getLive` → `watchLiveQuery` (tested live)

| Old (`getLive`) | New (`watchLiveQuery`) |
|---|---|
| `events[] { id, startTime, state, type, blockName, league, tournament, streams }` | same, but no `state` in this query |
| `match { id, strategy, teams { id, …, result, record }, games { id, number, state, teams { id, side } } }` | `match { id, strategy, games { …same… }, matchTeams { id (composite), code, name, image, result, record } }` |

No event `state` means a straight adapter would insert live rows with `state = ""` (`src/dao/models/event.rs`) and keep the old state on updates, until the 30 min schedule job fixes it. Every event from `watchLiveQuery(eventState: ["inProgress"])` is live, so the adapter sets `state = "inProgress"` itself.

Test 3: with `eventState: ["inProgress"], eventType: "all"` it answers HTTP 200, `events: []` when nothing is live, `cache-control: max-age=120, stale-while-revalidate=60`. Live scores can be up to ~3 min old, ~2 min with the old `getLive` (`max-age=60, stale-while-revalidate=60`).

Checked during a live match (FUR vs C9, 2026-10-16, [live check](2026-10-17-old-vs-new-api-comparison.md#live-check-fur-vs-c9)): same match, score, games, game states and best-of as `getLive`, no event `state`, team ids composite. Cache `max-age=120, stale-while-revalidate=60` also during the match (`age` up to 155 s). The end of game 3 came +4 min 56 s after the feed `finished` (old `getLive`: +3 min 48 s), one game measured. In game 2 `watchLiveQuery` and `scheduleEvents` disagreed with the same answer time. Not checked: which variables the live page itself sends.

Fallback with known caching: `homeEvents`/`scheduleEvents` with `eventState: ["inProgress"]`. The server accepted it in batch 4 (0 events at 04:12, nothing live) and returned the live match with `state: inProgress` and plain `teamId`s on 2026-10-16. 120 s cache + 60 s stale.

### `getEventDetails(id)` → `watchVodQuery(eventId)` (tested)

- Returns: `games { id, number, state, teams { id, side }, vods (full VOD details), recaps }`, `matchTeams { result, record }`, `strategy`, `streams`, `league`, `tournament`, `blockName`, `startTime`.
- No event or match `state`. Take it from `scheduleEvents` (filter `tournamentIds` with `homeEvents`, or a date range), or get it from the game states and score.

### New data, no old equivalent

| Need | Operation |
|---|---|
| Brackets, standings, Swiss, match destinations | `GetSeasonStage`, `GetSeasonTournamentDisplay` |
| Official winner per match | `GetSeasonTournamentDisplay.tournamentResult.matchResults { id, winningTeamId }` |
| A team's matches | `GetTeamEvents`, `GetLoLTeamEvents` |
| Rankings | `GetTeamGpr`, `GetLeagueElo` |
| Format/rules text | `Season`, `GetSeasonTournament` (`description`, `phases`, `awards`) |

## Still open

The checklist is in [ADR 0003, open before accepting](../decisions/0003-migrate-to-lolesports-graphql-api.md#open-before-accepting). Notes:

- Calls outside the browser work, with a lolesports.com `Referer` (or `Origin`) and the Apollo client headers. Test 3 called every mapped operation from a script with HTTP 200, and the mapping holds on real answers. Checked again from Triforce's environment on 2026-10-16 (`homeLeagues`, `GetLoLTeamOverview`).
- Live in-game stats: no GraphQL operation has them, and the new site doesn't seem to show them (seen 2026-10-16). The old feed `feed.lolesports.com/livestats/v1` is the only known source. It doesn't depend on the old `persisted/gw` API, takes the same game ids that `scheduleEvents`/`watchLiveQuery` return, and worked on 2026-10-15/16. But the new site doesn't use it, so it can disappear without notice. A HAR of the new live page would confirm the site makes no `livestats` request.
- The hashes belong to a build (`acab248`). A new build can change them, so a migrated pipeline needs a way to refresh the manifest, or at least to detect the failure and alert.
