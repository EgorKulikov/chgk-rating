# Roster and results submission

Contract checked against `idiotiqui/rating-site` commit `a7258ff6` (2026-09-08):
`site/src/Controller/RepresentativeResultsController.php`,
`site/src/Application/TournamentResultsQuery.php`, and
`site/src/Application/Tournament/ResultsEntry/TeamResultsWriter.php`.
These routes belong to `https://rating.chgk.info`, separate from the public
read API at `https://api.rating.chgk.info`.

## Authentication and scope

Use the same cookie session as the site. The representative endpoints resolve
the current user's approved representative/narrator requests and enforce
tournament permissions and deadlines. No JWT or extra CSRF field is needed
for these JSON requests. The client sends JSON with Origin and Referer headers.

The default scope acts as the logged-in player. Before writing, the client reads
the same endpoint to establish access (an empty roster GET is sufficient). If
that read returns HTTP 403, it tries `admin=1` and uses that scope for the write
only if the server authorizes the read. This supports organizers and regular
tournaments without a representative request. No write is retried. A new synch
tournament team still needs an unambiguous request; otherwise the server's
error is returned. Existing teams retain their request association.

## Rosters

`POST /api/tournament/{id}/representative/roster`:

```json
{
  "teams": [{
    "teamId": 9,
    "teamName": "Q&A \"2024\"",
    "rows": [
      {"playerId": 1, "flag": "К", "checked": true},
      {"playerId": 2, "flag": "Б", "checked": true},
      {"playerId": 3, "flag": "Л", "checked": true}
    ]
  }]
}
```

`upload_rosters(&api, tournament_id, rows)` groups rows by team and replaces
each included team's entire tournament roster. Players omitted from an included
team are removed from its tournament roster; other teams are not submitted or
deleted. Players and teams are identified by IDs. Player names and town strings
on `RosterRow` remain useful for CSV export but are not used to resolve IDs or
change the town in JSON submissions.

Every `Some(RosterFlag)` is preserved, even if the server later rejects it.
Only `None` is inferred: fetch the tournament's `idseason`, fetch each needed
team's season memberships once, and choose `Б` for same-team, same-season
membership active at the tournament's end date, otherwise `Л`. The server uses
an inclusive membership start and an exclusive end; null boundaries are open.
Timestamp offsets are respected. A season row's captain status never makes an
inferred flag `К`. Membership is never inferred from today's/latest season.

Every player ID is checked through the public API before saving: the server
would otherwise silently skip unknown IDs while replacing the roster. A
redirected/merged player ID must be corrected explicitly by the caller.
Missing season metadata, missing end dates needed to evaluate membership,
malformed dates, and failed/paginated lookups fail before any write. With all
flags explicit, season and membership lookups are unnecessary. Duplicate player
IDs and conflicting names for a team are rejected before requests.

Team names are sent directly, preserving quotes. `RosterUploadReport.sanitized`
is retained for compatibility and is always empty. `renamed_on_tournament` lists
submitted names differing from the registered team names. Nonblocking server
warnings are returned as human-readable strings in `warnings`.

## Results

`GET /api/tournament/{id}/representative/results` returns the authenticated
scope's complete question maps, including controversial answer text:

```json
{
  "meta": {"tournamentId": 42, "questionCount": 5},
  "teams": [{"id": 9, "questions": {"1": 1, "2": "спорный", "3": 0, "4": 0, "5": 0}}]
}
```

`upload_results` uses a default public `ApiClient` to get tournament round
sizes. `upload_results_with(&api, tournament_id, rows)` lets callers supply an
existing client or mirror. Both use the authenticated site to read existing
answers before converting round-local marks into absolute question numbers.
Each supplied round must contain exactly its configured question count.
Round numbers are one-based; duplicate team/round pairs are rejected.

`POST /api/tournament/{id}/representative/results` receives:

```json
{
  "teams": [{"id": 9, "questions": {"1": 1, "2": "спорный", "3": 1, "4": 0, "5": "новый ответ"}}]
}
```

Correct answers are `1`; wrong/blank marks are `0`; contested marks carry
answer text, or `?` if none was supplied. An empty, `0`, or `1` contested text
is rejected because the endpoint would interpret it as an ordinary mark.
Omitted rounds retain saved marks and controversial text. New teams get zero
for unsubmitted questions. Only teams present in the input are posted.

If a submitted team is absent from the initial snapshot, the client also reads
the full authorized view with `admin=1` before assuming it is new. The server
restricts that view to the caller's actual permissions. Only missing target
teams are taken from this extra read, and the write retains the original scope
so new teams still attach to the original request.

The server currently concatenates sparse saved round masks without inserting
missing earlier rounds. An incomplete snapshot (missing/null answers) therefore
cannot safely preserve omitted rounds. For an existing team in that situation,
the client requires all its rounds in the input and does not write a partial
update. Controversial-answer text can conceal those trailing nulls; a trailing
run of text as long as the smallest round is therefore treated as ambiguous
and also requires all rounds. This conservative check can reject a dense
snapshot with many trailing controversials. Full-round replacement remains
supported. These checks assume the server's saved masks have valid round widths.

Sending only changed questions is unsafe: the server recomputes the team's
total and removes controversial links absent from the payload. The client
therefore sends each submitted team's full question map. The GET and POST
are separate requests without a revision token, so another edit to the same
team between them can be overwritten. Callers should serialize their own
uploads. Team naming/town changes belong to roster submission, not this endpoint.

## Responses and verification

Success requires a top-level boolean `saved: true`. Roster validation failures
can still return HTTP 200 with `saved: true` and a nonempty `errors` list;
these are failures. Roster warnings do not cancel the save and are returned in
the report. Results warnings can mean teams were skipped after others saved,
so they return `Error::Site` explicitly stating that some results may have
been saved. There is no automatic retry of a write.

Login HTML (at any status) and HTTP 401 become `SessionExpired`. Permission,
deadline, validation and write-lock failures retain their HTTP status. Invalid
JSON, HTML success pages, missing confirmation and malformed response structures
are errors. Offline HTTP contract tests are in `tests/submission_mock.rs`;
they exercise the Rust client against fixtures taken from the server contract.
No authenticated production write is needed to run them.
