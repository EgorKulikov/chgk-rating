# chgk_rating

Rust client for [rating.chgk.info](https://rating.chgk.info), the tournament
site of the "Что? Где? Когда?" community.

- `api` — the public read-only JSON API (`api.rating.chgk.info`): players,
  teams, base rosters, towns, countries, venues, tournaments, results with
  answer masks and controversial verdicts.
- `site` — write flows that mimic the admin website's own forms, since there
  is no public write API: log in to a persistent `Session` (cookies only,
  never the password), create players and teams, upload rosters and results.
- `csv` — the row types and CSV builders those uploads use.

```rust
use chgk_rating::{api, site, csv::RosterRow, Error};

let http = reqwest::Client::new();
let team = api::get_team(&http, 107740).await?;

let session = site::login("me@example.com", "…").await?;   // persist `session`
let client = session.client()?;
match site::upload_rosters(&client, 14015, &rows).await {
    Ok(report) => { /* report.sanitized, report.renamed_on_tournament */ }
    Err(Error::SessionExpired) => { /* ask the user to log in again, then retry */ }
    Err(e) => { /* show e */ }
}
```

`docs/api_notes.md` and `docs/har_notes.md` record what the endpoints and
the site's importer actually do, as learned from HAR captures and live
probing — read them before changing a request.

Used by [chgk-local](https://github.com/EgorKulikov/chgk-local) and the
festival organiser app.

## License

MIT
