//! Plain data types returned by [`crate::api`]. All derive `Serialize` /
//! `Deserialize` so callers can cache them as they see fit.

use serde::{Deserialize, Serialize};

/// A player as listed on rating.chgk.info.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub id: i64,
    pub surname: String,
    pub name: String,
    /// Patronymic; empty when the player has none.
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
pub struct TeamInfo {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub town: String,
    #[serde(default)]
    pub country: String,
}

/// A town from `/towns`. `id` is the site's numeric town id, needed when
/// creating a team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TownInfo {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub country: String,
}

/// A synchronous-tournament venue ("площадка") from `/venues`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueInfo {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub town: Option<VenueTown>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueTown {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
}

impl VenueInfo {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tournament {
    pub id: i64,
    pub name: String,
    /// ISO timestamp (`dateStart`), if the site has one.
    #[serde(default)]
    pub date_start: Option<String>,
    /// Number of questions per round, in round order. Empty when the site
    /// has no `questionQty`.
    #[serde(default)]
    pub questions_per_round: Vec<u32>,
    /// Player ids of the organising committee.
    #[serde(default)]
    pub orgcommittee: Vec<i64>,
}

impl Tournament {
    pub fn total_questions(&self) -> u32 {
        self.questions_per_round.iter().sum()
    }
}

/// A synchronous tournament as played at one venue: the tournament joined
/// with that venue's request for it.
///
/// Field names are stable — callers cache these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynchTournament {
    pub id: i64,
    pub name: String,
    /// ISO timestamp of the synch playing at this venue.
    pub date: String,
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
pub struct TournamentResult {
    pub team_id: i64,
    pub team_name: String,
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
    /// Controversial ("спорный") verdicts recorded for this team.
    #[serde(default)]
    pub controversials: Vec<Controversial>,
}

/// A controversial-answer verdict for one question of one team.
/// `status` is `"A"` (accepted) or `"D"` (declined); anything else is
/// still pending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Controversial {
    pub question_number: u32,
    pub status: String,
}

/// [`Controversial`] flattened with its team id — the shape callers that
/// only care about verdicts want.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControversialVerdict {
    pub team_id: i64,
    pub question_number: u32,
    pub status: String,
}
