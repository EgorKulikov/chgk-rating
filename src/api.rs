//! Read-only client for `https://api.rating.chgk.info`.
//!
//! Everything goes through an [`ApiClient`]; no authentication is
//! involved. Write actions (login, creating players/teams, uploads) live in
//! [`crate::site`] because they have to mimic the website's own forms.
//!
//! Method naming:
//!
//! * `get_*` — one record by numeric id; a missing id is
//!   [`Error::Status`] with status 404.
//! * `get_*` on a sub-collection (`get_team_seasons`,
//!   `get_tournament_requests`, ...) — everything the site has for that
//!   id, every page.
//! * `search_*` — the site's own name filter (a case-insensitive substring
//!   match on every collection — see `docs/api_notes.md`), at most
//!   `limit` rows, in ascending id order. An empty query returns the
//!   first `limit` rows of the collection. `search_tournaments` takes a
//!   [`TournamentQuery`] instead of a name and limit.
//! * `find_*` — `search_*`, but a query that is a bare number is first
//!   tried as an id. Only a 404 on the id falls through to the search;
//!   any other failure is returned.
//! * `lookup_*` — exact-name resolution (case-insensitive on our side,
//!   using Unicode lowercasing) over the first 100 substring matches.
//! * `list_*` — a reference collection in full.
//!
//! See `docs/api_notes.md` for field-level notes on the endpoints.

use reqwest::{Client, Url};
use serde::Deserialize;

use crate::http::read_body;
use crate::models::{
    Appeal, Controversial, ControversialVerdict, Country, Language, Player, PlayerTournament,
    Region, Release, Season, SeasonMembership, SynchTournament, Team, TeamFlag, TeamMember,
    TeamTournament, Tournament, TournamentRequest, TournamentResult, Town, Venue, VenueType,
};
use crate::text::summarize_response;
use crate::{Error, Result};

/// Base URL of the public API.
pub const API_BASE: &str = "https://api.rating.chgk.info";

/// Client for the public read-only API.
#[derive(Debug, Clone)]
pub struct ApiClient {
    http: Client,
    base: Url,
}

impl ApiClient {
    /// A client for [`API_BASE`] with the crate's [`USER_AGENT`](crate::USER_AGENT)
    /// and default timeouts.
    ///
    /// # Errors
    /// [`Error::Http`] if the TLS backend cannot be initialised.
    pub fn new() -> Result<ApiClient> {
        let http = Client::builder()
            .user_agent(crate::USER_AGENT)
            .timeout(crate::REQUEST_TIMEOUT)
            .connect_timeout(crate::CONNECT_TIMEOUT)
            .https_only(true)
            .build()?;
        Ok(ApiClient::from_client(http))
    }

    /// Wrap a caller-configured `reqwest::Client` (proxy, extra headers,
    /// different timeouts). The caller is responsible for its `User-Agent`.
    pub fn from_client(http: Client) -> ApiClient {
        ApiClient {
            http,
            base: Url::parse(API_BASE).expect("API_BASE is a valid URL"),
        }
    }

    /// Like [`ApiClient::from_client`], but against another base URL (a
    /// mirror, or a local test server).
    ///
    /// # Errors
    /// [`Error::Config`] if `base` is not an absolute URL.
    pub fn with_base_url(http: Client, base: &str) -> Result<ApiClient> {
        let base =
            Url::parse(base).map_err(|e| Error::Config(format!("base url {:?}: {}", base, e)))?;
        Ok(ApiClient { http, base })
    }

    /// The underlying `reqwest::Client`.
    pub fn http(&self) -> &Client {
        &self.http
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base.as_str().trim_end_matches('/'), path)
    }
    /// `GET path?query`, decoded as JSON. A non-2xx status is
    /// [`Error::Status`] (with the path and query as `url`); a body that
    /// does not decode, or is too large, is [`Error::Parse`] naming the
    /// endpoint.
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        let resp = self.http.get(self.url(path)).query(query).send().await?;
        let url = resp.url().path_and_query_string();
        let (status, body) = read_body(resp).await.map_err(|e| match e {
            Error::Parse(m) => Error::Parse(format!("{}: {}", url, m)),
            other => other,
        })?;
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                url,
                summary: summarize_response(&body),
            });
        }
        serde_json::from_str(&body).map_err(|e| Error::Parse(format!("{}: {}", url, e)))
    }

    /// Every row of a paginated collection, 100 per page. A failed page is
    /// an error, and so is a collection that is still going after
    /// `max_pages` pages — never a short list.
    async fn get_all_pages<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        max_pages: u32,
    ) -> Result<Vec<T>> {
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let page_s = page.to_string();
            let mut q: Vec<(&str, &str)> = query.to_vec();
            q.push(("page", &page_s));
            q.push(("itemsPerPage", "100"));
            let batch: Vec<T> = self.get_json(path, &q).await?;
            let len = batch.len();
            all.extend(batch);
            if len < 100 {
                break;
            }
            if page >= max_pages {
                return Err(Error::Parse(format!(
                    "{}: more than {} pages of 100 rows; refusing to return a truncated list",
                    path, max_pages
                )));
            }
            page += 1;
        }
        Ok(all)
    }
}

trait PathAndQuery {
    fn path_and_query_string(&self) -> String;
}

impl PathAndQuery for Url {
    fn path_and_query_string(&self) -> String {
        match self.query() {
            Some(q) => format!("{}?{}", self.path(), q),
            None => self.path().to_string(),
        }
    }
}

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
    id: Option<i64>,
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
    id: Option<i64>,
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
    fn into_team(self, fallback_id: i64) -> Team {
        let country = self
            .town
            .as_ref()
            .and_then(|t| t.country_name())
            .unwrap_or_default();
        let town = self.town.and_then(|t| t.name).unwrap_or_default();
        Team {
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

impl From<ApiVenue> for Venue {
    fn from(v: ApiVenue) -> Self {
        Venue {
            id: v.id,
            name: v.name,
            town: v.town.map(|t| crate::models::VenueTown {
                id: t.id,
                country: t.country_name(),
                name: t.name,
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiTournament {
    id: i64,
    name: String,
    #[serde(default)]
    long_name: String,
    /// Live: `{"id": 3, "name": "Синхрон", "shortName": "С"}`; the spec
    /// says integer. Accept both.
    #[serde(default, rename = "type")]
    kind: Option<serde_json::Value>,
    #[serde(default)]
    date_start: Option<String>,
    #[serde(default)]
    date_end: Option<String>,
    #[serde(default)]
    last_edit_date: Option<String>,
    #[serde(default)]
    idseason: Option<i64>,
    #[serde(default)]
    question_qty: Option<serde_json::Value>,
    #[serde(default)]
    orgcommittee: Vec<ApiIdOnly>,
    #[serde(default)]
    editors: Vec<ApiIdOnly>,
    #[serde(default)]
    game_jury: Vec<ApiIdOnly>,
    #[serde(default)]
    appeal_jury: Vec<ApiIdOnly>,
    #[serde(default)]
    languages: Option<ApiLanguages>,
    #[serde(default)]
    rating_systems: Option<Vec<String>>,
    #[serde(default)]
    archive: Option<bool>,
    #[serde(default)]
    date_download_questions_from: Option<String>,
    #[serde(default)]
    date_requests_allowed_to: Option<String>,
    #[serde(default)]
    date_appeal_allowed_to: Option<String>,
    #[serde(default)]
    hide_results_to: Option<String>,
    #[serde(default)]
    difficulty_forecast: Option<f64>,
    #[serde(default)]
    regulations_url: Option<String>,
}

fn ids(v: Vec<ApiIdOnly>) -> Vec<i64> {
    v.into_iter().map(|m| m.id).collect()
}

impl From<ApiTournament> for Tournament {
    fn from(t: ApiTournament) -> Self {
        let (type_id, type_name) = match &t.kind {
            Some(serde_json::Value::Number(n)) => (n.as_i64(), None),
            Some(serde_json::Value::Object(o)) => (
                o.get("id").and_then(|v| v.as_i64()),
                o.get("name").and_then(|v| v.as_str()).map(str::to_string),
            ),
            _ => (None, None),
        };
        Tournament {
            id: t.id,
            name: t.name,
            long_name: t.long_name,
            type_id,
            type_name,
            date_start: t.date_start,
            date_end: t.date_end,
            last_edit_date: t.last_edit_date,
            season_id: t.idseason,
            questions_per_round: parse_question_qty(t.question_qty),
            orgcommittee: ids(t.orgcommittee),
            editors: ids(t.editors),
            game_jury: ids(t.game_jury),
            appeal_jury: ids(t.appeal_jury),
            languages: t
                .languages
                .map(ApiLanguages::into_vec)
                .unwrap_or_default()
                .into_iter()
                .map(|l| l.id)
                .collect(),
            rating_systems: t.rating_systems.unwrap_or_default(),
            archive: t.archive,
            date_download_questions_from: t.date_download_questions_from,
            date_requests_allowed_to: t.date_requests_allowed_to,
            date_appeal_allowed_to: t.date_appeal_allowed_to,
            hide_results_to: t.hide_results_to,
            difficulty_forecast: t.difficulty_forecast,
            regulations_url: t.regulations_url.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ApiLanguages {
    List(Vec<ApiLanguage>),
    // PHP serializes arrays with gaps in their indices as JSON objects.
    Indexed(std::collections::BTreeMap<String, ApiLanguage>),
}

impl ApiLanguages {
    fn into_vec(self) -> Vec<ApiLanguage> {
        match self {
            Self::List(languages) => languages,
            Self::Indexed(languages) => languages.into_values().collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiLanguage {
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct ApiIdOnly {
    id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRequest {
    id: i64,
    #[serde(default)]
    status: String,
    #[serde(default)]
    tournament_id: Option<i64>,
    #[serde(default)]
    venue: Option<ApiVenue>,
    #[serde(default)]
    representative: Option<ApiPlayer>,
    #[serde(default)]
    narrator: Option<ApiPlayer>,
    #[serde(default)]
    approximate_teams_count: Option<i64>,
    #[serde(default)]
    issued_at: Option<String>,
    #[serde(default)]
    date_start: Option<String>,
}

impl From<ApiRequest> for TournamentRequest {
    fn from(r: ApiRequest) -> Self {
        TournamentRequest {
            id: r.id,
            status: r.status,
            tournament_id: r.tournament_id,
            venue: r.venue.map(Into::into),
            representative: r.representative.map(Into::into),
            narrator: r.narrator.map(Into::into),
            approximate_teams_count: r.approximate_teams_count,
            issued_at: r.issued_at,
            date_start: r.date_start,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiAppeal {
    id: i64,
    idtournament: i64,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    status: String,
    question_number: u32,
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    appeal: Option<String>,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default)]
    issued_at: Option<String>,
    #[serde(default)]
    overridden_by_id: Option<i64>,
}

impl From<ApiAppeal> for Appeal {
    fn from(a: ApiAppeal) -> Self {
        Appeal {
            id: a.id,
            tournament_id: a.idtournament,
            kind: a.kind,
            status: a.status,
            question_number: a.question_number,
            answer: a.answer.unwrap_or_default(),
            appeal: a.appeal.unwrap_or_default(),
            comment: a.comment,
            issued_at: a.issued_at,
            overridden_by_id: a.overridden_by_id,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiTeamTournament {
    idteam: i64,
    idtournament: i64,
}

#[derive(Debug, Deserialize)]
struct ApiPlayerTournament {
    idplayer: i64,
    idteam: i64,
    idtournament: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiSeasonEntry {
    idplayer: i64,
    idteam: i64,
    idseason: i64,
    #[serde(default)]
    date_added: Option<String>,
    #[serde(default)]
    date_removed: Option<String>,
    #[serde(default)]
    player_number: i64,
}

impl From<ApiSeasonEntry> for SeasonMembership {
    fn from(e: ApiSeasonEntry) -> Self {
        SeasonMembership {
            player_id: e.idplayer,
            team_id: e.idteam,
            season_id: e.idseason,
            date_added: e.date_added,
            date_removed: e.date_removed,
            player_number: e.player_number,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiSeason {
    id: i64,
    date_start: String,
    date_end: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRelease {
    id: i64,
    date: String,
    #[serde(default)]
    real_date: Option<String>,
    #[serde(default)]
    last_run_refresh: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiVenueType {
    id: i64,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiTeamFlag {
    id: i64,
    name: String,
    #[serde(default)]
    full_name: String,
    #[serde(default)]
    short_name: String,
    #[serde(default)]
    value: i64,
}

#[derive(Debug, Deserialize)]
struct ApiRegionFull {
    id: i64,
    name: String,
    #[serde(default)]
    country: Option<ApiCountry>,
}

#[derive(Debug, Deserialize)]
struct ApiCountryFull {
    id: i64,
    name: String,
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
    current: Option<ApiCurrent>,
    #[serde(default)]
    controversials: Vec<ApiControversial>,
    #[serde(default)]
    flags: Vec<Option<String>>,
    #[serde(default, rename = "teamMembers")]
    team_members: Vec<ApiTeamMember>,
}

#[derive(Debug, Deserialize)]
struct ApiCurrent {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiTeamMember {
    player: ApiPlayer,
    #[serde(default)]
    flag: Option<String>,
    #[serde(default)]
    rating: Option<i64>,
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
    id: Option<i64>,
    #[serde(default)]
    venue: Option<ApiIdOnly>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiControversial {
    #[serde(default)]
    id: Option<i64>,
    question_number: u32,
    #[serde(default)]
    status: String,
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    issued_at: Option<String>,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default)]
    resolved_at: Option<String>,
    #[serde(default)]
    appeal_jury_comment: Option<String>,
}

impl From<ApiControversial> for Controversial {
    fn from(c: ApiControversial) -> Self {
        Controversial {
            id: c.id,
            question_number: c.question_number,
            status: c.status,
            answer: c.answer.unwrap_or_default(),
            issued_at: c.issued_at,
            comment: c.comment,
            resolved_at: c.resolved_at,
            appeal_jury_comment: c.appeal_jury_comment,
        }
    }
}

/// Filters for [`ApiClient::search_tournaments`]. All optional; build
/// with [`TournamentQuery::new`] and the chained setters (the struct is
/// `#[non_exhaustive]`, so a literal cannot be used).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TournamentQuery {
    /// Substring of the name, case-insensitive.
    pub name: Option<String>,
    /// Tournament type id: 2 regular, 3 synchronous, 6 strict synchronous,
    /// 8 asynchronous.
    pub type_id: Option<i64>,
    /// Archived synchronous tournaments only (`true`) or none (`false`).
    pub archive: Option<bool>,
    /// `dateStart[after]`, ISO date or timestamp.
    pub date_start_after: Option<String>,
    /// `dateStart[before]`.
    pub date_start_before: Option<String>,
    /// `lastEditDate[after]` — tournaments with rating-relevant changes
    /// since then.
    pub last_edit_after: Option<String>,
    /// Order by id descending instead of the default ascending.
    pub newest_first: bool,
    /// 1-based page; `0` means the first.
    pub page: u32,
    /// Rows per page; `0` means the site's default (30).
    pub limit: usize,
}

/// Options for [`ApiClient::get_tournament_results_with`]. Build with
/// [`ResultsQuery::new`] and the chained setters (the struct is
/// `#[non_exhaustive]`, so a literal cannot be used).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResultsQuery {
    /// Include `mask` and `controversials` (default `true`).
    pub masks_and_controversials: bool,
    /// Include the roster as played (`team_members`).
    pub team_members: bool,
    /// Include the team flags (зачёты).
    pub team_flags: bool,
    /// Only rows played at this venue (synchronous tournaments).
    pub venue: Option<i64>,
    /// Only teams from this town.
    pub town: Option<i64>,
    /// Only teams from this region (ignored when `town` is set).
    pub region: Option<i64>,
    /// Only teams from this country (ignored when `town` or `region` is
    /// set).
    pub country: Option<i64>,
    /// Only teams competing under this flag id.
    pub flag: Option<i64>,
}

impl Default for ResultsQuery {
    fn default() -> Self {
        ResultsQuery {
            masks_and_controversials: true,
            team_members: false,
            team_flags: false,
            venue: None,
            town: None,
            region: None,
            country: None,
            flag: None,
        }
    }
}

impl TournamentQuery {
    /// No filters: the first page of every tournament, oldest first.
    pub fn new() -> TournamentQuery {
        TournamentQuery::default()
    }

    /// Substring of the name, case-insensitive.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Tournament type id (2, 3, 6, 8 — see the field doc).
    pub fn type_id(mut self, type_id: i64) -> Self {
        self.type_id = Some(type_id);
        self
    }

    /// Archived synchronous tournaments only, or none.
    pub fn archive(mut self, archive: bool) -> Self {
        self.archive = Some(archive);
        self
    }

    /// `dateStart[after]`.
    pub fn date_start_after(mut self, date: impl Into<String>) -> Self {
        self.date_start_after = Some(date.into());
        self
    }

    /// `dateStart[before]`.
    pub fn date_start_before(mut self, date: impl Into<String>) -> Self {
        self.date_start_before = Some(date.into());
        self
    }

    /// `lastEditDate[after]`.
    pub fn last_edit_after(mut self, date: impl Into<String>) -> Self {
        self.last_edit_after = Some(date.into());
        self
    }

    /// Order by id descending.
    pub fn newest_first(mut self, newest_first: bool) -> Self {
        self.newest_first = newest_first;
        self
    }

    /// 1-based page.
    pub fn page(mut self, page: u32) -> Self {
        self.page = page;
        self
    }

    /// Rows per page.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

impl ResultsQuery {
    /// Masks and controversials included, nothing else, no filter.
    pub fn new() -> ResultsQuery {
        ResultsQuery::default()
    }

    /// Include `mask` and `controversials`.
    pub fn masks_and_controversials(mut self, include: bool) -> Self {
        self.masks_and_controversials = include;
        self
    }

    /// Include the roster as played.
    pub fn team_members(mut self, include: bool) -> Self {
        self.team_members = include;
        self
    }

    /// Include the team flags.
    pub fn team_flags(mut self, include: bool) -> Self {
        self.team_flags = include;
        self
    }

    /// Only rows played at this venue.
    pub fn venue(mut self, venue_id: i64) -> Self {
        self.venue = Some(venue_id);
        self
    }

    /// Only teams from this town.
    pub fn town(mut self, town_id: i64) -> Self {
        self.town = Some(town_id);
        self
    }

    /// Only teams from this region.
    pub fn region(mut self, region_id: i64) -> Self {
        self.region = Some(region_id);
        self
    }

    /// Only teams from this country.
    pub fn country(mut self, country_id: i64) -> Self {
        self.country = Some(country_id);
        self
    }

    /// Only teams competing under this flag.
    pub fn flag(mut self, flag_id: i64) -> Self {
        self.flag = Some(flag_id);
        self
    }
}

impl ApiClient {
    // ---- players -----------------------------------------------------------

    /// `GET /players/{id}`.
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_player(&self, player_id: i64) -> Result<Player> {
        let p: ApiPlayer = self
            .get_json(&format!("/players/{}", player_id), &[])
            .await?;
        Ok(p.into())
    }

    /// `GET /players?surname=…[&name=…]`. The query is split on the first
    /// space into surname and (optional) name.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn search_players(&self, query: &str, limit: usize) -> Result<Vec<Player>> {
        let (surname, name) = split_player_query(query);
        let limit = limit.to_string();
        let mut q = vec![("surname", surname), ("itemsPerPage", limit.as_str())];
        if let Some(name) = name {
            q.push(("name", name));
        }
        let resp: Vec<ApiPlayer> = self.get_json("/players", &q).await?;
        Ok(resp.into_iter().map(Into::into).collect())
    }

    /// [`search_players`](ApiClient::search_players), but a query that is a
    /// bare number is first tried as a player id.
    ///
    /// # Errors
    /// As [`search_players`](ApiClient::search_players).
    pub async fn find_players(&self, query: &str, limit: usize) -> Result<Vec<Player>> {
        if let Ok(id) = query.trim().parse::<i64>() {
            match self.get_player(id).await {
                Ok(p) => return Ok(vec![p]),
                Err(e) if e.status() == Some(404) => {}
                Err(e) => return Err(e),
            }
        }
        self.search_players(query, limit).await
    }

    // ---- teams -------------------------------------------------------------

    /// `GET /teams/{id}`.
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_team(&self, team_id: i64) -> Result<Team> {
        let t: ApiTeam = self.get_json(&format!("/teams/{}", team_id), &[]).await?;
        Ok(t.into_team(team_id))
    }

    /// `GET /teams?name=…` (case-insensitive substring match).
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn search_teams(&self, query: &str, limit: usize) -> Result<Vec<Team>> {
        let resp: Vec<ApiTeam> = self
            .get_json(
                "/teams",
                &[("name", query), ("itemsPerPage", &limit.to_string())],
            )
            .await?;
        Ok(resp.into_iter().map(|t| t.into_team(0)).collect())
    }

    /// [`search_teams`](ApiClient::search_teams), but a query that is a
    /// bare number is first tried as a team id.
    ///
    /// # Errors
    /// As [`search_teams`](ApiClient::search_teams).
    pub async fn find_teams(&self, query: &str, limit: usize) -> Result<Vec<Team>> {
        if let Ok(id) = query.trim().parse::<i64>() {
            match self.get_team(id).await {
                Ok(t) => return Ok(vec![t]),
                Err(e) if e.status() == Some(404) => {}
                Err(e) => return Err(e),
            }
        }
        self.search_teams(query, limit).await
    }
    /// A team's current base roster: the players listed for its latest
    /// season in `/teams/{id}/seasons` and not yet removed from it. A
    /// player the site no longer knows (404) is skipped with a warning;
    /// any other failure, including a failed page fetch, is an error
    /// (treating it as end-of-list used to silently truncate rosters).
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`], [`Error::Parse`].
    pub async fn get_base_roster(&self, team_id: i64) -> Result<Vec<Player>> {
        let all = self.get_team_seasons(team_id).await?;
        let latest = all.iter().map(|e| e.season_id).max().unwrap_or(0);
        let mut ids: Vec<i64> = all
            .iter()
            .filter(|e| e.season_id == latest && e.date_removed.is_none())
            .map(|e| e.player_id)
            .collect();
        ids.sort_unstable();
        ids.dedup();

        let mut players = Vec::with_capacity(ids.len());
        for pid in ids {
            match self.get_player(pid).await {
                Ok(p) => players.push(p),
                Err(e) if e.status() == Some(404) => tracing::warn!(
                    "base roster of team {}: player {} not found, skipped",
                    team_id,
                    pid
                ),
                Err(e) => return Err(e),
            }
        }
        Ok(players)
    }

    // ---- towns / countries -------------------------------------------------

    /// `GET /towns?name=…`.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn search_towns(&self, query: &str, limit: usize) -> Result<Vec<Town>> {
        let resp: Vec<ApiTown> = self
            .get_json(
                "/towns",
                &[("name", query), ("itemsPerPage", &limit.to_string())],
            )
            .await?;
        Ok(resp
            .into_iter()
            .filter_map(|t| {
                let country = t.country_name().unwrap_or_default();
                let id = t.id?;
                t.name.map(|name| Town { id, name, country })
            })
            .collect())
    }

    /// Numeric id of the town whose name equals `name` (case-insensitive).
    /// Needed by [`SiteClient::create_team`](crate::site::SiteClient::create_team).
    ///
    /// # Errors
    /// As [`search_towns`](ApiClient::search_towns).
    pub async fn lookup_town_id(&self, name: &str) -> Result<Option<i64>> {
        let needle = name.trim().to_lowercase();
        Ok(self
            .search_towns(name.trim(), LOOKUP_WINDOW)
            .await?
            .into_iter()
            .find(|t| t.name.to_lowercase() == needle)
            .map(|t| t.id))
    }
    /// `GET /countries?name=…`.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn search_countries(&self, query: &str, limit: usize) -> Result<Vec<Country>> {
        let resp: Vec<ApiCountry> = self
            .get_json(
                "/countries",
                &[("name", query), ("itemsPerPage", &limit.to_string())],
            )
            .await?;
        Ok(resp
            .into_iter()
            .filter_map(|c| {
                Some(Country {
                    id: c.id?,
                    name: c.name?,
                })
            })
            .collect())
    }

    /// The country whose name equals `name` (case-insensitive), if any —
    /// its id is what [`ResultsQuery::country`] wants.
    ///
    /// # Errors
    /// As [`search_countries`](ApiClient::search_countries).
    pub async fn lookup_country(&self, name: &str) -> Result<Option<Country>> {
        let needle = name.trim().to_lowercase();
        let results = self.search_countries(name.trim(), LOOKUP_WINDOW).await?;
        Ok(results
            .into_iter()
            .find(|c| c.name.to_lowercase() == needle))
    }

    // ---- venues ------------------------------------------------------------

    /// `GET /venues/{id}`.
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_venue(&self, venue_id: i64) -> Result<Venue> {
        let v: ApiVenue = self.get_json(&format!("/venues/{}", venue_id), &[]).await?;
        Ok(v.into())
    }
    /// Town name of a venue, or empty.
    ///
    /// # Errors
    /// As [`get_venue`](ApiClient::get_venue).
    #[deprecated(since = "0.2.0", note = "use `get_venue(id)?.town_name()`")]
    pub async fn get_venue_town(&self, venue_id: i64) -> Result<String> {
        Ok(self.get_venue(venue_id).await?.town_name())
    }

    /// `GET /venues?name=…`.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn search_venues(&self, query: &str, limit: usize) -> Result<Vec<Venue>> {
        let resp: Vec<ApiVenue> = self
            .get_json(
                "/venues",
                &[("name", query), ("itemsPerPage", &limit.to_string())],
            )
            .await?;
        Ok(resp.into_iter().map(Into::into).collect())
    }

    // ---- tournaments -------------------------------------------------------

    /// `GET /tournaments/{id}`.
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_tournament(&self, tournament_id: i64) -> Result<Tournament> {
        let t: ApiTournament = self
            .get_json(&format!("/tournaments/{}", tournament_id), &[])
            .await?;
        Ok(t.into())
    }

    /// Synchronous tournaments played at a venue, most recent first.
    ///
    /// `/tournaments` has no venue filter (see `docs/api_notes.md` §1), so
    /// this walks every page of `/venues/{id}/requests`, keeps approved
    /// (`A`) and new (`N`) requests, sorts by `dateStart` descending, and
    /// resolves the 100 most recent through `/tournaments/{id}`. A
    /// tournament the site no longer knows (404) is skipped with a
    /// warning; any other failure is an error.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`], [`Error::Parse`].
    pub async fn list_synch_tournaments_for_venue(
        &self,
        venue_id: i64,
    ) -> Result<Vec<SynchTournament>> {
        let mut all = self.get_venue_requests(venue_id).await?;
        all.retain(|r| matches!(r.status.as_str(), "A" | "N"));
        all.sort_by(|a, b| b.date_start.cmp(&a.date_start));

        let mut out: Vec<SynchTournament> = Vec::new();
        for r in all.into_iter().take(100) {
            let Some(tournament_id) = r.tournament_id else {
                tracing::warn!(
                    "venue {}: request without a tournament id skipped",
                    venue_id
                );
                continue;
            };
            let t = match self.get_tournament(tournament_id).await {
                Ok(t) => t,
                Err(e) if e.status() == Some(404) => {
                    tracing::warn!(
                        "venue {}: tournament {} not found, skipped",
                        venue_id,
                        tournament_id
                    );
                    continue;
                }
                Err(e) => return Err(e),
            };
            out.push(SynchTournament {
                id: t.id,
                name: t.name,
                date: r.date_start.or(t.date_start).unwrap_or_default(),
                questions_per_round: t.questions_per_round,
                representative_id: r.representative.map(|p| p.id),
                status: Some(r.status),
                orgcommittee: t.orgcommittee,
            });
        }
        Ok(out)
    }

    /// `GET /tournaments/{id}/results?includeMasksAndControversials=1` —
    /// one row per team, with answer masks and controversial verdicts.
    /// `mask`, `questions_total` and `position` are `None` until the
    /// results are published.
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_tournament_results(
        &self,
        tournament_id: i64,
    ) -> Result<Vec<TournamentResult>> {
        self.get_tournament_results_with(tournament_id, &ResultsQuery::default())
            .await
    }

    /// [`get_tournament_results`](ApiClient::get_tournament_results) with
    /// explicit includes and filters.
    ///
    /// # Errors
    /// As [`get_tournament_results`](ApiClient::get_tournament_results).
    pub async fn get_tournament_results_with(
        &self,
        tournament_id: i64,
        q: &ResultsQuery,
    ) -> Result<Vec<TournamentResult>> {
        let flag = |b: bool| if b { "1" } else { "0" };
        let ids: Vec<(&str, String)> = [
            ("venue", q.venue),
            ("town", q.town),
            ("region", q.region),
            ("country", q.country),
            ("flag", q.flag),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k, v.to_string())))
        .collect();
        let mut query: Vec<(&str, &str)> = vec![
            (
                "includeMasksAndControversials",
                flag(q.masks_and_controversials),
            ),
            ("includeTeamMembers", flag(q.team_members)),
            ("includeTeamFlags", flag(q.team_flags)),
        ];
        query.extend(ids.iter().map(|(k, v)| (*k, v.as_str())));
        let resp: Vec<ApiResultRow> = self
            .get_json(&format!("/tournaments/{}/results", tournament_id), &query)
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
                venue_id: r
                    .synch_request
                    .as_ref()
                    .and_then(|s| s.venue.as_ref().map(|v| v.id)),
                synch_request_id: r.synch_request.and_then(|s| s.id),
                current_name: r.current.and_then(|c| c.name),
                controversials: r.controversials.into_iter().map(Into::into).collect(),
                flags: r.flags.into_iter().flatten().collect(),
                team_members: r
                    .team_members
                    .into_iter()
                    .map(|m| TeamMember {
                        player: m.player.into(),
                        flag: m.flag,
                        rating: m.rating,
                    })
                    .collect(),
            })
            .collect())
    }

    // ---- tournaments: search, requests, appeals, controversials ----------

    /// `GET /tournaments` with the filters of [`TournamentQuery`]. One
    /// page; use `page` / `limit` to walk further.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn search_tournaments(&self, q: &TournamentQuery) -> Result<Vec<Tournament>> {
        let mut owned: Vec<(&str, String)> = Vec::new();
        if let Some(n) = &q.name {
            owned.push(("name", n.clone()));
        }
        if let Some(t) = q.type_id {
            owned.push(("type", t.to_string()));
        }
        if let Some(a) = q.archive {
            owned.push(("archive", a.to_string()));
        }
        if let Some(d) = &q.date_start_after {
            owned.push(("dateStart[after]", d.clone()));
        }
        if let Some(d) = &q.date_start_before {
            owned.push(("dateStart[before]", d.clone()));
        }
        if let Some(d) = &q.last_edit_after {
            owned.push(("lastEditDate[after]", d.clone()));
        }
        if q.newest_first {
            owned.push(("order[id]", "desc".into()));
        }
        if q.page > 0 {
            owned.push(("page", q.page.to_string()));
        }
        if q.limit > 0 {
            owned.push(("itemsPerPage", q.limit.to_string()));
        }
        let query: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let resp: Vec<ApiTournament> = self.get_json("/tournaments", &query).await?;
        Ok(resp.into_iter().map(Into::into).collect())
    }

    /// `GET /tournaments/{id}/intersections` — tournaments sharing
    /// questions with this one (e.g. the online playing of a synch).
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn get_tournament_intersections(
        &self,
        tournament_id: i64,
    ) -> Result<Vec<Tournament>> {
        let resp: Vec<ApiTournament> = self
            .get_json(
                &format!("/tournaments/{}/intersections", tournament_id),
                &[],
            )
            .await?;
        Ok(resp.into_iter().map(Into::into).collect())
    }

    /// `GET /tournaments/{id}/requests` — every venue request for a
    /// (a)synchronous tournament, all pages.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn get_tournament_requests(
        &self,
        tournament_id: i64,
    ) -> Result<Vec<TournamentRequest>> {
        let rows: Vec<ApiRequest> = self
            .get_all_pages(
                &format!("/tournaments/{}/requests", tournament_id),
                &[],
                REQUEST_PAGES,
            )
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// `GET /venues/{id}/requests` — every request a venue has filed, all
    /// pages, in ascending id order.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn get_venue_requests(&self, venue_id: i64) -> Result<Vec<TournamentRequest>> {
        let rows: Vec<ApiRequest> = self
            .get_all_pages(
                &format!("/venues/{}/requests", venue_id),
                &[],
                REQUEST_PAGES,
            )
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// `GET /tournament_synch_requests/{id}` — one venue request, with its
    /// venue, representative and narrator.
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_synch_request(&self, request_id: i64) -> Result<TournamentRequest> {
        let r: ApiRequest = self
            .get_json(&format!("/tournament_synch_requests/{}", request_id), &[])
            .await?;
        Ok(r.into())
    }

    /// `GET /tournaments/{id}/appeals`.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn get_tournament_appeals(&self, tournament_id: i64) -> Result<Vec<Appeal>> {
        let rows: Vec<ApiAppeal> = self
            .get_json(&format!("/tournaments/{}/appeals", tournament_id), &[])
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// `GET /tournament_synch_controversials/{id}` — one controversial
    /// record by the id carried in [`Controversial::id`].
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_controversial(&self, controversial_id: i64) -> Result<Controversial> {
        let c: ApiControversial = self
            .get_json(
                &format!("/tournament_synch_controversials/{}", controversial_id),
                &[],
            )
            .await?;
        Ok(c.into())
    }

    // ---- histories ---------------------------------------------------------

    /// `GET /teams/{id}/tournaments` — every tournament the team played.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn get_team_tournaments(&self, team_id: i64) -> Result<Vec<TeamTournament>> {
        let rows: Vec<ApiTeamTournament> = self
            .get_json(&format!("/teams/{}/tournaments", team_id), &[])
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| TeamTournament {
                team_id: r.idteam,
                tournament_id: r.idtournament,
            })
            .collect())
    }

    /// `GET /players/{id}/tournaments` — every tournament the player
    /// played, with the team.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn get_player_tournaments(&self, player_id: i64) -> Result<Vec<PlayerTournament>> {
        let rows: Vec<ApiPlayerTournament> = self
            .get_json(&format!("/players/{}/tournaments", player_id), &[])
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| PlayerTournament {
                player_id: r.idplayer,
                team_id: r.idteam,
                tournament_id: r.idtournament,
            })
            .collect())
    }

    /// `GET /players/{id}/seasons` — the player's base-squad memberships,
    /// all seasons.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn get_player_seasons(&self, player_id: i64) -> Result<Vec<SeasonMembership>> {
        let rows: Vec<ApiSeasonEntry> = self
            .get_all_pages(&format!("/players/{}/seasons", player_id), &[], 100)
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// `GET /teams/{id}/seasons` — the team's base squads, all seasons.
    /// [`get_base_roster`](ApiClient::get_base_roster) is the latest
    /// season of this resolved to players.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn get_team_seasons(&self, team_id: i64) -> Result<Vec<SeasonMembership>> {
        let rows: Vec<ApiSeasonEntry> = self
            .get_all_pages(&format!("/teams/{}/seasons", team_id), &[], 100)
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    // ---- reference lists and by-id lookups ----------------------------------

    /// `GET /seasons` — all rating seasons (one page; the site returns
    /// them unpaginated).
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn list_seasons(&self) -> Result<Vec<Season>> {
        let rows: Vec<ApiSeason> = self.get_json("/seasons", &[]).await?;
        Ok(rows.into_iter().map(season).collect())
    }

    /// `GET /seasons/{id}`.
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_season(&self, season_id: i64) -> Result<Season> {
        let s: ApiSeason = self
            .get_json(&format!("/seasons/{}", season_id), &[])
            .await?;
        Ok(season(s))
    }

    /// `GET /releases` — all rating releases, all pages.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn list_releases(&self) -> Result<Vec<Release>> {
        let rows: Vec<ApiRelease> = self.get_all_pages("/releases", &[], 20).await?;
        Ok(rows.into_iter().map(release).collect())
    }

    /// `GET /releases/{id}`.
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_release(&self, release_id: i64) -> Result<Release> {
        let r: ApiRelease = self
            .get_json(&format!("/releases/{}", release_id), &[])
            .await?;
        Ok(release(r))
    }

    /// `GET /languages`.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn list_languages(&self) -> Result<Vec<Language>> {
        let rows: Vec<ApiLanguage> = self.get_json("/languages", &[]).await?;
        Ok(rows
            .into_iter()
            .map(|l| Language {
                id: l.id,
                name: l.name,
            })
            .collect())
    }

    /// `GET /venue_types`.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn list_venue_types(&self) -> Result<Vec<VenueType>> {
        let rows: Vec<ApiVenueType> = self.get_all_pages("/venue_types", &[], 5).await?;
        Ok(rows
            .into_iter()
            .map(|v| VenueType {
                id: v.id,
                name: v.name,
            })
            .collect())
    }

    /// `GET /tournament_team_flags` — the flags (зачёты) teams can
    /// compete under.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn list_team_flags(&self) -> Result<Vec<TeamFlag>> {
        let rows: Vec<ApiTeamFlag> = self.get_json("/tournament_team_flags", &[]).await?;
        Ok(rows
            .into_iter()
            .map(|f| TeamFlag {
                id: f.id,
                name: f.name,
                full_name: f.full_name,
                short_name: f.short_name,
                value: f.value,
            })
            .collect())
    }

    /// `GET /regions` — all regions, all pages.
    ///
    /// # Errors
    /// [`Error::Status`], [`Error::Http`].
    pub async fn list_regions(&self) -> Result<Vec<Region>> {
        let rows: Vec<ApiRegionFull> = self.get_all_pages("/regions", &[], 50).await?;
        Ok(rows
            .into_iter()
            .map(|r| Region {
                id: r.id,
                name: r.name,
                country: r.country.and_then(|c| c.name).unwrap_or_default(),
            })
            .collect())
    }

    /// `GET /towns/{id}`.
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_town(&self, town_id: i64) -> Result<Town> {
        let path = format!("/towns/{}", town_id);
        let t: ApiTown = self.get_json(&path, &[]).await?;
        let country = t.country_name().unwrap_or_default();
        let name = t
            .name
            .ok_or_else(|| Error::Parse(format!("{}: town without a name", path)))?;
        Ok(Town {
            id: t.id.unwrap_or(town_id),
            country,
            name,
        })
    }

    /// `GET /countries/{id}`.
    ///
    /// # Errors
    /// [`Error::Status`] (404 for an unknown id), [`Error::Http`].
    pub async fn get_country(&self, country_id: i64) -> Result<Country> {
        let c: ApiCountryFull = self
            .get_json(&format!("/countries/{}", country_id), &[])
            .await?;
        Ok(Country {
            id: c.id,
            name: c.name,
        })
    }

    /// All controversial verdicts of a tournament, flattened. Empty when
    /// the tournament has none.
    ///
    /// # Errors
    /// As [`get_tournament_results`](ApiClient::get_tournament_results).
    pub async fn get_tournament_controversials(
        &self,
        tournament_id: i64,
    ) -> Result<Vec<ControversialVerdict>> {
        let rows = self.get_tournament_results(tournament_id).await?;
        Ok(rows
            .into_iter()
            .flat_map(|r| {
                r.controversials
                    .into_iter()
                    .map(move |c| ControversialVerdict {
                        team_id: r.team_id,
                        question_number: c.question_number,
                        status: c.status,
                    })
            })
            .collect())
    }
}

/// Rows fetched by the `lookup_*` helpers before giving up on an exact
/// match.
const LOOKUP_WINDOW: usize = 100;

/// Page cap for request lists: 20 000 requests per venue or tournament.
const REQUEST_PAGES: u32 = 200;

fn season(s: ApiSeason) -> Season {
    Season {
        id: s.id,
        date_start: s.date_start,
        date_end: s.date_end,
    }
}

fn release(r: ApiRelease) -> Release {
    Release {
        id: r.id,
        date: r.date,
        real_date: r.real_date.unwrap_or_default(),
        last_run_refresh: r.last_run_refresh.unwrap_or_default(),
    }
}

/// `"Михайлюк Роберт"` → `("Михайлюк", Some("Роберт"))`; a single token
/// has no name part.
fn split_player_query(query: &str) -> (&str, Option<&str>) {
    let mut parts = query.trim().splitn(2, ' ');
    let surname = parts.next().unwrap_or_default();
    let name = parts.next().map(str::trim).filter(|n| !n.is_empty());
    (surname, name)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn question_qty_array_and_object() {
        assert_eq!(
            parse_question_qty(Some(json!([12, 12, 12]))),
            vec![12, 12, 12]
        );
        assert_eq!(
            parse_question_qty(Some(json!({"2": 15, "1": 12, "3": 9}))),
            vec![12, 15, 9]
        );
        assert_eq!(parse_question_qty(Some(json!("nope"))), Vec::<u32>::new());
        assert_eq!(parse_question_qty(None), Vec::<u32>::new());
    }

    #[test]
    fn player_query_splits_on_first_space() {
        assert_eq!(
            split_player_query("Михайлюк Роберт"),
            ("Михайлюк", Some("Роберт"))
        );
        assert_eq!(split_player_query("  Михайлюк  "), ("Михайлюк", None));
        assert_eq!(
            split_player_query("Иванов Иван Иванович"),
            ("Иванов", Some("Иван Иванович"))
        );
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
        assert_eq!(t.into_team(77174).id, 77174);
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
