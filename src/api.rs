//! Read-only client for `https://api.rating.chgk.info`.
//!
//! Every function takes a plain `reqwest::Client`; no authentication is
//! involved. Write actions (login, creating players/teams, uploads) live in
//! [`crate::site`] because they have to mimic the website's own forms.
//!
//! See `docs/api_notes.md` for field-level notes on the endpoints.

use reqwest::Client;
use serde::Deserialize;

use crate::models::{
    Controversial, ControversialVerdict, Player, SynchTournament, TeamInfo, TournamentResult,
    Tournament, TownInfo, VenueInfo,
};
use crate::{Error, Result};

pub const API_BASE: &str = "https://api.rating.chgk.info";

// ---- wire types ------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ApiPlayer {
    id: i64,
    name: String,
    surname: String,
    #[serde(default)]
    patronymic: Option<String>,
}

impl From<ApiPlayer> for Player {
    fn from(p: ApiPlayer) -> Self {
        Player {
            id: p.id,
            surname: p.surname,
            name: p.name,
            patronymic: p.patronymic.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiCountry {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiRegion {
    #[serde(default)]
    country: Option<ApiCountry>,
}

#[derive(Debug, Deserialize)]
struct ApiTown {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    country: Option<ApiCountry>,
    #[serde(default)]
    region: Option<ApiRegion>,
}

impl ApiTown {
    /// Country name: directly on the town, or via its region.
    fn country_name(&self) -> Option<String> {
        self.country
            .as_ref()
            .and_then(|c| c.name.clone())
            .or_else(|| {
                self.region
                    .as_ref()
                    .and_then(|r| r.country.as_ref())
                    .and_then(|c| c.name.clone())
            })
    }
}

#[derive(Debug, Deserialize)]
struct ApiTeam {
    #[serde(default)]
    id: i64,
    name: String,
    #[serde(default)]
    town: Option<ApiTown>,
}

impl ApiTeam {
    fn into_info(self, fallback_id: i64) -> TeamInfo {
        let country = self
            .town
            .as_ref()
            .and_then(|t| t.country_name())
            .unwrap_or_default();
        let town = self.town.and_then(|t| t.name).unwrap_or_default();
        TeamInfo {
            // Some responses omit `id`; keep the id we asked for.
            id: if self.id != 0 { self.id } else { fallback_id },
            name: self.name,
            town,
            country,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiVenue {
    id: i64,
    name: String,
    #[serde(default)]
    town: Option<ApiTown>,
}

impl From<ApiVenue> for VenueInfo {
    fn from(v: ApiVenue) -> Self {
        VenueInfo {
            id: v.id,
            name: v.name,
            town: v.town.map(|t| crate::models::VenueTown {
                country: t.country_name(),
                name: t.name,
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiTournament {
    id: i64,
    name: String,
    #[serde(default, rename = "dateStart")]
    date_start: Option<String>,
    #[serde(default, rename = "questionQty")]
    question_qty: Option<serde_json::Value>,
    #[serde(default)]
    orgcommittee: Vec<ApiIdOnly>,
}

impl From<ApiTournament> for Tournament {
    fn from(t: ApiTournament) -> Self {
        Tournament {
            id: t.id,
            name: t.name,
            date_start: t.date_start,
            questions_per_round: parse_question_qty(t.question_qty),
            orgcommittee: t.orgcommittee.into_iter().map(|m| m.id).collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiIdOnly {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct ApiVenueRequest {
    #[serde(default)]
    status: Option<String>,
    #[serde(rename = "tournamentId")]
    tournament_id: i64,
    #[serde(default, rename = "dateStart")]
    date_start: Option<String>,
    #[serde(default)]
    representative: Option<ApiIdOnly>,
}

#[derive(Debug, Deserialize)]
struct ApiResultRow {
    team: ApiResultTeam,
    #[serde(default)]
    mask: Option<String>,
    // Both are null until the tournament's results are published.
    #[serde(default, rename = "questionsTotal")]
    questions_total: Option<f64>,
    #[serde(default)]
    position: Option<f64>,
    #[serde(default, rename = "synchRequest")]
    synch_request: Option<ApiSynchRequest>,
    #[serde(default)]
    controversials: Vec<ApiControversial>,
}

#[derive(Debug, Deserialize)]
struct ApiResultTeam {
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    town: Option<ApiTown>,
}

#[derive(Debug, Deserialize)]
struct ApiSynchRequest {
    #[serde(default)]
    venue: Option<ApiIdOnly>,
}

#[derive(Debug, Deserialize)]
struct ApiControversial {
    #[serde(rename = "questionNumber")]
    question_number: u32,
    #[serde(default)]
    status: String,
}

// ---- players ---------------------------------------------------------------

/// `GET /players/{id}`.
pub async fn get_player(client: &Client, player_id: i64) -> Result<Player> {
    let url = format!("{}/players/{}", API_BASE, player_id);
    let p: ApiPlayer = client.get(&url).send().await?.json().await?;
    Ok(p.into())
}

/// `GET /players?surname=…[&name=…]`. The query is split on the first
/// space into surname and (optional) name.
pub async fn search_players(client: &Client, query: &str, limit: usize) -> Result<Vec<Player>> {
    let url = format!("{}/players", API_BASE);
    let mut parts = query.trim().splitn(2, ' ');
    let surname = parts.next().unwrap_or_default();
    let mut req = client
        .get(&url)
        .query(&[("surname", surname), ("itemsPerPage", &limit.to_string())]);
    if let Some(name) = parts.next().map(str::trim).filter(|n| !n.is_empty()) {
        req = req.query(&[("name", name)]);
    }
    let resp: Vec<ApiPlayer> = req.send().await?.json().await.unwrap_or_default();
    Ok(resp.into_iter().map(Into::into).collect())
}

/// [`search_players`], but a query that is a bare number is first tried
/// as a player id.
pub async fn find_players(client: &Client, query: &str, limit: usize) -> Result<Vec<Player>> {
    if let Ok(id) = query.trim().parse::<i64>() {
        if let Ok(p) = get_player(client, id).await {
            return Ok(vec![p]);
        }
    }
    search_players(client, query, limit).await
}

// ---- teams -----------------------------------------------------------------

/// `GET /teams/{id}`.
pub async fn get_team(client: &Client, team_id: i64) -> Result<TeamInfo> {
    let url = format!("{}/teams/{}", API_BASE, team_id);
    let t: ApiTeam = client.get(&url).send().await?.json().await?;
    Ok(t.into_info(team_id))
}

/// `GET /teams?name=…`.
pub async fn search_teams(client: &Client, query: &str, limit: usize) -> Result<Vec<TeamInfo>> {
    let url = format!("{}/teams", API_BASE);
    let resp: Vec<ApiTeam> = client
        .get(&url)
        .query(&[("name", query), ("itemsPerPage", &limit.to_string())])
        .send()
        .await?
        .json()
        .await
        .unwrap_or_default();
    Ok(resp.into_iter().map(|t| t.into_info(0)).collect())
}

/// [`search_teams`], but a query that is a bare number is first tried as
/// a team id.
pub async fn find_teams(client: &Client, query: &str, limit: usize) -> Result<Vec<TeamInfo>> {
    if let Ok(id) = query.trim().parse::<i64>() {
        if let Ok(t) = get_team(client, id).await {
            return Ok(vec![t]);
        }
    }
    search_teams(client, query, limit).await
}

/// A team's current base roster: the players listed for its latest season
/// in `/teams/{id}/seasons`. Players whose own lookup fails are skipped
/// with a warning; a failed page fetch is an error (treating it as
/// end-of-list used to silently truncate rosters).
pub async fn get_base_roster(client: &Client, team_id: i64) -> Result<Vec<Player>> {
    #[derive(Deserialize)]
    struct Entry {
        idplayer: i64,
        idseason: i64,
    }
    // Guards against an endpoint that never returns an empty page.
    const MAX_PAGES: u32 = 100;

    let mut all: Vec<Entry> = Vec::new();
    let mut page = 1u32;
    loop {
        let url = format!(
            "{}/teams/{}/seasons?page={}&itemsPerPage=100",
            API_BASE, team_id, page
        );
        let entries: Vec<Entry> = client.get(&url).send().await?.json().await?;
        if entries.is_empty() {
            break;
        }
        all.extend(entries);
        page += 1;
        if page > MAX_PAGES {
            break;
        }
    }

    let latest = all.iter().map(|e| e.idseason).max().unwrap_or(0);
    let mut ids: Vec<i64> = all
        .iter()
        .filter(|e| e.idseason == latest)
        .map(|e| e.idplayer)
        .collect();
    ids.sort_unstable();
    ids.dedup();

    let mut players = Vec::with_capacity(ids.len());
    for pid in ids {
        match get_player(client, pid).await {
            Ok(p) => players.push(p),
            Err(e) => tracing::warn!("base roster of team {}: player {} lookup failed: {}", team_id, pid, e),
        }
    }
    Ok(players)
}

// ---- towns / countries -----------------------------------------------------

/// `GET /towns?name=…`.
pub async fn search_towns(client: &Client, query: &str, limit: usize) -> Result<Vec<TownInfo>> {
    let url = format!("{}/towns", API_BASE);
    let resp: Vec<ApiTown> = client
        .get(&url)
        .query(&[("name", query), ("itemsPerPage", &limit.to_string())])
        .send()
        .await?
        .json()
        .await
        .unwrap_or_default();
    Ok(resp
        .into_iter()
        .filter_map(|t| {
            let country = t.country_name().unwrap_or_default();
            t.name.map(|name| TownInfo { id: t.id, name, country })
        })
        .collect())
}

/// Numeric id of the town whose name equals `name` (case-insensitive).
/// Needed by [`crate::site::create_team`].
pub async fn lookup_town_id(client: &Client, name: &str) -> Result<Option<i64>> {
    let needle = name.trim().to_lowercase();
    Ok(search_towns(client, name.trim(), 10)
        .await?
        .into_iter()
        .find(|t| t.name.to_lowercase() == needle)
        .map(|t| t.id))
}

/// `GET /countries?name=…` → country names.
pub async fn search_countries(client: &Client, query: &str, limit: usize) -> Result<Vec<String>> {
    let url = format!("{}/countries", API_BASE);
    let resp: Vec<ApiCountry> = client
        .get(&url)
        .query(&[("name", query), ("itemsPerPage", &limit.to_string())])
        .send()
        .await?
        .json()
        .await
        .unwrap_or_default();
    Ok(resp.into_iter().filter_map(|c| c.name).collect())
}

/// The country whose name equals `name` (ASCII-case-insensitive), if any.
pub async fn get_country_by_name(client: &Client, name: &str) -> Result<Option<String>> {
    let results = search_countries(client, name, 1).await?;
    Ok(results
        .into_iter()
        .find(|n| n.eq_ignore_ascii_case(name.trim())))
}

// ---- venues ----------------------------------------------------------------

/// `GET /venues/{id}`.
pub async fn get_venue(client: &Client, venue_id: i64) -> Result<VenueInfo> {
    let url = format!("{}/venues/{}", API_BASE, venue_id);
    let v: ApiVenue = client.get(&url).send().await?.json().await?;
    Ok(v.into())
}

/// Town name of a venue, or empty.
pub async fn get_venue_town(client: &Client, venue_id: i64) -> Result<String> {
    Ok(get_venue(client, venue_id).await?.town_name())
}

/// `GET /venues?name=…`.
pub async fn search_venues(client: &Client, query: &str, limit: usize) -> Result<Vec<VenueInfo>> {
    let url = format!("{}/venues", API_BASE);
    let resp: Vec<ApiVenue> = client
        .get(&url)
        .query(&[("name", query), ("itemsPerPage", &limit.to_string())])
        .send()
        .await?
        .json()
        .await
        .unwrap_or_default();
    Ok(resp.into_iter().map(Into::into).collect())
}

// ---- tournaments -----------------------------------------------------------

/// `GET /tournaments/{id}`.
pub async fn get_tournament(client: &Client, tournament_id: i64) -> Result<Tournament> {
    let url = format!("{}/tournaments/{}", API_BASE, tournament_id);
    let t: ApiTournament = client.get(&url).send().await?.json().await?;
    Ok(t.into())
}

/// `questionQty` comes back either as an array (`[12, 12, 12]`) or as an
/// object keyed by round number (`{"1": 12, "2": 12}`), in which case the
/// keys are sorted numerically.
fn parse_question_qty(v: Option<serde_json::Value>) -> Vec<u32> {
    let Some(v) = v else { return vec![] };
    if let Some(arr) = v.as_array() {
        return arr
            .iter()
            .filter_map(|x| x.as_u64().map(|n| n as u32))
            .collect();
    }
    if let Some(obj) = v.as_object() {
        let mut entries: Vec<(u32, u32)> = obj
            .iter()
            .filter_map(|(k, val)| Some((k.parse().ok()?, val.as_u64()? as u32)))
            .collect();
        entries.sort_by_key(|(i, _)| *i);
        return entries.into_iter().map(|(_, n)| n).collect();
    }
    vec![]
}

/// Synchronous tournaments played at a venue, most recent first.
///
/// Filtering `/synch_tournaments` is silently ignored by the site (see
/// `docs/api_notes.md` §1), so this walks `/venues/{id}/requests`
/// (paginated, at most 50 pages), keeps approved (`A`) and new (`N`)
/// requests, sorts by `dateStart` descending, and resolves the 100 most
/// recent through `/tournaments/{id}`. Tournaments that fail to resolve
/// are skipped with a warning.
pub async fn list_synch_tournaments_for_venue(
    client: &Client,
    venue_id: i64,
) -> Result<Vec<SynchTournament>> {
    let mut all: Vec<ApiVenueRequest> = Vec::new();
    let mut page = 1u32;
    loop {
        let url = format!("{}/venues/{}/requests", API_BASE, venue_id);
        let batch: Vec<ApiVenueRequest> = client
            .get(&url)
            .query(&[("page", page.to_string()), ("itemsPerPage", "100".to_string())])
            .send()
            .await?
            .json()
            .await
            .unwrap_or_default();
        if batch.is_empty() {
            break;
        }
        let len = batch.len();
        all.extend(batch);
        if len < 100 || page >= 50 {
            break;
        }
        page += 1;
    }

    all.retain(|r| matches!(r.status.as_deref(), Some("A") | Some("N")));
    all.sort_by(|a, b| b.date_start.cmp(&a.date_start));

    let mut out: Vec<SynchTournament> = Vec::new();
    for r in all.into_iter().take(100) {
        let t = match get_tournament(client, r.tournament_id).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("venue {}: tournament {} lookup failed: {}", venue_id, r.tournament_id, e);
                continue;
            }
        };
        out.push(SynchTournament {
            id: t.id,
            name: t.name,
            date: r.date_start.or(t.date_start).unwrap_or_default(),
            questions_per_round: t.questions_per_round,
            representative_id: r.representative.map(|p| p.id),
            status: r.status,
            orgcommittee: Vec::new(),
        });
    }
    Ok(out)
}

/// `GET /tournaments/{id}/results?includeMasksAndControversials=1` — one
/// row per team, with answer masks and controversial verdicts.
pub async fn get_tournament_results(
    client: &Client,
    tournament_id: i64,
) -> Result<Vec<TournamentResult>> {
    let url = format!("{}/tournaments/{}/results", API_BASE, tournament_id);
    let resp: Vec<ApiResultRow> = client
        .get(&url)
        .query(&[("includeMasksAndControversials", "1")])
        .send()
        .await?
        .json()
        .await?;
    Ok(resp
        .into_iter()
        .map(|r| TournamentResult {
            team_id: r.team.id,
            team_name: r.team.name,
            town: r.team.town.and_then(|t| t.name),
            mask: r.mask,
            questions_total: r.questions_total.map(|q| q as i32),
            position: r.position,
            venue_id: r.synch_request.and_then(|s| s.venue.map(|v| v.id)),
            controversials: r
                .controversials
                .into_iter()
                .map(|c| Controversial {
                    question_number: c.question_number,
                    status: c.status,
                })
                .collect(),
        })
        .collect())
}

/// All controversial verdicts of a tournament, flattened. Empty when the
/// tournament has none or is not reachable.
pub async fn get_tournament_controversials(
    client: &Client,
    tournament_id: i64,
) -> Result<Vec<ControversialVerdict>> {
    let rows = match get_tournament_results(client, tournament_id).await {
        Ok(rows) => rows,
        Err(Error::Http(e)) if e.is_decode() => Vec::new(),
        Err(e) => return Err(e),
    };
    Ok(rows
        .into_iter()
        .flat_map(|r| {
            r.controversials.into_iter().map(move |c| ControversialVerdict {
                team_id: r.team_id,
                question_number: c.question_number,
                status: c.status,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn question_qty_array_and_object() {
        assert_eq!(parse_question_qty(Some(json!([12, 12, 12]))), vec![12, 12, 12]);
        assert_eq!(
            parse_question_qty(Some(json!({"2": 15, "1": 12, "3": 9}))),
            vec![12, 15, 9]
        );
        assert_eq!(parse_question_qty(Some(json!("nope"))), Vec::<u32>::new());
        assert_eq!(parse_question_qty(None), Vec::<u32>::new());
    }

    #[test]
    fn town_country_falls_back_to_region() {
        let t: ApiTown = serde_json::from_value(json!({
            "id": 2088, "name": "Краков",
            "region": {"country": {"name": "Польша"}}
        }))
        .unwrap();
        assert_eq!(t.country_name().as_deref(), Some("Польша"));
        let t: ApiTown = serde_json::from_value(json!({
            "id": 31, "name": "Берлин", "country": {"name": "Германия"},
            "region": {"country": {"name": "ignored"}}
        }))
        .unwrap();
        assert_eq!(t.country_name().as_deref(), Some("Германия"));
    }

    #[test]
    fn team_keeps_requested_id_when_missing() {
        let t: ApiTeam = serde_json::from_value(json!({"name": "X"})).unwrap();
        assert_eq!(t.into_info(77174).id, 77174);
    }

    #[test]
    fn result_row_parses_live_shape() {
        let row: ApiResultRow = serde_json::from_value(json!({
            "team": {"id": 107740, "name": "\"Ладно, погнали втроём сегодня\"", "town": {"name": "Краков"}},
            "mask": "1101",
            "questionsTotal": 3,
            "position": 2.5,
            "synchRequest": {"venue": {"id": 3360}},
            "controversials": [{"questionNumber": 4, "status": "A"}]
        }))
        .unwrap();
        assert_eq!(row.team.id, 107740);
        assert_eq!(row.synch_request.unwrap().venue.unwrap().id, 3360);
        assert_eq!(row.controversials[0].question_number, 4);
    }
}
