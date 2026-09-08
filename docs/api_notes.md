# rating.chgk.info public read API — field reference

Base URL: `https://api.rating.chgk.info`

The API is built on API Platform (Symfony) and exposes both a plain JSON
representation (default) and a JSON-LD / Hydra representation. The OpenAPI
3.1 document is served from `/docs`, but only under its own MIME type
(anything else is a 404/406):

```
GET https://api.rating.chgk.info/docs
Accept: application/vnd.openapi+json
```

The Hydra documentation is at `/docs.jsonld` with `Accept:
application/ld+json`.

## Authentication — what the spec says vs. what works

The OpenAPI document declares `bearerAuth` (JWT) on **all 56 operations**,
so its security flags carry no information. Probed anonymously on
2026-09-04: **every `GET` answers 200** except `/users/test` (401). The
token endpoint `POST /authentication_token` exists (an empty body gets
400, not 404), but it is not usable for our accounts, and the spec's
`POST`/`PATCH`/`DELETE` operations (create/edit players, teams, venues,
countries, seasons, venue types) are not usable either. Writes go
through the website with cookie sessions: roster/results use its separate
JSON endpoints (see `submission_api.md`); creation uses forms (`har_notes.md`).

## Endpoint coverage (anonymous GETs) → `ApiClient`

| Endpoint | Method |
|---|---|
| `/players/{id}` | `get_player` |
| `/players?surname&name` | `search_players`, `find_players` |
| `/players/{id}/seasons` | `get_player_seasons` |
| `/players/{id}/tournaments` | `get_player_tournaments` |
| `/teams/{id}` | `get_team` |
| `/teams?name` | `search_teams`, `find_teams` |
| `/teams/{id}/seasons` | `get_team_seasons`, `get_base_roster` |
| `/teams/{id}/tournaments` | `get_team_tournaments` |
| `/towns?name`, `/towns/{id}` | `search_towns`, `lookup_town_id`, `get_town` |
| `/countries?name`, `/countries/{id}` | `search_countries`, `lookup_country` (both return `Country { id, name }`), `get_country` |
| `/regions` | `list_regions` |
| `/venues?name`, `/venues/{id}` | `search_venues`, `get_venue` (`get_venue_town` is deprecated) |
| `/venues/{id}/requests` | `get_venue_requests`, `list_synch_tournaments_for_venue` |
| `/venue_types` | `list_venue_types` |
| `/tournaments` (filters below) | `search_tournaments` |
| `/tournaments/{id}` | `get_tournament` |
| `/tournaments/{id}/requests` | `get_tournament_requests` |
| `/tournaments/{id}/results` | `get_tournament_results`, `get_tournament_results_with`, `get_tournament_controversials` |
| `/tournaments/{id}/appeals` | `get_tournament_appeals` |
| `/tournaments/{id}/intersections` | `get_tournament_intersections` |
| `/tournament_synch_requests/{id}` | `get_synch_request` |
| `/tournament_synch_controversials/{id}` | `get_controversial` |
| `/tournament_team_flags` | `list_team_flags` |
| `/seasons`, `/seasons/{id}` | `list_seasons`, `get_season` |
| `/releases`, `/releases/{id}` | `list_releases`, `get_release` |
| `/languages` | `list_languages` |
| `/users`, `/users/{id}` | not wrapped on purpose (the user table, anonymously readable) |
| `/tournament_synch_appeals/{id}` | not wrapped: `/tournaments/{id}/appeals` returns the same records for a whole tournament |
| `/regions/{id}`, `/languages/{id}`, `/venue_types/{id}`, `/tournament_team_flags/{id}` | not wrapped; the list endpoints cover them |
| `/synch_tournaments`, `/regular_tournaments`, `/merger_tournaments` | not in the OpenAPI document (legacy paths); `/tournaments` covers them |

The entrypoint listing all collections is:

```
GET https://api.rating.chgk.info/
Accept: application/ld+json
```

## Content negotiation

* Default `Accept: application/json` (or no Accept header) returns a flat
  JSON array (collections) or object (single resources). No Hydra envelope.
* `Accept: application/ld+json` returns the Hydra envelope:
  ```json
  {
    "@context": "/contexts/Foo",
    "@id": "/foos",
    "@type": "Collection",
    "totalItems": 4429,
    "member": [ ... ],
    "view": {
      "@id": ".../foos?itemsPerPage=1&page=1",
      "@type": "PartialCollectionView",
      "first": "...?page=1",
      "last":  "...?page=4429",
      "next":  "...?page=2"
    }
  }
  ```
  Use this representation when you need `totalItems` or want to jump to the
  last page of a request list (those have no ordering parameter — see
  below).

## Pagination

* Query params: `page` (1-based) and `itemsPerPage`.
* Default page size appears to be 30. Maximum unknown but at least 100 works
  in practice.
* For total count and last-page URL, request with `Accept:
  application/ld+json` and read `totalItems` / `view.last`.

## Ordering

Default order is ascending primary key (`id`). The `order[...]`
parameters the spec advertises **do work** (re-verified 2026-09-04;
earlier versions of these notes said they were ignored):

* `/tournaments?order[id]=desc` and `order[lastEditDate]`;
* `/teams?order[name]=asc|desc` and `order[tournamentsPlayedB]`.

The request lists (`/venues/{id}/requests`, `/tournaments/{id}/requests`)
have no ordering parameter; they are always ascending by id. To get a
venue's newest requests either filter with `dateStart[after]` or fetch
all and sort client-side (what `list_synch_tournaments_for_venue` does).

## Filtering

The filters the spec advertises **do work** (re-verified 2026-09-04;
earlier versions of these notes said most were ignored). `itemsPerPage`
is honoured everywhere.

* Name filters — `/teams?name`, `/players?surname[&name]`, `/towns?name`,
  `/countries?name`, `/venues?name`, `/tournaments?name` — are
  **case-insensitive substring** matches (Cyrillic and Latin alike);
  `/players?surname=михайлюк` finds `Михайлюк`, `/venues?name=ако` finds
  `Балаково` and `Краков`. An unmatched name returns `[]`.
* `/tournaments`: `type` (2 очник, 3 синхрон, 6 строгий синхрон,
  8 асинхрон), `archive`, `dateStart[after|before|strictly_*]`,
  `dateEnd[...]`, `lastEditDate[...]`, `language`, `town`,
  `town.region`, `town.country`, `editor`, `ratingSystems`.
* `/teams`: `id[]` (several ids at once), `town`, `town.country`,
  `town.region`.
* `/towns`: `country`, `region`; `/regions`: `country`.
* `/venues/{id}/requests`, `/tournaments/{id}/requests`:
  `dateStart[...]`, `issuedAt[...]`.
* `/teams/{id}/seasons`, `/players/{id}/seasons`: `idseason`, `idplayer`,
  `idteam` (single or `[]`).
* `/tournaments/{id}/results`: see the results section.

There is still **no venue filter on `/tournaments`** — a venue's
tournaments come from `/venues/{id}/requests`.

---

## 1. Synchronous tournaments held at a specific venue (newest first)

### Endpoint (`/tournaments` has no venue filter)

```
GET /venues/{venueId}/requests?page={N}&itemsPerPage={K}
```

This returns the list of synch *requests* (заявки) the venue has filed,
each containing `tournamentId` and the actual local `dateStart` of the
playing at that venue. Every tournament that has at least one synch request
for `venueId` will appear here. Result class is `TournamentSynchRequest`
(see entity #3 below).

#### Verified probe

```
GET /venues/3030/requests?itemsPerPage=2
```

```json
[
  {
    "id": 1,
    "status": "A",
    "venue": { "id": 3030, "name": "Санкт-Петербург", ... },
    "representative": { "id": 14544, "name": "Константин", "patronymic": "Александрович", "surname": "Кноп", "gotQuestionsTag": 1637 },
    "narrator":       { "id": 14544, "name": "Константин", "patronymic": "Александрович", "surname": "Кноп", "gotQuestionsTag": 1637 },
    "issuedAt": "2011-09-20T18:55:38+04:00",
    "dateStart": "2011-10-16T00:00:00+04:00",
    "tournamentId": 1816
  },
  {
    "id": 266, "status": "A", "venue": {...},
    "representative": {...}, "narrator": {...},
    "issuedAt": "2011-12-07T23:28:17+04:00",
    "dateStart": "2011-12-11T14:00:00+04:00",
    "tournamentId": 1817
  }
]
```

`/venues/3030/requests` (Hydra view) reports `totalItems: 4429` for venue
3030 (St. Petersburg).

#### Reverse-chronological listing

There is no `order[...]` parameter on this endpoint. The list is in ascending request `id`,
which (because requests are filed in time order) is *almost* — but not
strictly — chronological by `dateStart`. The clean approach is:

1. `GET /venues/{id}/requests?itemsPerPage=1` with `Accept: application/ld+json`
   to learn `totalItems` and `view.last`.
2. Walk pages from the last page backwards with a reasonable
   `itemsPerPage` (e.g. 50).
3. Sort the in-memory page slice by `dateStart` descending if you need
   strict order (request `id` ordering and `dateStart` ordering can differ
   for the same venue).

`dateStart[after|before|strictly_after|strictly_before]` and
`issuedAt[...]` filters work here (verified 2026-09-04), so a recent window
can be requested directly; `list_synch_tournaments_for_venue` fetches all
pages and sorts client-side instead because it wants the newest 100
regardless of date.

#### Pagination

Standard `page` / `itemsPerPage`. `Accept: application/ld+json` exposes
`totalItems` and `view.{first,last,next,previous}`.

---

## 2. Tournament details (with rounds / questionQty)

### Endpoint

```
GET /tournaments/{id}
```

Works for all tournament subtypes (regular, synch, merger). For synch
tournaments you can equivalently `GET /synch_tournaments/{id}` (verified
9700 returns the same payload).

#### Verified probe — `GET /tournaments/9700`

```json
{
  "instantControversial": false,
  "dateArchivedAt": "2024-03-03T...",
  "dateDownloadQuestionsFrom": "...",
  "hideQuestionsTo": "...",
  "hideResultsTo": "...",
  "resultFixesTo": "...",
  "resultsRecapsTo": "...",
  "dateAppealAllowedTo": "...",
  "allowAppealCancel": true,
  "allowNarratorErrorAppeal": true,
  "archive": true,
  "dateRequestsAllowedTo": "...",

  "type": { "id": 3, "name": "Синхрон", "shortName": "С" },
  "id": 9700,
  "name": "Кубок Эквестрии — 17: January Joy (синхрон)",
  "longName": "",
  "lastEditDate":  "2024-...",
  "dateStart":     "2024-01-27T12:00:00+03:00",
  "dateEnd":       "2024-02-03T12:00:00+03:00",

  "paymentCategories": [...],
  "idseason": <int>,
  "orgcommittee": [...],
  "mainPayment": 700,
  "discountedPayment": 450,
  "discountedPaymentReason": "...",
  "currency": "RUB",
  "languages": [ { "id": "ru", "name": "Русский" } ],

  "editors": [...],
  "gameJury": [...],
  "appealJury": [...],

  "tournamentInRatingBalanced": true,
  "trueDL": ...,
  "difficultyForecast": ...,
  "maiiRating": false,
  "regulationsUrl": "...",

  "synchData": { ... same fields as the top-level synch fields, repeated ... },
  "ratingSystems": ["mak"],
  "rating": false,

  "questionQty": { "1": 12, "2": 12, "3": 12 }
}
```

### `questionQty` shape (load-bearing for the deserializer)

`questionQty` comes in **two shapes**: usually a JSON object whose keys are
stringified round numbers ("1", "2", ...) and whose values are integer
question counts — for tournament 9700: `{"1":12,"2":12,"3":12}` => 3
rounds, 12 questions each — but a plain array (`[12, 12, 12]`) has also
been observed (PHP serialises a list with consecutive 0-based keys as an
array). `api.rs::parse_question_qty` accepts both and sorts object keys
numerically. Number of rounds = number of entries.

For very old tournaments (e.g. id=1, year 2003) the field is `null` /
absent (verified 2026-09-03).

### Other tournament fields seen in the wild

Synch tournaments listing additionally contains: `archive`, `toursCanceled`
(array), `dbTags` (array), `lastRelease` (string IRI like `/releases/493`),
`season` (string IRI like `/seasons/2`), `appeals` (array), `requests`
(array — usually empty in the listing view), `teamsCount` (int), `tours`
(array), `resultsClosed` (bool), `masksClosed` (bool), `typeEnum` (int),
`maiiEpoch` (bool).

Pagination on `/tournaments` and `/synch_tournaments` is standard.

---

## 3. Synch request for a specific (tournament_id, venue_id)

There is no server-side combined filter that works. Two viable approaches:

### Approach A — via the venue (recommended)

```
GET /venues/{venueId}/requests
```

Walk pages and pick the entry whose `tournamentId == tournament_id`. There
is at most one approved request per (venue, tournament) pair in practice.

### Approach B — via the tournament

```
GET /tournaments/{tournamentId}/requests
```

Returns **all** venues' requests for that tournament, ignoring any
`venue.id=` query parameter. Filter client-side.

#### Verified probe — `GET /tournaments/9700/requests` (one element)

```json
{
  "id": 135224,
  "status": "A",                           // "A"=Approved "C"=Canceled "D"=Declined "N"=New
  "venue": {
    "id": 3030,
    "name": "Санкт-Петербург",
    "town": { "id": 285, "name": "Санкт-Петербург",
              "region": { "id": 128, "name": "Санкт-Петербург",
                          "country": { "id": 21, "name": "Россия" } },
              "country": { "id": 21, "name": "Россия" } },
    "type": { "id": 1, "name": "Постоянная" },
    "address": "...",   // optional
    "urls": []
  },
  "representative": {
    "id": 12366, "name": "Никита",
    "patronymic": "Дмитриевич", "surname": "Ивинский",
    "gotQuestionsTag": 8106          // optional
  },
  "narrator": { "id": 12366, ... },  // same Player shape; can equal representative
  "approximateTeamsCount": 5,
  "issuedAt": "2024-01-18T14:06:55+03:00",
  "dateStart": "2024-01-29T19:30:00+03:00",   // <-- the venue-local play datetime
  "tournamentId": 9700
}
```

A single request can also be fetched directly by its own id at
`GET /tournament_synch_requests/{requestId}` (verified for id 135224).

`dateStart` carries an explicit timezone offset and should be parsed with
`chrono::DateTime<FixedOffset>` (or similar).

The Hydra class is `TournamentSynchRequest`. Status enum values
(documented in Hydra description):

| code | meaning |
|------|---------|
| `A`  | Approved (заявка одобрена) |
| `C`  | Canceled  |
| `D`  | Declined  |
| `N`  | New (pending) |

For "official datetime of the playing at this venue" you want
`status == "A"`.

---

## 4. Team search by name (autocomplete)

### Endpoint

```
GET /teams?name={fragment}&itemsPerPage={N}
```

The `name` query parameter performs a case-insensitive substring match
against the team name (verified: `name=Несп` returns "Неспроста",
"Неспящие красавцы", "Неспортивное поведение", "Это несправедливо!").
A nonsense value returns `[]`.

#### Verified probe — `GET /teams?name=Неспроста&itemsPerPage=5`

```json
[
  {
    "id": 1,
    "name": "Неспроста",
    "town": {
      "id": 201, "name": "Москва",
      "region":  { "id": 90, "name": "Москва",
                   "country": { "id": 21, "name": "Россия" } },
      "country": { "id": 21, "name": "Россия" }
    },
    "country": { "id": 21, "name": "Россия" }
  },
  {
    "id": 33988, "name": "Неспроста",
    "town": { "id": 197, "name": "Минск", ... },
    "country": { "id": 5, "name": "Беларусь" }
  }
]
```

`itemsPerPage` caps the result count for autocomplete UIs. Pagination via
`page` works as on every collection.

---

## 5. Team by id

### Endpoint

```
GET /teams/{id}
```

#### Verified probe — `GET /teams/1`

```json
{
  "id": 1,
  "name": "Неспроста",
  "town": {
    "id": 201,
    "name": "Москва",
    "region": {
      "id": 90,
      "name": "Москва",
      "country": { "id": 21, "name": "Россия" }
    },
    "country": { "id": 21, "name": "Россия" }
  }
}
```

Note the listing variant `/teams?...` has a top-level `country` field as
well, but the single-item `/teams/{id}` does not — the country is only
available nested under `town.country` (and `town.region.country`).

---

## 6. Venues — verify a venue id exists

### List endpoint

```
GET /venues?page={N}&itemsPerPage={K}
```

### Lookup by id

```
GET /venues/{id}
```

#### Verified probe — `GET /venues/3030`

```json
{
  "id": 3030,
  "name": "Санкт-Петербург",
  "town": {
    "id": 285, "name": "Санкт-Петербург",
    "region":  { "id": 128, "name": "Санкт-Петербург",
                 "country": { "id": 21, "name": "Россия" } },
    "country": { "id": 21, "name": "Россия" }
  },
  "type": { "id": 1, "name": "Постоянная" },
  "urls": []
}
```

Venue payloads may also include an `address` string when set. `urls` is an
array of strings (often empty). `type.id` is the venue-type id; the lookup
table is `/venue_types`. Type 1 = "Постоянная" (permanent), type 4 =
"Разовая" (one-off), as observed in tournament-request payloads.

`GET /venues/{id}` returns `404` for nonexistent ids — use that as the
existence check for a configured `venue_id`.

---

## Tournament results (`/tournaments/{id}/results`)

`GET /tournaments/{id}/results` returns one row per team (all venues of a
synchronous tournament together). Query parameters (all verified
2026-09-04): `includeMasksAndControversials=1` adds `mask` and
`controversials`; `includeTeamMembers=1` adds `teamMembers` (player,
flag `К`/`Б`/`Л`, individual rating); `includeTeamFlags=1` adds `flags`
(short names of the зачёты); `includeRatingB=1` adds a `rating` object;
`venue`, `town`, `region`, `country`, `flag` (single or `[]`) filter the
rows. Sample row for tournament 14015, captured 2026-09-03 while the
results were **not yet published**:

```json
{
  "team": { "id": 86732, "name": "4:20", "town": { "id": 2088, "name": "Краков" } },
  "current": { "name": "4:20", "town": { "id": 2088, "name": "Краков" } },
  "mask": null,
  "questionsTotal": null,
  "position": null,
  "synchRequest": { "id": 189504, "venue": { "id": 3360, "name": "Краков" }, "tournamentId": 14015 },
  "controversials": []
}
```

* `mask` — `"110101…"`, one character per question across all rounds, in
  order. `null` until the tournament is past `dateDownloadQuestionsFrom`.
* `questionsTotal` (correct answers; a float in JSON) and `position` (ties
  give fractional values) — `null` until the results are published.
* `synchRequest` — absent for non-synchronous tournaments; its `venue.id`
  is how rows are attributed to a venue.
* `controversials` — full records: `{"id": 215378, "questionNumber": 26,
  "answer": "…", "issuedAt": …, "status": "D", "comment": null,
  "resolvedAt": null, "appealJuryComment": null}`; `status` is `A`
  (accepted), `D` (declined), otherwise pending. The same record is at
  `/tournament_synch_controversials/{id}`.
* `team.name` is the registered name; `current` is the team's present
  name/town (`current.name` is exposed as `TournamentResult::current_name`).

`/tournaments/{id}/results` for a nonexistent id is `404`.

---

## Quick Rust deserialization hints

* All datetimes are ISO-8601 with explicit offset (`+03:00`, `+04:00`,
  etc.). Use `chrono::DateTime<chrono::FixedOffset>` and call
  `.with_timezone(&Utc)` if you want UTC.
* `questionQty` => deserialise as `Option<serde_json::Value>` and accept
  both the object form (string keys = decimal round numbers) and the array
  form; see the tournament section above.
* Player surnames sometimes have an optional `patronymic`; `gotQuestionsTag`
  is also optional. Make those `Option<...>`.
* `Town`, `Region`, `Country` are deeply nested but uniform — define one
  struct each and reuse.
* The "list" representation of a synchronous tournament has many `[]`
  fields that are present but empty for older tournaments — type them as
  `Vec<T>` defaulting to empty.
* Some fields exist only on synch tournaments (whole `synchData` block);
  put behind `Option<SynchData>`.
* Request status is a single-letter enum: `A` / `C` / `D` / `N`. Model as
  an enum with `#[serde(rename = "A")]` etc.
* `tournament_synch_requests` resource id (the `id` field) is unique and
  globally addressable at `/tournament_synch_requests/{id}`.

## Other endpoints (shapes, verified 2026-09-04)

* `/tournaments/{id}` full object: `type` is an **object**
  `{"id": 3, "name": "Синхрон", "shortName": "С"}` (the spec says
  integer), plus `longName`, `dateEnd`, `lastEditDate`, `idseason`,
  `editors` / `gameJury` / `appealJury` (player objects), `languages`
  (`[{"id": "ru", "name": …}]`), `ratingSystems`, `archive`,
  `dateDownloadQuestionsFrom`, `dateRequestsAllowedTo`,
  `dateAppealAllowedTo`, `hideResultsTo`, `hideQuestionsTo`,
  `difficultyForecast`, `regulationsUrl`, `synchData`, `paymentCategories`.
* `/tournament_synch_requests/{id}` and the request lists: `{"id",
  "status" (A approved, N new, C cancelled, D declined), "venue" {…with
  town and type}, "representative" {player}, "narrator" {player} | null,
  "approximateTeamsCount", "issuedAt", "dateStart", "tournamentId"}`.
* `/tournaments/{id}/appeals`: `{"id", "idtournament", "type", "issuedAt",
  "status", "appeal", "comment", "answer", "questionNumber",
  "overriddenById"}`.
* `/teams/{id}/tournaments`: `{"idteam", "idtournament"}`, unpaginated
  (205 rows for team 86732 in one response). `/players/{id}/tournaments`:
  `{"idplayer", "idteam", "idtournament"}`, unpaginated.
* `/teams/{id}/seasons`, `/players/{id}/seasons`: `{"idplayer",
  "idseason", "idteam", "dateAdded", "dateRemoved", "playerNumber"}`,
  paginated.
* `/seasons` (47 rows, unpaginated; ids run into the future),
  `/releases` (paginated), `/languages` (`{"id": "ru", "name", "value"}`,
  unpaginated), `/venue_types` (4 rows), `/tournament_team_flags`
  (`{"id", "name": "SCHOOL", "value", "fullName", "shortName"}`, 58 rows in
  one unpaginated response),
  `/regions` (paginated, `country` nested).
* `/intersections`: tournaments sharing questions, as full tournament
  objects.

## What is *guessed* / not 100% verified

* The exact maximum allowed `itemsPerPage`. 100 works, larger values were
  not probed.
* `includeRatingB` output and `synchData` were seen but not modelled.
* `regular_tournaments` and `merger_tournaments` collections exist but
  were not probed; for *reading* an arbitrary tournament prefer the
  unified `/tournaments/{id}` endpoint.
