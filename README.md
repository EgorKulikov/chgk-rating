# chgk_rating

Rust client for [rating.chgk.info](https://rating.chgk.info), the tournament
site of the "Что? Где? Когда?" community.

- `api` — `ApiClient` for the public read-only JSON API (`api.rating.chgk.info`):
  players, teams, base rosters, towns, countries, regions, venues,
  tournaments (by id, searched by name/type/date, or listed for a venue),
  venue requests, results with answer masks, controversials and appeals,
  team and player histories, and the reference lists (seasons, releases,
  languages, venue types, team flags). Every anonymous read endpoint of the
  official spec is wrapped except the user table and a few by-id variants
  of reference lists; see the coverage table in `docs/api_notes.md`.
- `site` — write flows using the website's cookie-authenticated JSON API
  for roster/results submission and forms for login and player/team creation.
  Log in to a persistent `Session` (the site's
  cookies plus login name and player id — never the password), build a
  `SiteClient` from it, create players and teams, upload rosters and results.
  A serialised `Session` is a bearer credential, valid for about a year
  when it holds the `REMEMBERME` cookie (the default) and until the site's
  session expires otherwise; store it with owner-only permissions.
- `csv` — the row types accepted by submissions and optional CSV export builders.

```rust
use chgk_rating::{csv::RosterRow, site, ApiClient, Error};

async fn example(rows: Vec<RosterRow>) -> chgk_rating::Result<()> {
    let api = ApiClient::new()?;
    let team = api.get_team(107740).await?;
    println!("{} from {}", team.name, team.town);

    let session = site::login("me@example.com", "…").await?; // persist `session`
    let client = session.client()?;
    match client.upload_rosters(&api, 14015, &rows).await {
        Ok(report) => println!("{} renamed", report.renamed_on_tournament.len()),
        Err(Error::SessionExpired) => { /* ask the user to log in again, then retry */ }
        Err(e) => eprintln!("{}", e),
    }
    Ok(())
}
```

Roster submission preserves explicit `RosterFlag` values. When a row's flag
is `None`, it uses base (`Б`) if the player belongs to that team's base roster
in the tournament's season at its end date, and legionnaire (`Л`) otherwise.
Team names, including quotes, are sent unchanged. A submission replaces the
complete roster of each included team.

Results submission replaces each supplied round and retains omitted rounds,
including controversial answer text. Incomplete or ambiguous legacy snapshots
require all rounds for the affected team. `upload_results_with(&api, id, &rows)`
accepts a custom public API client for round metadata; `upload_results(id,
&rows)` uses the default client. Server warnings about skipped results return
an error, since other teams may already have been saved.

`docs/submission_api.md` documents the JSON contract from the server source.
`docs/api_notes.md` covers public reads; `docs/har_notes.md` records the login,
creation forms, and historical CSV import behavior.

Used by [chgk-local](https://github.com/EgorKulikov/chgk-local) and the
festival organiser app.

## License

MIT
