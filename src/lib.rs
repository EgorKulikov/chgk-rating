//! Client for [rating.chgk.info](https://rating.chgk.info), the tournament
//! site of the Russian-speaking "Что? Где? Когда?" community.
//!
//! Two halves:
//!
//! * [`api`] — the public, read-only JSON API at `api.rating.chgk.info`
//!   (players, teams, towns, venues, tournaments, results). Plain
//!   `reqwest::Client`, no authentication.
//! * [`site`] — write-side flows that mimic the admin website's own forms
//!   and AJAX calls, because there is no public write API: login into a
//!   persistent [`site::Session`], create players and teams, upload rosters
//!   and results for a tournament. [`csv`] holds the row types and CSV
//!   builders those uploads use.
//!
//! Sessions store only the cookies the site issued (`PHPSESSID`,
//! `REMEMBERME`) — never the password. When the site stops accepting them,
//! write calls fail with [`Error::SessionExpired`] so the caller can ask the
//! user to log in again and retry.
//!
//! Behavioural notes learned from live HAR captures and probing are in
//! `docs/har_notes.md` and `docs/api_notes.md`.

pub mod api;
pub mod csv;
mod error;
pub mod models;
pub mod site;

pub use error::{Error, Result};

/// Default `User-Agent` sent by clients built by this crate.
pub const USER_AGENT: &str = concat!(
    "chgk_rating/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/EgorKulikov/chgk-rating)"
);
