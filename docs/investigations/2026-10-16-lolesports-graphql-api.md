# lolesports.com GraphQL API

Date: 2026-10-16

How the new lolesports.com gets its data and how to call it from a script. Sources: batch 1 (site pages), batch 4 (2026-10-16 04:10-04:13 CEST, 3 HARs with filter "All": two team pages and the home page, with JS bundles) and tests 1-3 with `gql_tool.py`. Build: `apollographql-client-version: acab248`, the operations below come from that build manifest ([`tools/lolesports-explorer/manifest_acab248.json`](../../tools/lolesports-explorer/manifest_acab248.json)).

The old API is in [the old API investigation](2026-10-16-lolesports-old-api-and-live-feed.md), the mapping old → new in [the migration map](2026-10-16-lolesports-api-migration-map.md).

## How it works

| | |
|---|---|
| Endpoint | `GET https://lolesports.com/api/gql` (Netlify + CloudFront in front of the GraphQL server) |
| Query string | `operationName=<name>&variables=<JSON>&extensions=<JSON>` |
| `extensions` | `{"persistedQuery":{"version":1,"sha256Hash":"<id>"}}` (the site also adds `"clientLibrary":{"name":"@apollo/client","version":"4.1.9"}`) |
| Headers | `content-type: application/json`, `apollographql-client-name: Esports Web`, `apollographql-client-version: acab248`, and from a script also a lolesports.com `Referer` or `Origin` |
| API key | The browser doesn't send one. The site code adds `x-api-key` only when a server side `ESPORTS_API_KEY` is set (server rendering) |

### Only persisted queries

A free query is refused:

```json
{"errors":[{"message":"This endpoint does not allow freeform GraphQL requests; operations must be sent by ID in the persisted queries GraphQL extension.","extensions":{"code":"PERSISTED_QUERY_ID_REQUIRED"}}]}
```

So no introspection and no custom queries. Only the operations in the site safelist, with exactly the fields they ask for.

### Where the operations come from

- The site has the whole safelist in one JS chunk (`/_next/static/chunks/0hp8ne_u7o480.js` in build `acab248`) as `JSON.parse('{"format":"apollo-persisted-query-manifest","version":1,"operations":[…]}')`.
- Each entry is `{ id, name, type, body }`, and `id` = sha256 of `body` as written in the manifest. Checked for all 164 operations, and it matches the 7 hashes the browser sent.
- 164 operations: 124 queries, 40 mutations. Most are for other products (see the end of this doc).
- The hashes change when Riot changes a query (new build). When a call fails with an unknown id, record a new HAR (filter "All") and run `gql_tool.py extract` again. `apollographql-client-version` says which build you're on.

> The other hashes next to each query in the bundle (`__meta__.hash`) are not the ids the server wants. Use the manifest.

Find the manifest by hand (Firefox):

1. Open `https://lolesports.com/en-US/`, F12 → Network, filter `gql`, reload. Open a request (e.g. `homeLeagues`) and copy the start of its `sha256Hash`.
2. Debugger → Ctrl+Shift+F (search in all files) → paste the hash. The match is in a `_next/static/chunks/*.js` file, that's the manifest chunk.
3. `{}` to pretty print it. It has `JSON.parse('{"format":"apollo-persisted-query-manifest",…}')`.
4. Save it (right click → Copy link → `curl -o chunk.js '<link>'`, or a HAR with filter "All"), then `python3 gql_tool.py extract chunk.js -o manifest.json`.
5. Check: `printf '%s' "<body>" | sha256sum` gives the `id`.

If the search finds nothing the chunk isn't loaded yet, open a page that makes GraphQL calls (home, schedule) and search again. The chunk is not in the page HTML: a script that is there (`3ui5kucp_icww.js` in build `acab248`) loads it ~3 s after the page opens.

Other ways:

- Firefox Network search: tick *Disable Cache*, F5, wait ~5 s, magnifying glass in the filter bar (searches response bodies) → `apollo-persisted-query-manifest`.
- From a HAR: `gql_tool.py extract capture.har` prints the URL of the file where it found the manifest.
- Command line, following both loading steps:

  ```bash
  cd "$(mktemp -d)"   # the downloads go to a temp folder, not the repo
  B=https://lolesports.com
  curl -s "$B/en-US/" -o page.html
  grep -o 'static/chunks/[A-Za-z0-9_-]*\.js' page.html | sort -u > list1.txt
  for c in $(cat list1.txt); do curl -s "$B/_next/$c" -o "$(basename "$c")"; done
  grep -oh 'static/chunks/[A-Za-z0-9_-]*\.js' *.js | sort -u | comm -23 - list1.txt > list2.txt
  for c in $(cat list2.txt); do curl -s "$B/_next/$c" -o "$(basename "$c")"; done
  grep -l 'apollo-persisted-query-manifest' *.js
  ```

### Caching

| Operation | `cache-control` |
|---|---|
| `GetSeasonForNavigation`, `homeLeagues`, `GetGprSeasons` | `max-age=60, stale-while-revalidate=60` |
| `homeEvents`, `scheduleEvents`, `watchLiveQuery` | `max-age=120, stale-while-revalidate=60`, also during a live match ([live check](2026-10-17-old-vs-new-api-comparison.md#caching)) |
| `GetTeamGpr` | `max-age=57, stale-while-revalidate=57` |
| `GetPickemsTournamentsSimple` | `max-age=30, stale-while-revalidate=58` |
| `GetOtherLeagues`, `watchVodQuery`, `GetLoLTeamOverview` with `teamId` | `max-age=60, stale-while-revalidate=60` |
| `GetLoLTeamOverview` without `teamId` (all teams, 1.7 MB) | `max-age=7, stale-while-revalidate=7` |

Netlify really serves stale copies (`cache-status: fwd=stale; ttl=-102` seen). Netlify's `Date` is the time of the answer to us, the data was made at `Date - Age` (CloudFront, old API: `Date` is when it was made). Data can be up to max-age + stale-while-revalidate old: ~3 min for `scheduleEvents`/`homeEvents`/`watchLiveQuery`, ~2 min for the old `getLive` (`max-age=60, stale-while-revalidate=60`, `age` up to 63 s seen). A `max-age` a bit under 60 (39, 49, 51) is just the CDN counting down.

### Calling it from a script

- Without the client headers it's rejected. In test 1 all 8 queries sent with only `apollographql-client-name` got HTTP 401:

  ```json
  {"errors":[{"message":"No client headers set"}]}
  ```

- Test 2: the request has to look like it comes from lolesports.com. `apollographql-client-version` alone still failed. With `Referer: https://lolesports.com/…` or `Origin: https://lolesports.com`, every try returned HTTP 200 with real data. Browser User-Agent and `Sec-Fetch-*` headers changed nothing.

  Minimum that works:

  ```
  apollographql-client-name: Esports Web
  apollographql-client-version: <build, e.g. acab248>
  Referer: https://lolesports.com/
  content-type: application/json
  ```

  Any lolesports.com page works as `Referer`, `https://lolesports.com/en-US/` was also checked.

  Not isolated: if the two `apollographql-*` headers are still needed with `Referer`. We keep sending them like the site does. Script answers are cached like the browser ones (same CDN, same data). `gql_tool.py` sends these headers by default.

- `hl=en-US` gives English region names (`NORTH AMERICA`, `BRAZIL`, `EMEA`, `INTERNATIONAL`).
- `homeLeagues` without `flags` also returns leagues with `displayPriority.status: not_selected` (DCGI, WSCI…) and `hidden`.
- `watchLiveQuery` with `eventState: ["inProgress"], eventType: "all"` works: `events: []` when nothing is live, and the live match with the same data as the old `getLive` during FUR vs C9 on 2026-10-16 ([live check](2026-10-17-old-vs-new-api-comparison.md)). Not known yet: which variables the live page sends.
- Live in-game stats: no operation in the manifest returns frames, gold or kills. The schema has in-game enums (dragon types, monster types, lanes), so the backend models live games, but no persisted query uses them in this build. The only WebSocket in the bundles is Riot's messaging for rewards/drops (`/rms/v1/session`). The new site doesn't seem to show live in-game stats at all (seen 2026-10-16), so the only known source is the old feed, see [the old API investigation](2026-10-16-lolesports-old-api-and-live-feed.md#live-feed-feedlolesportscomlivestatsv1).

### Test 3: every mapped operation from a script (2026-10-16)

All 9 calls returned HTTP 200 with data (headers above, `hl=en-US`).

| # | Operation, variables | Result |
|---|---|---|
| 0 | `homeLeagues` (no flags) | 51 leagues: 3 `force_selected`, 6 `selected`, 33 `not_selected`, 9 `hidden` (LTA North/South/Cross, LLA, LCO, LCL, WQS, CACG, King's Duel) |
| 2 | `homeLeagues` + `flags: [excludeHidden, excludeWithoutTournaments]` | 41 leagues: hidden ones removed, plus `lcp_promotion` (no tournaments). "TFT Esports" comes with `sport: ["lol"]` |
| 3 | `GetOtherLeagues` (`locale`, `sport: "lol"`) | 51 leagues, 513 tournaments, 2011 → 2026, each with `season { id, name }`, `split { id }`, `stages { id, displayState }`. 8 old tournaments have no season/split. LCK CL has 14 tournaments |
| 4 | `GetLoLTeamOverview` without `teamId` | 1,586 teams, 963 with players, 5,431 player entries (4,526 different players, a player can be in more than one team), 849 with `homeLeague`. Roles: `top`, `jungle`, `mid`, `bottom`, `support`, `none` |
| 5 | `GetLoLTeamOverview` `teamId: [MVK]` | Team + 6 players `{ id, handle, summonerName, firstName, lastName, role, image }` + `homeLeague { id, slug, name, region }` |
| 1 | `scheduleEvents` `pageSize: 5` | 5 events centered on now (3 completed, 2 upcoming) + `pages { older, newer }` |
| 8 | `scheduleEvents` LCK CL, `eventState: ["completed"]`, `pageSize: 20` | 20 matches (Aug-Sep 2026), Bo3 and Bo5, `newer: null` (latest page) |
| 7 | `watchVodQuery` MVK-FUR | 5 games with `state`, `teams { id, side }` (unneeded games still have sides), 14 VODs per game (one per broadcast language: `provider`, `parameter`, `mediaLocale`, `startMillis`/`endMillis`), `matchTeams.record` = tournament W/L |
| 6 | `watchLiveQuery` `inProgress` | `events: []` (nothing live) |

Every completed match in tests 1 and 8 (25 matches, LCK CL included) is consistent: the winner has `ceil(count/2)` wins, completed games = sum of `gameWins`, the rest are `unneeded`, event `state` = match `state`. Some completed tier 2 games have no VOD (8 of 64 in LCK CL), so `vods` can be empty on a completed game.

Checked again from Triforce's environment on 2026-10-16 (evening):

- `homeLeagues` with both flags: HTTP 200, **42** leagues (one more than in test 2, the data changes).
- `GetLoLTeamOverview` without `teamId`: HTTP 200, 1.7 MB, 1,586 teams, 963 with players, 5,431 player entries, 4,526 different players, 849 with `homeLeague`. These are exactly the numbers the old API gave us the same day (`team`, `team_player` and `player` tables). Contents compared row by row on 2026-10-16, all equal ([comparison](2026-10-17-old-vs-new-api-comparison.md#data-comparison-our-db-vs-the-new-api)).

## Enums

From the schema in the bundles. Names are guessed from usage, values are exact.

| Enum | Values |
|---|---|
| EventState | `unstarted`, `inProgress`, `completed` |
| GameState | `unstarted`, `inProgress`, `completed`, `unneeded` |
| EventType | `all`, `match`, `show` |
| MatchStrategyType | `bestOf`, `drawableBestOf`, `playAll`, `placementPlayAll`, `placementCheckmate` |
| Match flags | `hasVod`, `hasRecap`, `isSpoiler`, `thirdPlace` |
| VodType | `main`, `recap` |
| Side | `blue`, `red` |
| Sport | `lol`, `val`, `tft`, `wr`, `rb` |
| LeagueFlags | `excludeHidden`, `excludeWithoutTournaments` |
| DisplayPriority status | `selected`, `not_selected`, `force_selected`, `hidden` |
| Section type | `bracket`, `group`, `crossGroup` |
| Destination type | `match`, `decisionPoint`, `standing` |
| Split region (seen in data) | `REGIONAL`, `GLOBAL` |
| Player role / position | `top`, `jungle`, `mid`, `bottom`, `support`, `none` |
| Stream provider | `youtube`, `twitch`, `kick`, `afreecatv`, `soop`, `riotesports`, `tencent`, `trovo`, `openrec`, `nimotv`, `mildom` |
| Dragon type (not used) | `chemtech`, `cloud`, `elder`, `hextech`, `infernal`, `mountain`, `ocean`, `unknown_dragon_type` |

Variable values the site really sent:

- `hl: "en-GB"` (any locale)
- `sport: ["lol"]` (or `"lol"` where the type is a single `Sport`)
- `flags: ["excludeHidden","excludeWithoutTournaments"]`
- `eventState: ["completed"]`, `["unstarted"]`, `["inProgress"]` or `["completed","unstarted"]`
- `eventType: "match"` or `"all"`
- `vodType: ["recap"]`
- `pageSize: 40` or `300`
- `eventDateStart` / `eventDateEnd` as ISO timestamps
- `leagues: [<league ids>]`

## Data notes

- Ids are the same as in the old API and the live feed: leagues, tournaments, matches, teams and games. E.g. MVK-FUR games `115660588583040711`, `115660588583171784`, `115660588583171785`, `116299649500204434` and `116299649500204435` are the same in `homeEvents` and the old `getEventDetails`.
- For `EventMatch`, the event `id` is the match id.
- `MatchTeam.id` is composite: `"<matchId>:<teamId>"`, e.g. `115660588583040710:107251245690956393`. Use `teamId` for the team.
- Events are a union: `EventMatch` (has `match`, `matchTeams`) and `EventShow` (broadcast without teams). Filter on `__typename` or `type`.
- `match.strategy { type, count }` is the best-of, e.g. `bestOf`/5.
- `match.games[] { id, number, state, vods, teams { id, side } }`, game states include `unneeded` like the old API.
- `pages { older, newer }` are base64 tokens (`older::<id>`) passed back as `pageToken`, same as the old `getSchedule`.
- Times are ISO datetimes (`startTime`/`endTime`), not dates.
- Ids are 18 digits (> 2^53): store them as strings or `i64`/`u64`, never as floats.

## Operations for LoL esports data

Full query text: `python3 gql_tool.py show manifest_acab248.json <name>`. "Seen" = real response captured in a HAR, "test 3" = called from a script with data.

### Leagues, seasons, tournaments

| Operation | Variables | Returns |
|---|---|---|
| `getLeagues` | `hl!`, `sport` | `leagues { id, name, slug, image }` |
| `homeLeagues` (seen, test 3) | `hl!`, `sport`, `flags` | `leagues { id, name, slug, image, region, displayPriority { position, status } }` |
| `SettingsLeagues` | `hl!`, `sport`, `flags` | `leagues { id, name, image, displayPriority { status } }` |
| `GetOtherLeagues` (test 3) | `locale!`, `sport` | `leagues { id, name, slug, image, darkImage, region, displayPriority, tournaments { id, name, slug, startTime, endTime, season { id, name, startTime, endTime }, split { id }, stages { id, displayState } } }`, every league with its tournaments in one call |
| `GetSeasonForNavigation` (seen) | `hl!`, `seasonId` | season → splits → tournaments → league |
| `Season` | `hl!`, `seasonIds` | seasons → splits → tournaments (description, awards, phases, players, teams with `homeLeague`, league) |
| `GetGprSeasons` (seen) | `hl!`, `sport` | `seasons { id, name, slug, sport, startTime, endTime, splits { id, name, slug, startTime, endTime } }`, all seasons |
| `GetSeasonTournaments`, `GetTournamentsForSchedule`, `GetSeasonLeagues`, `GetSplitLeague`, `GetSeason` | `hl!`, season id(s) | Lighter views of season → splits → tournaments |
| `GetSeasonTournament` | `hl!`, `tournamentId` | Tournament + `state`, `stages { sections { matches { id, state } } }`, `teams`, `scores`, `split` |

### Schedule, live, match details

| Operation | Variables | Returns |
|---|---|---|
| `scheduleEvents` (test 3) | `hl!`, `sport`, `leagues`, `leaguesExclude`, `eventDateStart`, `eventDateEnd`, `eventState`, `eventType`, `pageSize`, `pageToken`, `vodType` | `esports { events { … }, pages { older, newer } }`. `EventMatch { id, startTime, state, type, blockName, league, tournament { id, name }, streams, match { id, state, type, flags, strategy, games { id, number, state, vods, recaps } }, matchTeams { teamId, code, name, image, homeLeague { regionSlug }, result { gameWins, outcome } } }` |
| `homeEvents` (seen) | Same, plus `tournamentIds`, minus `leaguesExclude` | Same fields as `scheduleEvents` |
| `watchLiveQuery` (test 3, empty) | `hl!`, `sport`, `eventState`, `eventType`, `pageSize` | `EventMatch { id, startTime, blockName, league (+region), tournament { id }, streams, match { id, strategy, games { id, number, state, teams { id, side } }, matchTeams { id, code, name, result, record } } }`. No `teamId` here: split the composite `id` or use `games.teams.id` |
| `watchSingleLiveEventByLeagueQuery` | `hl!`, `leagueId!`, `eventState`, `eventType`, `pageSize` | Only event ids |
| `watchVodQuery` (test 3) | `hl!`, `eventId!`, `vodType` | `scheduledEvent`: like `watchLiveQuery` plus full VODs (`firstFrameTime`, `startMillis`, `endMillis`, `offset`…) per game. No event `state` in this query |
| `GetSeasonTournamentEvents` | `hl!`, `sport!`, `tournamentId!` | `events { match { id }, startTime }` |
| `GetTeamEvents` / `GetLoLTeamEvents` | `hl!`, `teamId`, `eventState`, `eventType`, dates, `pageSize` | A team's matches with results and records |

### Brackets and standings

| Operation | Variables | Returns |
|---|---|---|
| `GetSeasonStage` | `hl!`, `stageIds!`, `orderBy` | Stage → sections (`columns → cells → matches`, `standings`, `rankings`, destinations) |
| `GetSeasonTournamentDisplay` | `hl!`, `tournamentId!`, `orderBy` | The whole tournament with all stages, plus `tournamentResult { matchResults { id, winningTeamId } }` |

### Teams and players

| Operation | Variables | Returns |
|---|---|---|
| `GetLoLTeamOverview` (test 3, no `teamId` = all teams) | `hl!`, `sport`, `teamId` | `teams { id, slug, name, code, image, backgroundImage, homeLeague { id, name, region, slug, image }, players { id, summonerName, handle, firstName, lastName, role, image } }` |
| `GetTeams` | `hl!`, `sport`, `teamId` | `teams { id, slug, name, code, image, primaryColor, homeLeague { id, name, image }, players { id, handle, image, status, positions { name, slug }, homeTeam { id, slug } } }` |
| `SettingsTeams` | `hl!`, `sport` | `teams { id, code, image, status, homeLeague { name } }`, looks like all teams, no filter |

### Rankings (no old equivalent)

`GetTeamGpr` (seen, Global Power Rankings with history and per-tournament records), `GetLeagueElo`, `GetSuperlativeTeams`.

## Site page payloads (RSC)

The site is a Next.js App Router app on Netlify. In batch 1 the browser never called the old API. Besides GraphQL, pages get data as RSC: a GET on the page URL with header `RSC: 1` returns `text/x-component`. These are not cached (rendered on the server every time).

| Route | Has |
|---|---|
| `/{locale}/tournament/{tId}` | 307 to the default stage |
| `/{locale}/tournament/{tId}/stage/{stageId}` | `stages[]` with every match of the tournament as `{id, state}`, `Section` (bracket/group/crossGroup) with `standings`, `columns→cells→matches`, `rankings` |
| `/{locale}/tournament/{tId}/overview` | Format text and teams |
| `/{locale}/results/{tId}/stage/{stageId}` | Same, plus 32 secondary leagues and Swiss rounds |
| `/{locale}/season/{seasonId}/handbook/…` | Rules text |

Match in these payloads: `{ id, state, startTime?, description ("R1M2", "QF1"…), structuralId, flags, matchTeams[] { teamId, code, name, result { outcome, gameWins } | null }, destinations { win, loss } }`. No best-of and no games.

Parsing:

- One row per line, `<hex id>:<payload>`.
- `I[…]` is a component import.
- `T<hexlen>,<text>` is a long text block, read it by length, it can have newlines.
- The rest is JSON. `"$undefined"` means undefined, `"$L17"` is a component and `"$<row>:<path>"` is a reference.
- Payloads repeat objects, sometimes as stubs `{id, state}`, with references (`"$5:1:props:…"`). Don't follow the React tree (it changes with every redesign): walk the JSON, keep every object with a `__typename` and merge by id. `har_explorer.py` does this.

## Everything else in the manifest (not for Triforce)

- Pick'em and Crystal Ball: `GetPickems*`, `GetCrystalBall*`, `Make*Picks` mutations
- Drops, rewards, factions: `GetDrops*`, `GetEarnedDrop`, `GetFactions*`, `JoinFaction`
- Compete (amateur/Riftbound/TFT tournaments, organizers): `GetCompete*`, `GetOrganizer*`, `SubmitGameResults`…
- VALORANT and TFT: `GetVal*`, `GetTft*`, `val*`
- CMS content: `ALlArticle`, `ArticlePage`, `NewsList`, `lolHomepageWidgets`, sponsors
- Settings: `GetRewardsOpting`, `UpdateRewardsOpting`

Mutations need a logged-in Riot account. `gql_tool.py call` refuses to send them.
