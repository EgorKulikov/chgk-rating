//! Plain data types returned by [`crate::api`]. All derive `Serialize` /
//! `Deserialize` so callers can cache them as they see fit.
//!
//! The structs are `#[non_exhaustive]`: the site keeps growing fields, and
//! new ones are added here without a breaking release. That also means
//! they cannot be built with struct literals outside this crate; test
//! fixtures in consumers go through `serde_json::from_value` (every field
//! that can be absent has a serde default).

use serde::{Deserialize, Serialize};

/// A player as listed on rating.chgk.info.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Player {
    /// The site's numeric player id.
    pub id: i64,
    /// Surname (фамилия).
    pub surname: String,
    /// Given name (имя).
    pub name: String,
    /// Patronymic (отчество); empty when the player has none.
    #[serde(default)]
    pub patronymic: String,
}

impl Player {
    /// `"<id> <surname> <name> [<patronymic>]"`.
    pub fn display(&self) -> String {
        if self.patronymic.is_empty() {
            format!("{} {} {}", self.id, self.surname, self.name)
        } else {
            format!(
                "{} {} {} {}",
                self.id, self.surname, self.name, self.patronymic
            )
        }
    }
}

/// A team as listed on rating.chgk.info. `town` / `country` are empty when
/// the site has none on record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Team {
    /// The site's numeric team id.
    pub id: i64,
    /// Registered team name, exactly as the site stores it (it may contain
    /// literal `"` characters).
    pub name: String,
    /// Town name, or empty.
    #[serde(default)]
    pub town: String,
    /// Country name, or empty.
    #[serde(default)]
    pub country: String,
}

/// A town from `/towns`. `id` is the site's numeric town id, needed when
/// creating a team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Town {
    /// The site's numeric town id.
    pub id: i64,
    /// Town name.
    pub name: String,
    /// Country name, or empty.
    #[serde(default)]
    pub country: String,
}

/// A synchronous-tournament venue ("площадка") from `/venues`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Venue {
    /// The site's numeric venue id.
    pub id: i64,
    /// Venue name.
    pub name: String,
    /// The venue's town, when the site has one on record.
    #[serde(default)]
    pub town: Option<VenueTown>,
}

/// The town of a [`Venue`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct VenueTown {
    /// The site's numeric town id, when the site sends one.
    #[serde(default)]
    pub id: Option<i64>,
    /// Town name.#[serde(default)]
    pub name: Option<String>,
    /// Country name.
    #[serde(default)]
    pub country: Option<String>,
}

impl Venue {
    /// `"Name (Town, Country)"` — town/country omitted when absent.
    pub fn display(&self) -> String {
        let mut out = self.name.clone();
        let town = self.town.as_ref().and_then(|t| t.name.clone());
        let country = self.town.as_ref().and_then(|t| t.country.clone());
        match (town, country) {
            (Some(t), Some(c)) => out.push_str(&format!(" ({}, {})", t, c)),
            (Some(t), None) => out.push_str(&format!(" ({})", t)),
            _ => {}
        }
        out
    }

    /// Town name, or empty.
    pub fn town_name(&self) -> String {
        self.town
            .as_ref()
            .and_then(|t| t.name.clone())
            .unwrap_or_default()
    }
}

/// A tournament from `/tournaments/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Tournament {
    /// The site's numeric tournament id.
    pub id: i64,
    /// Tournament name.
    pub name: String,
    /// Full name; often empty.
    #[serde(default)]
    pub long_name: String,
    /// Tournament type id: 2 regular (очник), 3 synchronous, 6 strict
    /// synchronous, 8 asynchronous. See [`Tournament::is_synchronous`].
    #[serde(default)]
    pub type_id: Option<i64>,
    /// Tournament type name as the site shows it (`Синхрон`, ...).
    #[serde(default)]
    pub type_name: Option<String>,
    /// ISO timestamp (`dateStart`), if the site has one.
    #[serde(default)]
    pub date_start: Option<String>,
    /// ISO timestamp (`dateEnd`).
    #[serde(default)]
    pub date_end: Option<String>,
    /// Last change to rating-relevant data (results, rosters, ...).
    #[serde(default)]
    pub last_edit_date: Option<String>,
    /// Season id (`idseason`); see [`Season`].
    #[serde(default)]
    pub season_id: Option<i64>,
    /// Number of questions per round, in round order. Empty when the site
    /// has no `questionQty`.
    #[serde(default)]
    pub questions_per_round: Vec<u32>,
    /// Player ids of the organising committee.
    #[serde(default)]
    pub orgcommittee: Vec<i64>,
    /// Player ids of the question editors.
    #[serde(default)]
    pub editors: Vec<i64>,
    /// Player ids of the game jury.
    #[serde(default)]
    pub game_jury: Vec<i64>,
    /// Player ids of the appeal jury.
    #[serde(default)]
    pub appeal_jury: Vec<i64>,
    /// Language codes (`ru`, `uk`, ...); see [`Language`].
    #[serde(default)]
    pub languages: Vec<String>,
    /// Rating systems the results count for (`mak`, `chgkgg`).
    #[serde(default)]
    pub rating_systems: Vec<String>,
    /// Whether a synchronous tournament is archived.
    #[serde(default)]
    pub archive: Option<bool>,
    /// From when venues may download the questions; result masks become
    /// visible after this moment.
    #[serde(default)]
    pub date_download_questions_from: Option<String>,
    /// Until when venues may request the tournament.
    #[serde(default)]
    pub date_requests_allowed_to: Option<String>,
    /// Until when appeals are accepted.
    #[serde(default)]
    pub date_appeal_allowed_to: Option<String>,
    /// Until when results are hidden.
    #[serde(default)]
    pub hide_results_to: Option<String>,
    /// The organisers' difficulty forecast.
    #[serde(default)]
    pub difficulty_forecast: Option<f64>,
    /// Link to the regulations, when not auto-generated.
    #[serde(default)]
    pub regulations_url: String,
}

impl Tournament {
    /// Sum of [`Tournament::questions_per_round`].
    pub fn total_questions(&self) -> u32 {
        self.questions_per_round.iter().sum()
    }

    /// Synchronous, strict synchronous or asynchronous — the kinds played
    /// at venues through requests.
    pub fn is_synchronous(&self) -> bool {
        matches!(self.type_id, Some(3) | Some(6) | Some(8))
    }
}

/// A synchronous tournament as played at one venue: the tournament joined
/// with that venue's request for it.
///
/// Field names are stable — callers cache these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SynchTournament {
    /// The site's numeric tournament id.
    pub id: i64,
    /// Tournament name.
    pub name: String,
    /// ISO timestamp of the synch playing at this venue; empty when the
    /// site has none.
    pub date: String,
    /// Number of questions per round, in round order.
    pub questions_per_round: Vec<u32>,
    /// Player id of the venue's representative for this tournament. Only
    /// this account can create teams/players or upload rosters/results for
    /// the venue's playing of it.
    #[serde(default)]
    pub representative_id: Option<i64>,
    /// Venue-request status: `"A"` approved, `"N"` new. `None` for
    /// tournaments that did not come from a venue request list.
    #[serde(default)]
    pub status: Option<String>,
    /// Player ids of the organising committee. Any member may upload.
    #[serde(default)]
    pub orgcommittee: Vec<i64>,
}

impl From<Tournament> for SynchTournament {
    fn from(t: Tournament) -> Self {
        SynchTournament {
            id: t.id,
            name: t.name,
            date: t.date_start.unwrap_or_default(),
            questions_per_round: t.questions_per_round,
            representative_id: None,
            status: None,
            orgcommittee: t.orgcommittee,
        }
    }
}

/// One team's row from `/tournaments/{id}/results`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TournamentResult {
    /// The site's numeric team id.
    pub team_id: i64,
    /// Team name as played in this tournament.
    pub team_name: String,
    /// Town name, when the site has one.
    #[serde(default)]
    pub town: Option<String>,
    /// `"110101…"` — one char per question across all rounds, in order.
    /// `Some` once the tournament is past `dateDownloadQuestionsFrom`.
    #[serde(default)]
    pub mask: Option<String>,
    /// Correct answers; `None` until the results are published.
    #[serde(default)]
    pub questions_total: Option<i32>,
    /// Final position (ties give fractional values); `None` until the
    /// results are published.
    #[serde(default)]
    pub position: Option<f64>,
    /// Venue the team played at (synchronous tournaments).
    #[serde(default)]
    pub venue_id: Option<i64>,
    /// The venue request the row belongs to (synchronous tournaments).
    #[serde(default)]
    pub synch_request_id: Option<i64>,
    /// The team's present registered name (the site's `current.name`),
    /// which may equal `team_name`.
    #[serde(default)]
    pub current_name: Option<String>,
    /// Controversial ("спорный") verdicts recorded for this team.
    #[serde(default)]
    pub controversials: Vec<Controversial>,
    /// Flag (зачёт) short names the team competed under; filled only by
    /// [`ResultsQuery::team_flags`](crate::api::ResultsQuery::team_flags).
    #[serde(default)]
    pub flags: Vec<String>,
    /// The roster as played; filled only by
    /// [`ResultsQuery::team_members`](crate::api::ResultsQuery::team_members).
    #[serde(default)]
    pub team_members: Vec<TeamMember>,
}

/// One player of a team's roster in a tournament, from the results
/// endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TeamMember {
    /// The player.
    pub player: Player,
    /// `К` captain, `Б` base squad, `Л` legionnaire, or `None`. See
    /// [`TeamMember::roster_flag`].
    #[serde(default)]
    pub flag: Option<String>,
    /// Individual rating in the release preceding the tournament's end.
    #[serde(default)]
    pub rating: Option<i64>,
}

impl TeamMember {
    /// [`flag`](TeamMember::flag) as the CSV builder's
    /// [`RosterFlag`](crate::csv::RosterFlag); `None` for an empty or
    /// unknown code.
    pub fn roster_flag(&self) -> Option<crate::csv::RosterFlag> {
        let c = self.flag.as_ref()?.trim().chars().next()?;
        crate::csv::RosterFlag::from_char(c)
    }
}

/// A controversial-answer verdict for one question of one team.
/// `status` is `"A"` (accepted) or `"D"` (declined); anything else is
/// still pending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Controversial {
    /// The site's id of the record (`/tournament_synch_controversials/{id}`).
    #[serde(default)]
    pub id: Option<i64>,
    /// 1-based question number across all rounds.
    pub question_number: u32,
    /// `"A"`, `"D"`, or a pending status.
    pub status: String,
    /// The answer the team gave.
    #[serde(default)]
    pub answer: String,
    /// When the verdict was requested.
    #[serde(default)]
    pub issued_at: Option<String>,
    /// The venue's comment.
    #[serde(default)]
    pub comment: Option<String>,
    /// When it was resolved.
    #[serde(default)]
    pub resolved_at: Option<String>,
    /// The jury's comment.
    #[serde(default)]
    pub appeal_jury_comment: Option<String>,
}

/// An appeal on one question of a tournament, from
/// `/tournaments/{id}/appeals`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Appeal {
    /// The site's appeal id.
    pub id: i64,
    /// The tournament.
    pub tournament_id: i64,
    /// Appeal type as the site codes it (`"A"` for a request to accept an
    /// answer; other letters exist).
    pub kind: String,
    /// `"A"` accepted, `"D"` declined, otherwise pending.
    pub status: String,
    /// 1-based question number.
    pub question_number: u32,
    /// The answer under appeal.
    #[serde(default)]
    pub answer: String,
    /// The appellant's text.
    #[serde(default)]
    pub appeal: String,
    /// The jury's comment.
    #[serde(default)]
    pub comment: Option<String>,
    /// When it was filed.
    #[serde(default)]
    pub issued_at: Option<String>,
    /// Id of the appeal that superseded this one, if any.
    #[serde(default)]
    pub overridden_by_id: Option<i64>,
}

/// A venue's request to play a (a)synchronous tournament, from
/// `/venues/{id}/requests`, `/tournaments/{id}/requests` and
/// `/tournament_synch_requests/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TournamentRequest {
    /// The site's request id — what the site's importer calls
    /// `add_with_request_id`.
    pub id: i64,
    /// `"A"` approved, `"N"` new, `"C"` cancelled, `"D"` declined.
    pub status: String,
    /// The tournament requested.
    #[serde(default)]
    pub tournament_id: Option<i64>,
    /// The venue, when the response carries it (all three endpoints do
    /// today; kept optional because the spec allows `null`).
    #[serde(default)]
    pub venue: Option<Venue>,
    /// The venue's representative for this playing.
    #[serde(default)]
    pub representative: Option<Player>,
    /// The narrator, when one is recorded.
    #[serde(default)]
    pub narrator: Option<Player>,
    /// Expected number of teams.
    #[serde(default)]
    pub approximate_teams_count: Option<i64>,
    /// When the request was filed.
    #[serde(default)]
    pub issued_at: Option<String>,
    /// When the venue plays the tournament.
    #[serde(default)]
    pub date_start: Option<String>,
}

/// A tournament a team played, from `/teams/{id}/tournaments`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TeamTournament {
    /// The team.
    pub team_id: i64,
    /// The tournament.
    pub tournament_id: i64,
}

/// A tournament a player played, from `/players/{id}/tournaments`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PlayerTournament {
    /// The player.
    pub player_id: i64,
    /// The team they played for.
    pub team_id: i64,
    /// The tournament.
    pub tournament_id: i64,
}

/// One player's membership of one team's base squad in one season, from
/// `/teams/{id}/seasons` and `/players/{id}/seasons`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SeasonMembership {
    /// The player.
    pub player_id: i64,
    /// The team.
    pub team_id: i64,
    /// The season; see [`Season`].
    pub season_id: i64,
    /// When the player joined.
    #[serde(default)]
    pub date_added: Option<String>,
    /// When the player left, if they did.
    #[serde(default)]
    pub date_removed: Option<String>,
    /// Position on the roster sheet.
    #[serde(default)]
    pub player_number: i64,
}

/// A rating season, from `/seasons`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Season {
    /// The site's season id.
    pub id: i64,
    /// First day, ISO timestamp.
    pub date_start: String,
    /// Last day, ISO timestamp.
    pub date_end: String,
}

/// A rating release, from `/releases`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Release {
    /// The site's release id.
    pub id: i64,
    /// Nominal release date.
    pub date: String,
    /// When it was actually computed.
    #[serde(default)]
    pub real_date: String,
    /// Last recomputation.
    #[serde(default)]
    pub last_run_refresh: String,
}

/// A tournament language, from `/languages`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Language {
    /// Two-letter code (`ru`, `uk`, ...), as used in
    /// [`Tournament::languages`].
    pub id: String,
    /// Display name.
    pub name: String,
}

/// A venue type, from `/venue_types` (1 permanent, 4 one-off, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct VenueType {
    /// The site's type id.
    pub id: i64,
    /// Display name.
    pub name: String,
}

/// A team flag (зачёт), from `/tournament_team_flags`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TeamFlag {
    /// The site's flag id.
    pub id: i64,
    /// Internal name (`COMMON`, `SCHOOL`, ...).
    pub name: String,
    /// Display name.
    pub full_name: String,
    /// Short label shown in result tables.
    pub short_name: String,
    /// Numeric value.
    #[serde(default)]
    pub value: i64,
}

/// A region, from `/regions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Region {
    /// The site's region id.
    pub id: i64,
    /// Region name.
    pub name: String,
    /// Country name, or empty.
    #[serde(default)]
    pub country: String,
}

/// A country, from `/countries/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Country {
    /// The site's country id.
    pub id: i64,
    /// Country name.
    pub name: String,
}

/// [`Controversial`] flattened with its team id — the shape callers that
/// only care about verdicts want.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ControversialVerdict {
    /// The site's numeric team id.
    pub team_id: i64,
    /// 1-based question number across all rounds.
    pub question_number: u32,
    /// `"A"`, `"D"`, or a pending status.
    pub status: String,
}
