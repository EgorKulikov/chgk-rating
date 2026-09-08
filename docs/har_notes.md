# rating.chgk.info Write-Side HAR Reference

Historical form reference for the Rust client that drives the **admin website** at
`https://rating.chgk.info` (NOT the read-only `api.rating.chgk.info`).

Roster/results submission now uses the website's JSON endpoints described in
`submission_api.md`. The CSV import and fix-form sections below document the
old workflow and the optional CSV helpers; they no longer describe
`SiteClient::upload_rosters` or `SiteClient::upload_results`.

The HAR captures this document is based on live in the `chgk_local`
project (they are not shipped with this crate — they contain a live
session). They were captured with Chrome DevTools while driving the site as
a logged-in administrator; later behaviour was verified by probing with the
client itself (dates noted inline). **Chrome strips `Cookie` request headers
and `Set-Cookie` response headers from HAR exports**, so the specific cookie
names aren't directly observable from the captures — they have to be
re-discovered empirically at runtime (see the Session/Cookie section).

---

## Base URL and general conventions

- Scheme / host: `https://rating.chgk.info` (HTTP/2, but the Rust client can
  use HTTP/1.1 — the server is nginx/1.18.0 behind a Symfony app).
- All "interesting" write requests are sent with `Origin:
  https://rating.chgk.info` and a `Referer` pointing at the tournament admin
  page (usually `https://rating.chgk.info/tournament/<id>` or the legacy
  `https://rating.chgk.info/tournaments.php?displaytournament=<id>`).
- AJAX endpoints expect `X-Requested-With: XMLHttpRequest` and `Accept: */*`.
- Form bodies are either
  `application/x-www-form-urlencoded; charset=UTF-8` (AJAX) /
  `application/x-www-form-urlencoded` (full-page form POSTs) / or
  `multipart/form-data` (file uploads).
- Russian strings in form data are percent-encoded UTF-8. Spaces encoded as
  `+`.

---

## Session / cookie model

Symfony security firewall with `_remember_me` — so two cookies matter:

- **`PHPSESSID`** (session cookie) — set on first contact, replaced on
  successful login.
- **`REMEMBERME`** (persistent cookie) — set by the login POST because the
  login form submits `_remember_me=on`. With this cookie alone the session
  can be re-hydrated on later requests.

Both cookies are stripped from the HAR exports; confirm at runtime by
inspecting `Set-Cookie` on the login response. A cookie jar (e.g.
`reqwest_cookie_store`) is sufficient — every subsequent request should just
replay whatever cookies the login response set.

---

## CSRF model

Symfony's built-in per-form CSRF tokens. There is **no site-wide
`X-CSRF-Token` header.** Tokens are embedded as hidden inputs in the HTML of
each form page.

Observed:

- Login form (`GET /login`): hidden `<input name="_csrf_token">` inside the
  form. Captured value from `login.har`:
  `<csrf token>`
- The tournament admin page (`GET /tournament/<id>`) hosts the
  `/player/create`, `/teams/create`, `/tournaments.php?displaytournament=<id>`
  (roster and positions upload), and `/result/submit` forms. **None of these
  forms carry a CSRF token in the rendered HTML** (verified by grepping the
  recorded HTML of `GET /tournament/13114`). The AJAX/multipart endpoints
  rely purely on session cookie + same-origin checks.

Consequence for the Rust client:

1. `GET /login` to obtain `_csrf_token` (scrape the hidden input).
2. `POST /login` with that token to obtain session cookies.
3. Everything else is cookie-only; no additional token scraping is required
   for player/team creation, roster uploads, or result uploads.

---

## 1. Login — `login.har`

### 1.1 `POST https://rating.chgk.info/login`

Full-page form submit. This is the request that exchanges credentials for
session cookies.

Headers (relevant):

| Header | Value |
|---|---|
| `Content-Type` | `application/x-www-form-urlencoded` |
| `Origin` | `https://rating.chgk.info` |
| `Referer` | `https://rating.chgk.info/login` |
| `Accept` | `text/html,application/xhtml+xml,application/xml;q=0.9,...` |
| `Upgrade-Insecure-Requests` | `1` |

Body (form-encoded, verbatim from HAR):

```
_csrf_token=<csrf token>&_username=%3Cadmin%20e-mail%3E&_password=<redacted>&_remember_me=on&go=%D0%92%D1%85%D0%BE%D0%B4
```

Fields:

| Name | Value | Notes |
|---|---|---|
| `_csrf_token` | opaque string | Scraped from `GET /login` hidden input. |
| `_username` | `<admin e-mail>` (URL-encoded) | The account's login e-mail. |
| `_password` | cleartext | (The real value is in the HAR; rotate it.) |
| `_remember_me` | `on` | Required for the `REMEMBERME` cookie. |
| `go` | `Вход` (URL-encoded as `%D0%92%D1%85%D0%BE%D0%B4`) | Submit button; Symfony doesn't care but some setups do. Safe to include. |

Response: **302** to `Location: /` (`Content-Type: text/html; charset=utf-8`,
empty body). On success, Symfony issues `Set-Cookie: PHPSESSID=...` (and
`REMEMBERME=...` because `_remember_me=on`). On failure, Symfony redirects
back to `/login` with a flash; detect by inspecting the redirect target and
the post-redirect HTML for either the logout link or the login form. The
post-login `GET /` in the HAR confirms login via the presence of these
elements:

```html
<span class="no-display" id="rt_user_email">&lt;admin e-mail&gt;</span>
<span class="no-display" id="rt_user_idplayer">17230</span>
<a class="dropdown-item" href="/logout">Выход</a>
```

### 1.2 Prerequisite: `GET https://rating.chgk.info/login`

Not captured in `login.har` (the HAR starts at the POST), but the form with
the `_csrf_token` hidden input lives there. The Rust client must:

1. `GET /login` (no auth).
2. Parse the HTML and extract `input[name=_csrf_token]`'s `value` attribute.
3. Use it in the `POST /login` body.

---

## 2. Create player — `create_player.har`

### 2.1 `POST https://rating.chgk.info/player/create`

AJAX call fired by `#new-player-form` on the tournament admin page
(`<form id="new-player-form" action="/player/create">`). Same-origin fetch.

Headers:

| Header | Value |
|---|---|
| `Content-Type` | `application/x-www-form-urlencoded; charset=UTF-8` |
| `Accept` | `*/*` |
| `Origin` | `https://rating.chgk.info` |
| `Referer` | `https://rating.chgk.info/tournament/13321` (any tournament page works; this just needs to be same-origin) |
| `X-Requested-With` | `XMLHttpRequest` |

Body (verbatim):

```
surname=%D0%A2%D0%B5%D1%81%D1%82%D0%BE%D0%B2&name=%D0%A2%D0%B5%D1%81%D1%82&patronymic=%D0%A2%D0%B5%D1%81%D1%82%D0%BE%D0%B2%D0%B8%D1%87
```

Fields:

| Name | Required | Notes |
|---|---|---|
| `surname` | yes | Last name, UTF-8 percent-encoded. |
| `name` | yes | First name. |
| `patronymic` | no (but always sent) | Middle name / отчество. Empty string is OK; absent is OK. |

**No CSRF token. No tournament id.** The player is created at the global
scope.

Response: `200 application/json`, body exactly:

```
{"success":true}
```

**Important gotcha:** the response **does not contain the new player's
`id`.** The site relies on a subsequent dropdown refresh to pick up the new
record. To discover the new player id from Rust you will have to either:

- Re-query `/player/search` or `/player/list` (TBD — not captured here), or
- Query the public `api.rating.chgk.info/players?name=...&surname=...` and
  take the newest match, or
- Wait for the user to refresh the dropdown and capture that endpoint.

Flagging this for follow-up; a fresh HAR that records the dropdown refresh
after a create would pin down the lookup endpoint.

---

## 3. Create team — `create_team.har`

### 3.1 `POST https://rating.chgk.info/teams/create`

AJAX call from `#new-team-form` (`<form id="new-team-form"
action="/teams/create">`) on the tournament admin page.

Headers: same set as `create_player` (XHR, same referer pattern).

| Header | Value |
|---|---|
| `Content-Type` | `application/x-www-form-urlencoded; charset=UTF-8` |
| `Accept` | `*/*` |
| `Origin` | `https://rating.chgk.info` |
| `Referer` | `https://rating.chgk.info/tournament/13321` |
| `X-Requested-With` | `XMLHttpRequest` |

Body (verbatim):

```
name=%D0%A2%D0%B5%D1%81%D1%82%D0%BE%D0%B2%D0%B0%D1%8F+%D0%BA%D0%BE%D0%BC%D0%B0%D0%BD%D0%B4%D0%B0&new-team-town=449
```

Fields:

| Name | Required | Notes |
|---|---|---|
| `name` | yes | Team name, UTF-8 percent-encoded. Spaces encoded as `+`. |
| `new-team-town` | yes | Numeric **town id** (here `449`). This is the internal CHGK town id that the public API exposes under `/towns`. |

Response: `200 application/json`, body **exactly**:

```
[]
```

Same gotcha as `create_player`: the server does not return the new team id.

**Unresolved:** the client (`SiteClient::create_team`) requires
`{"success":true}` like `create_player` does, which contradicts the `[]`
above. One of the two is stale; re-probe before relying on the error.
Plan on a follow-up lookup (likely `api.rating.chgk.info/teams?name=...`).

---

## 4. Upload rosters — `upload_rosters.har`

**Important:** the HAR only captures the **second step** of the two-step
rosters upload flow. The actual multipart file upload (`POST
/tournaments.php?displaytournament=<id>` with the spreadsheet) was performed
in an earlier browser session and is **not** in this HAR. Everything below
about "step A" is reconstructed from the HTML of the admin page, which
**is** in the HAR.

### 4.1 Step A — multipart file upload (NOT captured, inferred from HTML)

Form on the tournament admin page:

```html
<form action="/tournaments.php?displaytournament=13114"
      enctype="multipart/form-data" method="post">
    <input type="hidden" name="add_with_request_id"/>
    Загрузка команд турнира (файлы CSV, XLSX, ODS):
    <input type="file" name="file" accept=".csv, .xlsx, .ods" class="new_file_input"/>
    <input type="submit" class="btn btn-light" name="import_teams"
           value="Импортировать" id="import_teams"/>
</form>
```

Request (reconstructed):

- Method: `POST`
- URL: `https://rating.chgk.info/tournaments.php?displaytournament=<tournamentId>`
- `Content-Type`: `multipart/form-data; boundary=...`
- `Referer`: `https://rating.chgk.info/tournament/<tournamentId>` (or the
  legacy `/tournaments.php?...` URL).
- Multipart fields:
  - `add_with_request_id` — empty string (hidden input, no value attribute).
  - `file` — the `.xlsx`/`.csv`/`.ods` file. Content-Type for xlsx is
    `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet`;
    for csv use `text/csv`; for ods
    `application/vnd.oasis.opendocument.spreadsheet`.
  - `import_teams` — `Импортировать` (submit-button field — the server
    dispatches on `import_teams` being present, so **include it**).

Response (verified live 2026-09-02 against T14015 as the Krakow
representative): always HTTP 200 with the tournament page, in one of
three shapes —
1. Success: the page contains the flash line
   `Импорт завершился успешно, ошибок не найдено!`.
2. Name/id mismatch: the page contains the "Ошибки команд" fix form with
   hidden `idimport=<int>`, hidden `add_with_request_id=<venue request id>`
   (filled in here, unlike the plain upload form), and a row per team with
   `team_<hex>_{idteam,name,town,action}` fields. That is the form
   captured in step B below. Radio `_action` values: `<idteam>` (use that
   existing team), `create`, `data` (fix in import),
   `change_name_on_tournament` (one-off name), `noop`.
3. **Silent drop**: neither of the above — the importer threw the file away
   without saving anything or saying so. Trigger: a row whose team name
   contains a `"` character AND differs from the team's registered name
   (so it would need the fix form). Verified on team 107740 (registered
   as `"Ладно, погнали втроём сегодня"`, quotes included):
   - CSV, RFC-doubled `"""…втроём…"""` (matches) → success;
   - CSV / XLSX with `"…вчетвером…"` (quoted, mismatch) → silent drop;
   - XLSX with `…вчетвером…` (unquoted, mismatch) → normal fix form.
   So the CSV parser is fine (PhpSpreadsheet — an unreadable file yields
   the visible `Ошибка импорта: Unable to read data from {$pFilename}`);
   the fix-form path crashes on quotes. One bad row kills the whole file.
   Hence `sanitize_roster_rows` strips quotes only for names that differ
   from the registered one, and `upload_rosters` treats shape 3 as an
   error. A fourth shape — a visible `Ошибка импорта: …` line, e.g. for an
   unreadable file — is reported as `Error::Site` with that text.

### 4.2 Step B — confirm / resolve duplicates (captured)

`POST https://rating.chgk.info/tournament/13114`

Full-page form POST. This is a **per-tournament** request — one per
spreadsheet upload, resolving ALL teams in that spreadsheet in a single
batch.

Headers:

| Header | Value |
|---|---|
| `Content-Type` | `application/x-www-form-urlencoded` |
| `Origin` | `https://rating.chgk.info` |
| `Referer` | `https://rating.chgk.info/tournaments.php?displaytournament=13114` |
| `Accept` | `text/html,application/xhtml+xml,...` |
| `Upgrade-Insecure-Requests` | `1` |

Body (verbatim, line-wrapped for reading — the real body has no newlines):

```
idimport=192317
&fix_in_import=true
&add_with_request_id=
&team_00000000000016700000000000000000_idteam=54151
&team_00000000000016700000000000000000_name=%D0%99%D0%BE%D1%82%D0%B0+%D0%9A%D0%B8%D0%BB%D1%8F
&team_00000000000016700000000000000000_town=%D0%A1%D0%B1%D0%BE%D1%80%D0%BD%D0%B0%D1%8F
&team_00000000000016700000000000000000_action=change_name_on_tournament
&team_00000000000016e70000000000000000_idteam=96498
&team_00000000000016e70000000000000000_name=Sechs+im+Weggla
&team_00000000000016e70000000000000000_town=%D0%9D%D1%8E%D1%80%D0%BD%D0%B1%D0%B5%D1%80%D0%B3
&team_00000000000016e70000000000000000_action=change_name_on_tournament
&team_00000000000017050000000000000000_idteam=87944
&team_00000000000017050000000000000000_name=%D0%A0%D1%8B%D0%B1%D0%B0-%D1%87%D0%B0%D1%81%D1%8B
&team_00000000000017050000000000000000_town=%D0%A1%D0%B1%D0%BE%D1%80%D0%BD%D0%B0%D1%8F
&team_00000000000017050000000000000000_action=change_name_on_tournament
&team_000000000000170c0000000000000000_idteam=91036
&team_000000000000170c0000000000000000_name=%D0%A0%D0%B5%D1%86%D0%B5%D1%81%D1%81%D0%B8%D0%B2%D0%BD%D1%8B%D0%B9+%D0%B4%D0%BE%D0%BC%D0%B8%D0%BD%D0%B0%D0%BD%D1%82
&team_000000000000170c0000000000000000_town=%D0%A1%D0%B1%D0%BE%D1%80%D0%BD%D0%B0%D1%8F
&team_000000000000170c0000000000000000_action=change_name_on_tournament
```

Top-level fields:

| Name | Value | Notes |
|---|---|---|
| `idimport` | `192317` | **Opaque server-allocated import id** — returned in the HTML of step A. The Rust client must scrape it out of the "fix_in_import" form returned by step A. |
| `fix_in_import` | `true` | Literally the string `true`. |
| `add_with_request_id` | empty in this 2024 capture; filled with the venue request id in the 2026-09-02 probe (§4.1) | Echo back whatever the fix form carries, empty or not. |

Per-team fields (repeated for each team the server wasn't able to
auto-match). Prefix is `team_<hexkey>_` where `<hexkey>` is a 32-char
hex identifier the server assigns in step A; you **must** echo whatever
prefix the step-A HTML gave you — do NOT invent new keys. The suffixes
are:

| Suffix | Meaning |
|---|---|
| `_idteam` | Numeric team id the admin picked (from autocomplete / lookup). |
| `_name` | Team name as it appears for this tournament (may differ from the canonical `name` of `idteam`). |
| `_town` | Town **name** (string, not id — e.g. `Нюрнберг`, `Сборная`). |
| `_action` | What to do. The client sends `change_name_on_tournament`; the full radio set is listed in §4.1 (`<idteam>`, `create`, `data`, `change_name_on_tournament`, `noop`). |

Response: `302`, `Location: /tournament/13114` (the admin returns to the
clean tournament page). The follow-up `GET /tournament/13114` and `GET
/tournament/13114/results?_=<ts>` in the HAR are just the browser re-loading
that page and the team list — they are **not** required for the roster
upload itself.

### Roster upload is batch, not per-team

Both step A (multipart file) and step B (fix_in_import POST) are **one
request per tournament**, covering all teams in the spreadsheet. The Rust
client should NOT loop over teams making individual calls for the roster
import.

---

## 5. Upload results — `upload_results.har`

**The HAR does not contain the actual upload POST.** It only contains a
`GET /tournament/13114` page load and a `GET /tournament/13114/results?_=<ts>`
AJAX team list fetch. The upload itself was presumably performed in an
earlier session.

The upload form IS in the HTML of the captured `GET /tournament/13114`,
and that's enough to reconstruct the request.

### 5.1 `POST https://rating.chgk.info/result/submit` (multipart, reconstructed)

Form on the tournament admin page:

```html
<form action="/result/submit" enctype="multipart/form-data" method="post"
      id="submit_result_new_form">
    <input type="hidden" name="add_with_request_id"/>
    <input type="hidden" name="tournament_id" value="13114">
    <input type="file" name="file" accept=".csv, .xlsx, .ods" class="new_file_input"/>
    <input type="submit" class="btn btn-light" value="Импортировать"/>
</form>
```

Request:

- Method: `POST`
- URL: `https://rating.chgk.info/result/submit`
- `Referer`: `https://rating.chgk.info/tournament/<tournamentId>`
- `Origin`: `https://rating.chgk.info`
- `Content-Type`: `multipart/form-data; boundary=...`
- Multipart fields:
  - `add_with_request_id` — empty string.
  - `tournament_id` — the tournament id as string (e.g. `13114`).
  - `file` — the spreadsheet (`.csv`/`.xlsx`/`.ods`) containing team results
    per question.

Response: not captured. By analogy with the roster upload, expect either a
302 redirect back to the tournament page on success, or an HTML page with a
follow-up form on conflicts. **Watch for this at runtime and capture a
fresh HAR if the response surprises us.**

### Results upload is batch, not per-team

Same as rosters — one multipart POST per tournament uploads all teams'
results at once. The `tournament_id` is the only per-tournament scoping.

### 5.2 Related: "positions" upload (same page, separate form)

Not captured either, but listed here because it's the third file-upload
form on the same page and the Rust client may eventually need it:

```html
<form action="/tournaments.php?displaytournament=13114"
      enctype="multipart/form-data" method="post">
    <input type="hidden" name="add_with_request_id"/>
    Загрузка мест команд (файлы CSV, XLSX, ODS):
    <input type="file" name="file" accept=".csv, .xlsx, .ods"/>
    <input type="submit" name="import_positions" id="import_positions"/>
</form>
```

Same URL as the roster upload (`/tournaments.php?displaytournament=<id>`) but
dispatched by the submit-button field name `import_positions` instead of
`import_teams`. If we ever need to push final places separately from the
result upload, mirror the roster upload but use `import_positions` as the
discriminator.

---

## Summary table

| Action | Method | URL | Body type | Batch? | Response |
|---|---|---|---|---|---|
| Get CSRF | GET | `/login` | — | — | HTML with `_csrf_token` hidden input |
| Log in | POST | `/login` | form-urlencoded | — | 302 → `/` + `PHPSESSID`/`REMEMBERME` cookies |
| Create player | POST | `/player/create` | form-urlencoded (XHR) | per-player | `{"success":true}` (no id) |
| Create team | POST | `/teams/create` | form-urlencoded (XHR) | per-team | `[]` (no id) |
| Upload rosters — step A | POST | `/tournaments.php?displaytournament=<id>` | multipart (`file`, `import_teams`, `add_with_request_id`) | per-tournament | always 200 with the tournament page: success flash, fix form (`idimport` + `team_<hex>_*`), `Ошибка импорта`, or nothing (silent drop) — see §4.1 |
| Upload rosters — step B | POST | `/tournament/<id>` | form-urlencoded (`idimport`, `fix_in_import=true`, per-team fields) | per-tournament | 302 → `/tournament/<id>` |
| Upload results | POST | `/result/submit` | multipart (`file`, `tournament_id`, `add_with_request_id`) | per-tournament | not captured; the client treats a non-login page without `Ошибка импорта` as success |
| Upload positions | POST | `/tournaments.php?displaytournament=<id>` | multipart (`file`, `import_positions`, `add_with_request_id`) | per-tournament | not captured |

---

## Gaps / follow-up captures needed

1. ~~Raw login response `Set-Cookie` header~~ — verified at runtime: the
   site issues `PHPSESSID` and, with `_remember_me=on`, `REMEMBERME`; the
   client persists both.
2. **`create_player` / `create_team` success payload with IDs** — the HAR
   responses do not carry the new id. Capture the dropdown refresh request
   that happens right after (probably `/player/search?...` or similar) to
   see how the site looks up the fresh record.
3. ~~Step A of roster upload~~ — verified live on 2026-09-02, see §4.1
   (always 200; the fix form's field scheme and `_action` values are
   recorded there).
4. **Result upload response** — `upload_results.har` contains no POST at
   all. Re-capture to see the success/failure payload for `/result/submit`.
5. **Position upload** — not captured; reconstruct from the HTML only.

---

## Source captures

`login.har`, `create_player.har`, `create_team.har`, `upload_rosters.har`
and `upload_results.har`, kept privately in the `chgk_local` project (they
carry a live session and are not published).
