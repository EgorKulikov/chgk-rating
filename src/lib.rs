//! Client for [rating.chgk.info](https://rating.chgk.info), the tournament
//! site of the Russian-speaking "Что? Где? Когда?" community.
//!
//! Two halves:
//!
//! * [`api`] — the public, read-only JSON API at `api.rating.chgk.info`
//!   (players, teams, towns, venues, tournaments, results), through an
//!   [`ApiClient`]. No authentication.
//! * [`site`] — the website's cookie-authenticated JSON submission endpoints
//!   and login/creation forms: log in to obtain
//!   a persistent [`Session`], build a [`SiteClient`] from it, then create
//!   players and teams and upload rosters and results for a tournament.
//!   [`csv`] holds the input row types and optional CSV export builders.
//!
//! Sessions store the cookies the site issued (`PHPSESSID`, `REMEMBERME`)
//! plus the login name and player id — never the password. When the site
//! stops accepting the cookies, write calls fail with
//! [`Error::SessionExpired`] so the caller can ask the user to log in again
//! and retry.
//!
//! ```no_run
//! use chgk_rating::{csv::RosterRow, site, ApiClient, Error};
//!
//! # async fn example(rows: Vec<RosterRow>) -> chgk_rating::Result<()> {
//! let api = ApiClient::new()?;
//! let team = api.get_team(107740).await?;
//! println!("{} from {}", team.name, team.town);
//!
//! let session = site::login("me@example.com", "…").await?; // persist `session`
//! let client = session.client()?;
//! match client.upload_rosters(&api, 14015, &rows).await {
//!     Ok(report) => println!("{} renamed", report.renamed_on_tournament.len()),
//!     Err(Error::SessionExpired) => { /* ask the user to log in again, then retry */ }
//!     Err(e) => eprintln!("{}", e),
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Submission contracts from the server source are in `docs/submission_api.md`;
//! public read and legacy form notes are in `docs/api_notes.md` and `docs/har_notes.md`.

#![warn(missing_docs)]

pub mod api;
pub mod csv;
mod error;
mod http;
pub mod models;
pub mod site;
mod text;
pub use api::ApiClient;
pub use error::{Error, Result};
pub use http::MAX_RESPONSE_BYTES;
pub use models::*;
pub use site::{Session, SiteClient};

/// Default `User-Agent` sent by clients built by this crate.
pub const USER_AGENT: &str = concat!(
    "chgk_rating/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/EgorKulikov/chgk-rating)"
);

/// Overall request timeout applied by [`ApiClient::new`] and
/// [`Session::client`].
pub const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Connect timeout applied by [`ApiClient::new`] and [`Session::client`].
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
