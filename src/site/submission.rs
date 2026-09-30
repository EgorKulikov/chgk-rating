//! JSON writes used by the representative's results-entry page.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{
    format_description::well_known::Rfc3339, macros::format_description, Date, OffsetDateTime,
};

use super::{is_login_page, read_response, RenamedTeam, RosterUploadReport, SiteClient};
use crate::csv::{Mark, ResultsRoundRow, RosterFlag, RosterRow};
use crate::text::summarize_response;
use crate::{ApiClient, Error, Player, Result, SeasonMembership};

#[derive(Deserialize)]
struct CreatedPlayer {
    id: i64,
    surname: String,
    name: String,
    #[serde(default)]
    patronymic: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RosterTeam<'a> {
    team_id: i64,
    team_name: &'a str,
    rows: Vec<RosterPlayer>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RosterPlayer {
    player_id: i64,
    flag: char,
    checked: bool,
}

#[derive(Deserialize)]
struct ResultsSnapshot {
    meta: ResultsMeta,
    teams: Vec<ResultsTeam>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResultsMeta {
    tournament_id: i64,
    question_count: u32,
}

#[derive(Deserialize, Serialize)]
struct ResultsTeam {
    id: i64,
    questions: BTreeMap<u32, Value>,
}

impl SiteClient {
    /// Upload per-round marks through the representative's JSON results API.
    /// Uses a default [`ApiClient`] for the tournament's round sizes; use
    /// [`Self::upload_results_with`] for a custom public API client or mirror.
    ///
    /// Each supplied row replaces a complete round. Omitted rounds and their
    /// controversial answers are read from the authenticated results endpoint
    /// and retained. Other teams are not submitted. Team names and towns are
    /// managed by roster submission; this endpoint changes only answers.
    /// Incomplete or ambiguous legacy snapshots require all rounds for that
    /// team, because the server can misnumber sparse saved rounds.
    ///
    /// Reading and saving are separate requests: concurrent edits to the same
    /// team's results can be overwritten (the server has no revision check).
    ///
    /// # Errors
    /// As [`Self::upload_results_with`].
    pub async fn upload_results(&self, tournament_id: i64, rows: &[ResultsRoundRow]) -> Result<()> {
        self.upload_results_with(&ApiClient::new()?, tournament_id, rows)
            .await
    }

    /// [`Self::upload_results`] with a public API client for tournament metadata.
    ///
    /// # Errors
    /// [`Error::SessionExpired`], [`Error::Status`], [`Error::Http`],
    /// [`Error::Parse`]; [`Error::Site`] for invalid or duplicate rounds, marks
    /// not matching the configured round size, or unsuccessful saves. Server
    /// warnings are errors because they can mean that some teams were skipped;
    /// other teams may already have been saved. Metadata is limited to 100,000
    /// questions to bound the complete answer map assembled for each team.
    pub async fn upload_results_with(
        &self,
        api: &ApiClient,
        tournament_id: i64,
        rows: &[ResultsRoundRow],
    ) -> Result<()> {
        const ACTION: &str = "upload_results";
        validate_id(ACTION, "tournament", tournament_id)?;
        if rows.is_empty() {
            return Err(Error::site(ACTION, "no result rows to upload"));
        }
        let mut rounds = HashSet::new();
        for row in rows {
            validate_id(ACTION, "team", row.team_id)?;
            if row.round == 0 || !rounds.insert((row.team_id, row.round)) {
                return Err(Error::site(
                    ACTION,
                    format!(
                        "team {} has invalid or duplicate round {}",
                        row.team_id, row.round
                    ),
                ));
            }
        }

        // Public reads can hide masks and controversial text. Resolve access
        // through a read before writing, including organizers without a request.
        let (response, admin) = self.load_submission(tournament_id, "results").await?;
        let mut snapshot: ResultsSnapshot = serde_json::from_value(response)
            .map_err(|e| Error::Parse(format!("results snapshot: {e}")))?;
        let tournament = api.get_tournament(tournament_id).await?;
        let qty = &tournament.questions_per_round;
        let total = qty
            .iter()
            .try_fold(0_u32, |sum, &count| sum.checked_add(count));
        let total = total.filter(|&n| n > 0 && n <= 100_000).ok_or_else(|| {
            Error::Parse("tournament question count is missing or exceeds 100,000".into())
        })?;
        if qty.contains(&0)
            || snapshot.meta.tournament_id != tournament_id
            || tournament.id != tournament_id
            || snapshot.meta.question_count != total
        {
            return Err(Error::Parse(
                "inconsistent tournament question metadata".into(),
            ));
        }
        // Organizers may write another request's team even when the default
        // GET omits it. Ask for the full authorized view before treating a team
        // as new; ordinary representatives are safely scoped down by the server.
        let known: HashSet<_> = snapshot.teams.iter().map(|team| team.id).collect();
        let missing: HashSet<_> = rows
            .iter()
            .map(|row| row.team_id)
            .filter(|id| !known.contains(id))
            .collect();
        if !admin && !missing.is_empty() {
            let full: ResultsSnapshot =
                serde_json::from_value(self.get_submission(tournament_id, "results", true).await?)
                    .map_err(|e| Error::Parse(format!("full results snapshot: {e}")))?;
            if full.meta.tournament_id != tournament_id || full.meta.question_count != total {
                return Err(Error::Parse(
                    "inconsistent full results snapshot metadata".into(),
                ));
            }
            snapshot.teams.extend(
                full.teams
                    .into_iter()
                    .filter(|team| missing.contains(&team.id)),
            );
        }
        let mut existing = BTreeMap::new();
        for team in snapshot.teams {
            let supplied_rounds = rounds.iter().filter(|&&(id, _)| id == team.id).count();
            // The GET overlays controversy strings on the trailing nulls left
            // by missing masks. Hiding a whole missing round therefore requires
            // at least the smallest round's worth of trailing string answers.
            let trailing_text = team
                .questions
                .values()
                .rev()
                .take_while(|v| v.is_string())
                .count();
            let could_hide_missing_round =
                trailing_text >= *qty.iter().min().expect("nonempty rounds") as usize;
            if supplied_rounds > 0
                && supplied_rounds < qty.len()
                && (team.questions.len() != total as usize
                    || team.questions.values().any(Value::is_null)
                    || could_hide_missing_round)
            {
                // rating-site concatenates sparse saved tour masks, losing
                // their offsets. A partial snapshot cannot safely identify the
                // omitted rounds: require authoritative input for the whole team.
                return Err(Error::site(
                    ACTION,
                    format!(
                    "team {} has an incomplete or ambiguous results snapshot; upload all rounds for this team",
                    team.id,
                ),
                ));
            }
            if team.questions.iter().any(|(&number, value)| {
                number == 0
                    || number > total
                    || !(value.is_null()
                        || value.is_string()
                        || value == &json!(0)
                        || value == &json!(1))
            }) || existing.insert(team.id, team.questions).is_some()
            {
                return Err(Error::Parse(
                    "invalid or duplicate team in saved results".into(),
                ));
            }
        }
        let mut teams: BTreeMap<i64, ResultsTeam> = BTreeMap::new();
        for row in rows {
            let round = (row.round - 1) as usize;
            if qty.get(round).copied().map(|n| n as usize) != Some(row.marks.len()) {
                return Err(Error::site(
                    ACTION,
                    format!(
                        "team {} round {} has {} marks; expected {:?}",
                        row.team_id,
                        row.round,
                        row.marks.len(),
                        qty.get(round),
                    ),
                ));
            }
            let team = teams.entry(row.team_id).or_insert_with(|| {
                let mut questions = existing.remove(&row.team_id).unwrap_or_default();
                for question in 1..=total {
                    questions.entry(question).or_insert(json!(0));
                }
                ResultsTeam {
                    id: row.team_id,
                    questions,
                }
            });
            let offset: u32 = qty[..round].iter().sum();
            for (index, mark) in row.marks.iter().enumerate() {
                let value = match mark {
                    Mark::Correct => json!(1),
                    Mark::Wrong | Mark::Blank => json!(0),
                    Mark::Contested => {
                        let text = row
                            .controversials
                            .get(&index)
                            .map(String::as_str)
                            .unwrap_or("?")
                            .trim();
                        if text.is_empty() || text == "0" || text == "1" {
                            return Err(Error::site(
                                ACTION,
                                "contested answer must be nonempty text other than 0 or 1",
                            ));
                        }
                        json!(text)
                    }
                };
                team.questions.insert(offset + index as u32 + 1, value);
            }
        }
        let payload: Vec<_> = teams.into_values().collect();
        let response = self
            .post_submission(tournament_id, "results", admin, &json!({"teams": payload}))
            .await?;
        let warnings = expect_saved(ACTION, response)?;
        if !warnings.is_empty() {
            return Err(Error::site(
                ACTION,
                format!(
                    "some results may have been saved; server reported: {}",
                    warnings.join("; "),
                ),
            ));
        }
        Ok(())
    }

    /// Replace the submitted teams' tournament rosters through the site's JSON API.
    /// Other teams are untouched. Team names, including quotes, are sent directly
    /// as tournament-specific names; no CSV sanitising or fix form is involved.
    ///
    /// Explicit [`RosterRow::flag`] values are preserved. For `None`, use `Б` when
    /// the player belongs to this team's base roster in the tournament's season
    /// at its end date, and `Л` otherwise. Membership is fetched once per team
    /// needing inference. Failed lookups never turn into guessed flags.
    ///
    /// # Errors
    /// [`Error::SessionExpired`], [`Error::Status`], [`Error::Http`],
    /// [`Error::Parse`]; [`Error::Site`] for invalid input, missing season/date
    /// metadata needed for inference, or a rejected save (including `saved: true`
    /// with nonempty `errors`). Nonblocking warnings are returned in the report.
    pub async fn upload_rosters(
        &self,
        api: &ApiClient,
        tournament_id: i64,
        rows: &[RosterRow],
    ) -> Result<RosterUploadReport> {
        const ACTION: &str = "upload_rosters";
        validate_id(ACTION, "tournament", tournament_id)?;
        if rows.is_empty() {
            return Err(Error::site(ACTION, "no roster rows to upload"));
        }
        let mut teams: BTreeMap<i64, Vec<&RosterRow>> = BTreeMap::new();
        let mut players = HashSet::new();
        for row in rows {
            validate_id(ACTION, "team", row.team_id)?;
            validate_id(ACTION, "player", row.player_id)?;
            if !players.insert(row.player_id) {
                return Err(Error::site(
                    ACTION,
                    format!("duplicate player {} in roster upload", row.player_id),
                ));
            }
            let team = teams.entry(row.team_id).or_default();
            if row.team_name.trim().is_empty()
                || team
                    .first()
                    .is_some_and(|first| first.team_name != row.team_name)
            {
                return Err(Error::site(
                    ACTION,
                    format!("team {} has empty or inconsistent names", row.team_id),
                ));
            }
            team.push(row);
        }

        let tournament = if rows.iter().any(|r| r.flag.is_none()) {
            let tournament = api.get_tournament(tournament_id).await?;
            if tournament.season_id.is_none() {
                return Err(Error::site(
                    ACTION,
                    "tournament has no season; cannot infer roster flags",
                ));
            }
            Some(tournament)
        } else {
            None
        };
        let mut payload = Vec::with_capacity(teams.len());
        let mut report = RosterUploadReport::default();
        // The server silently skips unknown player IDs during a replacement
        // save, so validate them before risking removal of the existing roster.
        for row in rows {
            let player = api.get_player(row.player_id).await?;
            if player.id != row.player_id {
                return Err(Error::site(
                    ACTION,
                    format!(
                        "player {} resolves to {}; update the roster's player ID before submitting",
                        row.player_id, player.id,
                    ),
                ));
            }
        }
        for (team_id, rows) in teams {
            let name = &rows[0].team_name;
            let registered = api.get_team(team_id).await?;
            if registered.name != *name {
                report.renamed_on_tournament.push(RenamedTeam {
                    team_id,
                    name: name.clone(),
                });
            }
            let mut base_players = HashSet::new();
            if rows.iter().any(|r| r.flag.is_none()) {
                let tournament = tournament
                    .as_ref()
                    .expect("missing flags require a tournament");
                for membership in api.get_team_seasons(team_id).await? {
                    if membership.team_id == team_id
                        && Some(membership.season_id) == tournament.season_id
                        && rows
                            .iter()
                            .any(|r| r.player_id == membership.player_id && r.flag.is_none())
                        && membership_active(&membership, tournament.date_end.as_deref())?
                    {
                        base_players.insert(membership.player_id);
                    }
                }
            }
            payload.push(RosterTeam {
                team_id,
                team_name: name,
                rows: rows
                    .iter()
                    .map(|r| RosterPlayer {
                        player_id: r.player_id,
                        flag: r
                            .flag
                            .unwrap_or_else(|| {
                                if base_players.contains(&r.player_id) {
                                    RosterFlag::Base
                                } else {
                                    RosterFlag::Legionnaire
                                }
                            })
                            .as_char(),
                        checked: true,
                    })
                    .collect(),
            });
        }
        let (_, admin) = self.load_submission(tournament_id, "roster").await?;
        let response = self
            .post_submission(tournament_id, "roster", admin, &json!({"teams": payload}))
            .await?;
        report.warnings = expect_saved(ACTION, response)?;
        Ok(report)
    }

    /// Create a player through the tournament's results-entry API
    /// (`POST /api/tournaments/{id}/representative/players`) and return
    /// the new record, id included. The site allows this to the
    /// tournament's representatives and organisers, which is why a
    /// tournament id is required; the scope (own request or `admin=1`) is
    /// resolved the same way as for roster uploads. `patronymic` may be
    /// empty.
    ///
    /// # Errors
    /// [`Error::SessionExpired`]; [`Error::Status`] when the site refuses
    /// (403 without rights on the tournament, 422 with its validation
    /// message in `summary`); [`Error::Site`] for a non-positive
    /// tournament id; [`Error::Parse`] if the answer is not a player;
    /// [`Error::Http`].
    pub async fn create_player(
        &self,
        tournament_id: i64,
        surname: &str,
        name: &str,
        patronymic: &str,
    ) -> Result<Player> {
        validate_id("create_player", "tournament", tournament_id)?;
        let (_, admin) = self.load_submission(tournament_id, "roster").await?;
        let response = self
            .post_submission(
                tournament_id,
                "players",
                admin,
                &json!({"surname": surname, "name": name, "patronymic": patronymic}),
            )
            .await?;
        let created: CreatedPlayer = serde_json::from_value(response)
            .map_err(|e| Error::Parse(format!("create_player: {e}")))?;
        Ok(Player {
            id: created.id,
            surname: created.surname,
            name: created.name,
            patronymic: created.patronymic.unwrap_or_default(),
        })
    }

    async fn load_submission(&self, tournament_id: i64, resource: &str) -> Result<(Value, bool)> {
        match self.get_submission(tournament_id, resource, false).await {
            Ok(value) => Ok((value, false)),
            // The server authorizes admin=1 itself. This restores organizer
            // access without ever retrying a write or changing a representative's
            // default request when their ordinary scope is already accessible.
            Err(error) if error.status() == Some(403) => Ok((
                self.get_submission(tournament_id, resource, true).await?,
                true,
            )),
            Err(error) => Err(error),
        }
    }

    async fn get_submission(
        &self,
        tournament_id: i64,
        resource: &str,
        admin: bool,
    ) -> Result<Value> {
        let path = submission_path(tournament_id, resource, admin);
        let resp = self
            .http
            .get(self.url(&path))
            .header("Accept", "application/json")
            .send()
            .await?;
        submission_response(resp, &path).await
    }

    async fn post_submission(
        &self,
        tournament_id: i64,
        resource: &str,
        admin: bool,
        payload: &Value,
    ) -> Result<Value> {
        let path = submission_path(tournament_id, resource, admin);
        let resp = self
            .http
            .post(self.url(&path))
            .header("Origin", self.origin())
            .header(
                "Referer",
                self.url(&format!("/tournaments/{tournament_id}")),
            )
            .header("Accept", "application/json")
            .json(payload)
            .send()
            .await?;
        submission_response(resp, &path).await
    }
}

fn submission_path(tournament_id: i64, resource: &str, admin: bool) -> String {
    format!(
        "/api/tournaments/{tournament_id}/representative/{resource}{}",
        if admin { "?admin=1" } else { "" }
    )
}

async fn submission_response(resp: reqwest::Response, path: &str) -> Result<Value> {
    let (status, body) = read_response(resp).await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || is_login_page(&body) {
        return Err(Error::SessionExpired);
    }
    if !status.is_success() {
        return Err(Error::Status {
            status: status.as_u16(),
            url: path.into(),
            summary: summarize_response(&body),
        });
    }
    serde_json::from_str(&body).map_err(|e| Error::Parse(format!("{path}: {e}")))
}

fn expect_saved(action: &str, response: Value) -> Result<Vec<String>> {
    let errors = notices(&response, "errors")?;
    let warnings = notices(&response, "warnings")?;
    if response.get("saved") != Some(&Value::Bool(true))
        || !errors.is_empty()
        || response.get("error").is_some_and(|v| !v.is_null())
    {
        return Err(Error::site(
            action,
            summarize_response(&response.to_string()),
        ));
    }
    Ok(warnings)
}

fn notices(response: &Value, key: &str) -> Result<Vec<String>> {
    match response.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => Ok(values
            .iter()
            .map(|v| summarize_response(v.as_str().unwrap_or(&v.to_string())))
            .collect()),
        Some(_) => Err(Error::Parse(format!(
            "submission response {key} must be an array"
        ))),
    }
}

fn validate_id(action: &str, kind: &str, id: i64) -> Result<()> {
    if id <= 0 {
        return Err(Error::site(action, format!("{kind} id must be positive")));
    }
    Ok(())
}

fn membership_active(membership: &SeasonMembership, date_end: Option<&str>) -> Result<bool> {
    if membership.date_added.is_none() && membership.date_removed.is_none() {
        return Ok(true);
    }
    let date = parse_date(date_end.ok_or_else(|| {
        Error::site(
            "upload_rosters",
            "tournament has no end date; cannot infer time-dependent roster flags",
        )
    })?)?;
    let added = membership
        .date_added
        .as_deref()
        .map(parse_date)
        .transpose()?;
    let removed = membership
        .date_removed
        .as_deref()
        .map(parse_date)
        .transpose()?;
    Ok(added.is_none_or(|start| start <= date) && removed.is_none_or(|end| date < end))
}

fn parse_date(raw: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(raw, &Rfc3339)
        .or_else(|_| {
            Date::parse(raw, format_description!("[year]-[month]-[day]"))
                .map(|d| d.midnight().assume_utc())
        })
        .map_err(|e| Error::Parse(format!("roster membership date {raw:?}: {e}")))
}
