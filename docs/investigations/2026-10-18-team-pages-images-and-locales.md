# Team pages, images and locales

Date: 2026-10-18

What the new site shows on a team page, where the images come from, and how the API answers a locale it doesn't know. Sources: two HARs of team pages (`/en-GB/teams/karmine-corp`, `/en-GB/teams/fuego`, not in git), the dev DB and calls to `GetOtherLeagues`.

## Team pages

- The page is rendered on the server. The team, its roster and its next matches are in the HTML, there is no GraphQL call for them. The only GraphQL call is `GetSeasonForNavigation`.
- The team logo is `image`, the same value `GetLoLTeamOverview` sends. No other team image is on the page.

## Alternative team image

- The new API has no `alternativeImage`, and the site doesn't use one.
- The old API still has it and still updates it: 1,325 of 1,586 teams have one, the newest from 2026-10-06. 659 are newer than the team's `image`, 48 older, 336 are the same file.
- v2 takes it from the old `getTeams` while the old API lives (ADR 0004). An answer with no alternative image at all is ignored, so the stored ones stay.

## Player photos

A player without a photo doesn't come with `image` `null`. The API sends a placeholder:

| File | Players |
|---|---|
| `default-headshot.png` | 1,169 |
| `1643734200321_silhouette_transparent.png` | 311 |
| `download.png` (https) | 284 |
| `1633033031951_silhouette.png` | 193 |
| `placeholder.jpg` | 94 |
| `silhouette.png`, `download.png`, `placeholder.png`, `placeholder_.jpg`, `pobrane1.png`, `pobrane2.png`, `eSports_male_NoPlayer.png` | 5 to 39 each |

There are also 984 players with their own copy of a placeholder (`<timestamp>_silhouette.png`, `<timestamp>_placeholder.png`, `<timestamp>_EMPPhotoPlaceholderImage.png`).

v2 stores `NULL` when the file name is one of these (with or without the timestamp), when 3 or more players share the same URL, or when it is empty. A new shared URL is logged, so its name can be added to the list. After that, 1,374 of 4,526 players have a photo. Not checked: the image files themselves (`static.lolesports.com` wasn't reachable from where this was run), so a single `download.png` upload is kept.

## Locales

`GetOtherLeagues` with a locale the API doesn't know (`zz-ZZ`) answers 200, no `errors`, and the English texts. The headers are the same as for a real locale, nothing says it fell back. `es-ES`, `fr-FR` and `de-DE` come translated (`Mundial`, `Mondial`, `WM`).

v2 skips a locale whose names are all the same as `en-US`. Readers fall back to `en-US` anyway, so a real locale with English names (like `en-GB`) loses nothing.

## Results pages: stages, sections and teams

Sources: HARs of `/en-GB/results/<tournamentId>/stage/<stageId>` (DCGI 2026, LFL Invitational) and of `/en-GB/teams/french-flair`, not in git, plus calls to the API.

- The results pages are rendered on the server too, with no GraphQL call in the HAR. The page data has the same shape as the site queries `GetSeasonTournament` and `GetSeasonStage`, both in the manifest.
- `GetSeasonTournament(hl, tournamentId: [ID!])` takes a list of ids. 100 ids per call work, all 514 in one call fail with HTTP 414 (URL too long), so 6 calls cover every tournament. Same `cache-control: max-age=60` as the rest.
- Per tournament it sends:

| Field | What we got for the 514 tournaments |
|---|---|
| `stages { id name slug sections { name type matches { id state } } }` | 1,100 stages, all with a name (`Playoffs` 431, `Regular Season` 360, `Groups` 50, ...). 637 have a different name in `es-ES` (`Knockouts` → `Eliminatorias`). 1,315 sections (`bracket` 689, `group` 612, `crossGroup` 14) with 33,929 match ids |
| `split { id name region slug }` | Mostly ids, not names: `split_2`, `split_1`, `tier_2_leagues_2026`. Only 38 look like names (`Split 1`, `Split 3`). Only `Worlds` → `Mundial` is translated |
| `teams` (with `homeLeague`) | 263 tournaments list the teams that played it |
| `players` | Always empty |
| `awards`, `scores`, `phases`, `description` | Only 39 to 41 tournaments (international events with points) |

- `GetSeasonStage(hl, stageIds: [ID!]!)` sends the same stage with each section's standings, rankings and bracket matches (`matchTeams` with `teamId`, `result`, `destinations`).
- French Flair: the page shows the same 3 player photos we store, and `default-headshot.png` for the 2 we store as `NULL`. The logo is `image`, the same file we store as `alt_image_url`.
