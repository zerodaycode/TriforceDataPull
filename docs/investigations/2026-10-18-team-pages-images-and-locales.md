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
