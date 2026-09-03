# rating.chgk.info public read API — field reference

Base URL: `https://api.rating.chgk.info`

The API is built on API Platform (Symfony) and exposes both a plain JSON
representation (default) and a JSON-LD / Hydra representation. There is
**no** Swagger UI / `openapi.json` reachable on this host. The discoverable
spec is the Hydra documentation at:

```
GET https://api.rating.chgk.info/docs.jsonld
Accept: application/ld+json
```

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
  last page (handy because ordering parameters are silently ignored — see
  below).

## Pagination

* Query params: `page` (1-based) and `itemsPerPage`.
* Default page size appears to be 30. Maximum unknown but at least 100 works
  in practice.
* For total count and last-page URL, request with `Accept:
  application/ld+json` and read `totalItems` / `view.last`.

## Ordering — IMPORTANT CAVEAT

`order[field]=asc|desc` query parameters are **silently ignored** on every
endpoint that was tested (`/synch_tournaments`, `/tournaments`, `/teams`,
`/venues/{id}/requests`, `/tournaments/{id}/requests`). The collection is
always returned in ascending primary-key (`id`) order.

To get newest-first you must either:

1. Fetch with `Accept: application/ld+json`, read `view.last`, then walk
   pages backwards (`page=last_page, last_page-1, ...`), reversing each
   page's contents in the client; **or**
2. Fetch all and sort client-side.

## Filtering — IMPORTANT CAVEAT

Filter query parameters using API Platform's usual `field=value`,
`field.subfield=value`, or `dateStart[after]=...` syntax are **silently
ignored** on the tested collection endpoints, with **two exceptions**:

* `/teams?name=<fragment>` — works (case-insensitive substring match).
* The dedicated subresource paths described below act as filters by their
  parent ID (e.g. `/venues/{id}/requests`).

So filtering `synch_tournaments` by venue does not work — you must walk the
venue's request subresource instead.

---

## 1. Synchronous tournaments held at a specific venue (newest first)

### Endpoint (do NOT try to filter `/synch_tournaments`)

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

`order[dateStart]=desc` is ignored. The list is in ascending request `id`,
which (because requests are filed in time order) is *almost* — but not
strictly — chronological by `dateStart`. The clean approach is:

1. `GET /venues/{id}/requests?itemsPerPage=1` with `Accept: application/ld+json`
   to learn `totalItems` and `view.last`.
2. Walk pages from the last page backwards with a reasonable
   `itemsPerPage` (e.g. 50).
3. Sort the in-memory page slice by `dateStart` descending if you need
   strict order (request `id` ordering and `dateStart` ordering can differ
   for the same venue).

There is no server-side date range filter that was found to work; do
date-window filtering client-side after fetching.

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

`questionQty` is a **JSON object whose keys are stringified round numbers
("1", "2", ...) and whose values are integer question counts**. It is
**not** an array. Number of rounds = `len(questionQty)`. For tournament
9700: `{"1":12,"2":12,"3":12}` => 3 rounds, 12 questions each.

For very old tournaments (e.g. id=1, year 2003) the field may be entirely
absent. Treat it as `Option<BTreeMap<String, u32>>` (or similar) in Rust.

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

## Quick Rust deserialization hints

* All datetimes are ISO-8601 with explicit offset (`+03:00`, `+04:00`,
  etc.). Use `chrono::DateTime<chrono::FixedOffset>` and call
  `.with_timezone(&Utc)` if you want UTC.
* `questionQty` => `Option<std::collections::BTreeMap<String, u32>>`. The
  string keys are decimal round numbers but are quoted in JSON.
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

## What is *guessed* / not 100% verified

* The exact maximum allowed `itemsPerPage`. 100 works, larger values were
  not probed.
* Whether `name=` on `/teams` is strictly substring vs. some token-based
  match. All probed examples behave like a case-insensitive substring on
  `name`.
* Whether any hidden ordering / filter params exist that the Hydra docs
  don't advertise — none found among the obvious API Platform conventions
  (`order[...]`, `dateStart[after|before]`, `field=`, `field.id=`).
* `regular_tournaments` and `merger_tournaments` collections exist but
  were not probed; for *reading* an arbitrary tournament prefer the
  unified `/tournaments/{id}` endpoint.
