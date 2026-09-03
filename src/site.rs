//! Write-side client for `https://rating.chgk.info` — the admin website,
//! which has no public write API. Every call here mimics a form or AJAX
//! request the site's own pages make; `docs/har_notes.md` is the reference.
//!
//! Flow: [`login`] once to obtain a [`Session`] (the cookies the site
//! issued — never the password), persist it, and build a `reqwest::Client`
//! from it with [`Session::client`] for each write. When the site stops
//! accepting the cookies, writes fail with [`Error::SessionExpired`]; log in
//! again and retry.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::{Client, Url};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Deserialize, Serialize};

use crate::csv::{build_results_csv, build_rosters_csv, ResultsRoundRow, RosterRow};
use crate::{Error, Result};

pub const SITE: &str = "https://rating.chgk.info";

// ---- sessions --------------------------------------------------------------

/// A logged-in rating.chgk.info session: the site's cookies (`PHPSESSID`,
/// `REMEMBERME`) serialised as JSON, plus who they belong to.
///
/// Serialisable; field names are stable so callers can persist it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// `CookieStore::save_json` output.
    pub cookies_json: String,
    /// The account's own player id, scraped from the homepage after login.
    #[serde(default)]
    pub player_id: Option<i64>,
    /// The login name used (an e-mail address), for display.
    #[serde(default)]
    pub login: String,
    /// Unix timestamp of the login.
    pub logged_in_at: i64,
}

impl Session {
    /// Build a session from raw cookie name/value pairs (e.g. copied from a
    /// browser or produced by [`Session::cookie_header`]). The inverse of
    /// `cookie_header`; `player_id` is whatever the caller knows.
    pub fn from_cookies(cookies: &[(&str, &str)], login: &str, player_id: Option<i64>) -> Result<Session> {
        let jar = Arc::new(CookieStoreMutex::new(CookieStore::default()));
        {
            let mut store = jar.lock().map_err(|_| Error::Other("cookie jar poisoned".into()))?;
            let url = Url::parse(SITE).expect("SITE is a valid URL");
            for (name, value) in cookies {
                store
                    .parse(&format!("{}={}; Path=/", name, value), &url)
                    .map_err(|e| Error::Parse(format!("cookie {}: {}", name, e)))?;
            }
        }
        Ok(Session {
            cookies_json: dump_jar(&jar)?,
            player_id,
            login: login.to_string(),
            logged_in_at: now_unix(),
        })
    }

    /// A client carrying this session's cookies, ready for write calls.
    /// Does not verify the cookies are still accepted — use [`verify`] or
    /// just attempt the write and handle [`Error::SessionExpired`].
    pub fn client(&self) -> Result<Client> {
        self.client_with_user_agent(crate::USER_AGENT)
    }

    pub fn client_with_user_agent(&self, user_agent: &str) -> Result<Client> {
        build_client(jar_from_json(&self.cookies_json)?, user_agent)
    }

    /// The `Cookie:` header value a browser would send to the site with
    /// these cookies — for handing the session to other tools.
    pub fn cookie_header(&self) -> Result<String> {
        let store = store_from_json(&self.cookies_json)?;
        let url = Url::parse(SITE).expect("SITE is a valid URL");
        let mut parts: Vec<String> = store
            .get_request_values(&url)
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        parts.sort();
        Ok(parts.join("; "))
    }
}

fn build_client(jar: Arc<CookieStoreMutex>, user_agent: &str) -> Result<Client> {
    Ok(Client::builder()
        .cookie_provider(jar)
        .user_agent(user_agent)
        .build()?)
}

// `load_json` / `save_json` are deprecated in favour of
// `cookie_store::serde::json`, but they define the format sessions were
// persisted in before this crate existed. Keep them so stored sessions
// stay readable.
#[allow(deprecated)]
fn store_from_json(json: &str) -> Result<CookieStore> {
    CookieStore::load_json(json.as_bytes())
        .map_err(|e| Error::Parse(format!("stored cookies: {}", e)))
}

fn jar_from_json(json: &str) -> Result<Arc<CookieStoreMutex>> {
    Ok(Arc::new(CookieStoreMutex::new(store_from_json(json)?)))
}

#[allow(deprecated)]
fn dump_jar(jar: &Arc<CookieStoreMutex>) -> Result<String> {
    let store = jar.lock().map_err(|_| Error::Other("cookie jar poisoned".into()))?;
    let mut buf: Vec<u8> = Vec::new();
    // `PHPSESSID` is a session cookie (no expiry); plain `save_json` would
    // drop it and leave only `REMEMBERME`, which works (the site re-creates
    // the session from it) but costs an extra round trip and depends on the
    // remember-me path staying enabled. Keep both.
    store
        .save_incl_expired_and_nonpersistent_json(&mut buf)
        .map_err(|e| Error::Other(format!("could not serialise cookies: {}", e)))?;
    String::from_utf8(buf).map_err(|e| Error::Other(format!("cookie json not UTF-8: {}", e)))
}

/// Log in and return the resulting [`Session`].
///
/// Three requests: `GET /login` for the CSRF token, `POST /login` with the
/// credentials (`_remember_me=on`, so the site also issues a long-lived
/// `REMEMBERME` cookie), then `GET /` to confirm the logout link is there
/// and read the account's player id.
pub async fn login(login: &str, password: &str) -> Result<Session> {
    login_with_user_agent(login, password, crate::USER_AGENT).await
}

pub async fn login_with_user_agent(login: &str, password: &str, user_agent: &str) -> Result<Session> {
    let jar = Arc::new(CookieStoreMutex::new(CookieStore::default()));
    let client = build_client(jar.clone(), user_agent)?;

    let html = client
        .get(format!("{}/login", SITE))
        .send()
        .await?
        .text()
        .await?;
    let token = extract_csrf_token(&html)
        .ok_or_else(|| Error::LoginFailed("no _csrf_token on the login page".into()))?;

    let form = [
        ("_csrf_token", token.as_str()),
        ("_username", login),
        ("_password", password),
        ("_remember_me", "on"),
        ("go", "Вход"),
    ];
    client
        .post(format!("{}/login", SITE))
        .header("Origin", SITE)
        .header("Referer", format!("{}/login", SITE))
        .form(&form)
        .send()
        .await?;

    let player_id = match verify(&client).await {
        Ok(id) => id,
        Err(Error::SessionExpired) => {
            return Err(Error::LoginFailed(
                "the site did not accept the credentials (wrong password, or a captcha)".into(),
            ))
        }
        Err(e) => return Err(e),
    };

    Ok(Session {
        cookies_json: dump_jar(&jar)?,
        player_id,
        login: login.to_string(),
        logged_in_at: now_unix(),
    })
}

/// Check that `client` is logged in: `GET /` must show the logout link.
/// Returns the account's player id when the homepage exposes it.
pub async fn verify(client: &Client) -> Result<Option<i64>> {
    let home = client.get(format!("{}/", SITE)).send().await?.text().await?;
    if !home.contains("/logout") {
        return Err(Error::SessionExpired);
    }
    Ok(extract_player_id(&home))
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---- create player / team --------------------------------------------------

/// `POST /player/create`. The response carries no id — look the player up
/// through [`crate::api::search_players`] afterwards.
pub async fn create_player(client: &Client, surname: &str, name: &str, patronymic: &str) -> Result<()> {
    let body = post_ajax(
        client,
        "/player/create",
        &[("surname", surname), ("name", name), ("patronymic", patronymic)],
    )
    .await?;
    expect_success_json("create_player", &body)
}

/// `POST /teams/create`. `town_id` is the numeric id from
/// [`crate::api::lookup_town_id`]. No id in the response — search for the
/// team afterwards.
pub async fn create_team(client: &Client, name: &str, town_id: i64) -> Result<()> {
    let town = town_id.to_string();
    let body = post_ajax(client, "/teams/create", &[("name", name), ("new-team-town", &town)]).await?;
    expect_success_json("create_team", &body)
}

async fn post_ajax(client: &Client, path: &str, form: &[(&str, &str)]) -> Result<String> {
    let resp = client
        .post(format!("{}{}", SITE, path))
        .header("Origin", SITE)
        .header("Referer", format!("{}/", SITE))
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Accept", "*/*")
        .form(form)
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        if is_login_page(&body) {
            return Err(Error::SessionExpired);
        }
        return Err(Error::Status {
            status: status.as_u16(),
            summary: summarize_response(&body),
        });
    }
    Ok(body)
}

/// The AJAX endpoints answer `{"success":true}` on success; a stale
/// session gets the login page with HTTP 200, which must not pass.
fn expect_success_json(what: &str, body: &str) -> Result<()> {
    if body.contains("\"success\":true") {
        return Ok(());
    }
    if is_login_page(body) {
        return Err(Error::SessionExpired);
    }
    Err(Error::Site(format!("{}: {}", what, summarize_response(body))))
}

// ---- roster upload ---------------------------------------------------------

/// What [`upload_rosters`] did beyond the plain import.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RosterUploadReport {
    /// Team names that had their quotes removed before upload; see
    /// [`sanitize_roster_rows`].
    pub sanitized: Vec<SanitizedName>,
    /// Teams whose name differed from the registered one and were given
    /// the row's name as a one-off name for this tournament.
    pub renamed_on_tournament: Vec<(i64, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedName {
    pub team_id: i64,
    pub from: String,
    pub to: String,
}

/// Upload rosters for a tournament (the venue representative's or an
/// organiser's account must be logged in).
///
/// Two steps, as on the site: the CSV goes to the tournament page's import
/// form; if the site answers with its "Ошибки команд" form because some
/// row's team name differs from the registered one, that form is submitted
/// back choosing "внести разовое название" (one-off name) for each such
/// team. Rows are passed through [`sanitize_roster_rows`] first.
///
/// Errors: [`Error::SessionExpired`]; [`Error::Site`] when the site reports
/// a problem *or* silently ignores the file (it does that for a quoted
/// name that mismatches — see `docs/har_notes.md` §4.1).
pub async fn upload_rosters(
    client: &Client,
    tournament_id: i64,
    rows: &[RosterRow],
) -> Result<RosterUploadReport> {
    let (rows, sanitized) = sanitize_roster_rows(client, rows).await;
    if rows.is_empty() {
        return Err(Error::Site("no roster rows to upload".into()));
    }
    let csv = build_rosters_csv(&rows);
    tracing::info!(
        "upload_rosters: tournament {} — {} row(s), {} byte(s), {} name(s) sanitized",
        tournament_id,
        rows.len(),
        csv.len(),
        sanitized.len()
    );

    let part = reqwest::multipart::Part::bytes(csv.into_bytes())
        .file_name("rosters.csv")
        .mime_str("text/csv")
        .map_err(|e| Error::Other(e.to_string()))?;
    let form = reqwest::multipart::Form::new()
        .text("add_with_request_id", "")
        .text("import_teams", "Импортировать")
        .part("file", part);

    let resp = client
        .post(format!("{}/tournaments.php?displaytournament={}", SITE, tournament_id))
        .header("Origin", SITE)
        .header("Referer", format!("{}/tournament/{}", SITE, tournament_id))
        .multipart(form)
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() && !status.is_redirection() {
        return Err(Error::Status {
            status: status.as_u16(),
            summary: summarize_response(&body),
        });
    }

    let mut report = RosterUploadReport {
        sanitized,
        renamed_on_tournament: Vec::new(),
    };
    let fix = match classify_import_response(&body) {
        ImportOutcome::Success => return Ok(report),
        ImportOutcome::LoginPage => return Err(Error::SessionExpired),
        ImportOutcome::Ignored => {
            tracing::warn!(
                "upload_rosters: tournament {} — site returned neither confirmation nor fix form",
                tournament_id
            );
            return Err(Error::Site(
                "the site silently ignored the roster file (no confirmation, no error); \
                 check team and player names for unusual characters"
                    .into(),
            ));
        }
        ImportOutcome::FixForm(fix) => fix,
    };

    tracing::info!(
        "upload_rosters: tournament {} — fix form for {} team(s), idimport={}, request_id={:?}",
        tournament_id,
        fix.teams.len(),
        fix.idimport,
        fix.request_id
    );
    let mut pairs: Vec<(String, String)> = vec![
        ("idimport".into(), fix.idimport),
        ("fix_in_import".into(), "true".into()),
        ("add_with_request_id".into(), fix.request_id),
    ];
    for t in &fix.teams {
        pairs.push((format!("{}_idteam", t.key), t.idteam.clone()));
        pairs.push((format!("{}_name", t.key), t.name.clone()));
        pairs.push((format!("{}_town", t.key), t.town.clone()));
        pairs.push((format!("{}_action", t.key), "change_name_on_tournament".into()));
        if let Ok(id) = t.idteam.parse::<i64>() {
            report.renamed_on_tournament.push((id, t.name.clone()));
        }
    }
    let resp = client
        .post(format!("{}/tournament/{}", SITE, tournament_id))
        .header("Origin", SITE)
        .header(
            "Referer",
            format!("{}/tournaments.php?displaytournament={}", SITE, tournament_id),
        )
        .form(&pairs)
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() && !status.is_redirection() {
        return Err(Error::Status {
            status: status.as_u16(),
            summary: summarize_response(&body),
        });
    }
    if is_login_page(&body) {
        return Err(Error::SessionExpired);
    }
    Ok(report)
}

/// Make roster rows safe for the site's importer.
///
/// The importer matches a row's team by id and name; a name that differs
/// from the registered one goes to the fix form — unless it contains a `"`
/// character, in which case the site drops the whole file silently (in
/// CSV and XLSX alike; a quoted name that *matches* imports fine). So for
/// each team whose name contains `"`, the registered name is looked up and,
/// if it differs, the quotes are removed from that team's rows. A failed
/// lookup counts as "differs" — losing quotes beats losing the upload.
pub async fn sanitize_roster_rows(
    client: &Client,
    rows: &[RosterRow],
) -> (Vec<RosterRow>, Vec<SanitizedName>) {
    let mut replacements: BTreeMap<i64, String> = BTreeMap::new();
    let mut sanitized = Vec::new();
    for r in rows {
        if r.team_id <= 0 || !r.team_name.contains('"') || replacements.contains_key(&r.team_id) {
            continue;
        }
        let registered = crate::api::get_team(client, r.team_id).await.map(|t| t.name);
        let same = matches!(&registered, Ok(n) if n.trim() == r.team_name.trim());
        if same {
            continue;
        }
        let to = r.team_name.replace('"', "");
        tracing::info!(
            "sanitize_roster_rows: team {} name {:?} has quotes and differs from registered {:?}; using {:?}",
            r.team_id,
            r.team_name,
            registered.as_deref().unwrap_or("<lookup failed>"),
            to
        );
        sanitized.push(SanitizedName {
            team_id: r.team_id,
            from: r.team_name.clone(),
            to: to.clone(),
        });
        replacements.insert(r.team_id, to);
    }
    let out = rows
        .iter()
        .map(|r| match replacements.get(&r.team_id) {
            Some(name) => RosterRow {
                team_name: name.clone(),
                ..r.clone()
            },
            None => r.clone(),
        })
        .collect();
    (out, sanitized)
}

// ---- results upload --------------------------------------------------------

/// Upload results (per-team, per-round marks) via `POST /result/submit`.
pub async fn upload_results(client: &Client, tournament_id: i64, rows: &[ResultsRoundRow]) -> Result<()> {
    if rows.is_empty() {
        return Err(Error::Site("no result rows to upload".into()));
    }
    let csv = build_results_csv(rows);
    let part = reqwest::multipart::Part::bytes(csv.into_bytes())
        .file_name("results.csv")
        .mime_str("text/csv")
        .map_err(|e| Error::Other(e.to_string()))?;
    let form = reqwest::multipart::Form::new()
        .text("add_with_request_id", "")
        .text("tournament_id", tournament_id.to_string())
        .part("file", part);
    let resp = client
        .post(format!("{}/result/submit", SITE))
        .header("Origin", SITE)
        .header("Referer", format!("{}/tournament/{}", SITE, tournament_id))
        .multipart(form)
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() && !status.is_redirection() {
        return Err(Error::Status {
            status: status.as_u16(),
            summary: summarize_response(&body),
        });
    }
    if is_login_page(&body) {
        return Err(Error::SessionExpired);
    }
    Ok(())
}

// ---- response classification / scraping -----------------------------------

/// The three things the tournament page can come back as after an import
/// POST (always HTTP 200), plus "not logged in".
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ImportOutcome {
    /// `Импорт завершился успешно, ошибок не найдено!`
    Success,
    /// The "Ошибки команд" form: rows whose team name did not match.
    FixForm(FixForm),
    /// The site's login page: the session is gone.
    LoginPage,
    /// Plain tournament page — the importer discarded the file silently.
    Ignored,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FixForm {
    pub idimport: String,
    /// Hidden `add_with_request_id` of the fix form (the venue request the
    /// import is attached to); empty if absent.
    pub request_id: String,
    pub teams: Vec<FixTeam>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FixTeam {
    /// `team_<hex>` field-name prefix.
    pub key: String,
    pub name: String,
    pub town: String,
    pub idteam: String,
}

pub(crate) fn classify_import_response(body: &str) -> ImportOutcome {
    if is_login_page(body) {
        return ImportOutcome::LoginPage;
    }
    if let Some(idimport) = scrape_idimport(body) {
        return ImportOutcome::FixForm(FixForm {
            idimport,
            request_id: scrape_add_with_request_id(body).unwrap_or_default(),
            teams: scrape_fix_teams(body),
        });
    }
    if body.contains("Импорт завершился успешно") {
        return ImportOutcome::Success;
    }
    ImportOutcome::Ignored
}

/// The site's login form: a `_csrf_token` plus `_username` input.
pub(crate) fn is_login_page(body: &str) -> bool {
    body.contains("name=\"_csrf_token\"") && body.contains("name=\"_username\"")
}

fn extract_csrf_token(html: &str) -> Option<String> {
    let i = html.find("name=\"_csrf_token\"")?;
    attr_value_after(&html[i..], "value=\"")
}

/// `<span id="rt_user_idplayer">12345</span>`
fn extract_player_id(html: &str) -> Option<i64> {
    let i = html.find("id=\"rt_user_idplayer\"")?;
    let after = &html[i..];
    let gt = after.find('>')?;
    let rest = &after[gt + 1..];
    let end = rest.find('<')?;
    rest[..end].trim().parse().ok()
}

fn scrape_idimport(html: &str) -> Option<String> {
    let i = html.find("name=\"idimport\"")?;
    attr_value_after(&html[i..], "value=\"")
}

/// First non-empty `add_with_request_id` value. The plain upload form has
/// the input without a value; the fix form fills it in.
fn scrape_add_with_request_id(html: &str) -> Option<String> {
    let needle = "name=\"add_with_request_id\"";
    let mut cursor = 0usize;
    while let Some(i) = html[cursor..].find(needle) {
        let abs = cursor + i + needle.len();
        let tail = &html[abs..];
        let tag_end = tail.find('>')?;
        if let Some(v) = attr_value_after(&tail[..tag_end], "value=\"") {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
        cursor = abs;
    }
    None
}

/// One entry per `team_<hex>` group of `_idteam` / `_name` / `_town`
/// inputs in the fix form.
fn scrape_fix_teams(html: &str) -> Vec<FixTeam> {
    let mut map: BTreeMap<String, FixTeam> = BTreeMap::new();
    let mut cursor = 0usize;
    while let Some(i) = html[cursor..].find("name=\"team_") {
        let abs = cursor + i + "name=\"".len();
        let after_quote = &html[abs..];
        let Some(end) = after_quote.find('"') else { break };
        let full_name = &after_quote[..end];
        cursor = abs + end;
        let Some(suffix_start) = full_name.rfind('_') else { continue };
        let prefix = &full_name[..suffix_start];
        let suffix = &full_name[suffix_start + 1..];
        // Only `team_<hex>_<field>`; the page also has unrelated inputs such
        // as `team_actions`, which would otherwise become a phantom team.
        match prefix.strip_prefix("team_") {
            Some(hex) if !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) => {}
            _ => continue,
        }
        let Some(val) = attr_value_after(&html[cursor..], "value=\"") else { continue };
        let entry = map.entry(prefix.to_string()).or_insert_with(|| FixTeam {
            key: prefix.to_string(),
            name: String::new(),
            town: String::new(),
            idteam: String::new(),
        });
        match suffix {
            "name" => entry.name = val,
            "town" => entry.town = val,
            "idteam" => entry.idteam = val,
            _ => {}
        }
    }
    map.into_values().filter(|t| !t.idteam.is_empty()).collect()
}

fn attr_value_after(s: &str, marker: &str) -> Option<String> {
    let v = s.find(marker)?;
    let after = &s[v + marker.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// A short human-readable extract of a response body: a JSON `error` /
/// `message`, or the visible text of an HTML page, capped at 300 chars.
fn summarize_response(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "(empty response body)".into();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        for key in ["error", "message"] {
            if let Some(m) = v.get(key).and_then(|x| x.as_str()) {
                return truncate(m, 300);
            }
        }
    }
    // Tags become spaces so `</p><br/>x` does not glue words together.
    let mut out = String::new();
    let mut in_tag = false;
    for ch in trimmed.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&collapsed, 300)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGIN_PAGE: &str = r#"<form action="/login" method="post">
        <input type="hidden" name="_csrf_token" value="07cbb476b9ffcdfc.abc">
        <input type="text" name="_username"> <input type="password" name="_password">
        </form>"#;

    // Trimmed from a live tournament page after a mismatching import
    // (T14015, 2026-09-02).
    const FIX_FORM: &str = r#"
        <form action="/tournaments.php?displaytournament=14015" enctype="multipart/form-data" method="post">
          <input type="hidden" name="add_with_request_id"/>
          <input type="file" name="file"/>
          <input type="submit" name="import_teams" value="Импортировать"/>
        </form>
        <form action="/tournament/14015" method="post">
          <input type="hidden" name="idimport" value="196018"/>
          <input type="hidden" name="fix_in_import" value="true"/>
          <input type="hidden" name="add_with_request_id" value="189504">
          <button id="show_teams_import_errors" type="button">Ошибки команд</button>
          <tr class="import-error-table-row">
            <td>Строка: 1 107740 Ладно, погнали вчетвером сегодня (Краков)
                имя в базе: 107740 &quot;Ладно, погнали втроём сегодня&quot; (Краков)
              <label>ID: <input type="text" name="team_000000000000081b0000000000000000_idteam" value="107740" /></label>
              <label>Название: <input type="text" name="team_000000000000081b0000000000000000_name" value="Ладно, погнали вчетвером сегодня" /></label>
              <label>Город: <input type="text" name="team_000000000000081b0000000000000000_town" value="Краков" /></label>
            </td>
            <td>
              <input type="radio" name="team_000000000000081b0000000000000000_action"
                value="107740" /> 107740 &quot;Ладно, погнали втроём сегодня&quot;
              <label><input type="radio" name="team_000000000000081b0000000000000000_action" value="create" /> создать</label>
              <label><input type="radio" name="team_000000000000081b0000000000000000_action" value="change_name_on_tournament" /> внести разовое название</label>
              <label><input type="radio" name="team_000000000000081b0000000000000000_action" value="noop" /> ничего не делать</label>
            </td>
          </tr>
          <input type="submit" value="Сохранить" />
        </form>
        <form action="/tournaments.php" method="post" id="action_form">
          <input type="hidden" name="tournament_id" value="14015">
          <input type="hidden" name="team_actions">
          <input type="hidden" name="action">
        </form>"#;

    #[test]
    fn classifies_success_fixform_login_and_ignored() {
        assert_eq!(
            classify_import_response("<div>Импорт завершился успешно, ошибок не найдено!</div>"),
            ImportOutcome::Success
        );
        assert_eq!(classify_import_response(LOGIN_PAGE), ImportOutcome::LoginPage);
        assert_eq!(
            classify_import_response("<html><body>Чудове Чудовисько</body></html>"),
            ImportOutcome::Ignored
        );
        match classify_import_response(FIX_FORM) {
            ImportOutcome::FixForm(f) => {
                assert_eq!(f.idimport, "196018");
                assert_eq!(f.request_id, "189504");
                assert_eq!(
                    f.teams,
                    vec![FixTeam {
                        key: "team_000000000000081b0000000000000000".into(),
                        name: "Ладно, погнали вчетвером сегодня".into(),
                        town: "Краков".into(),
                        idteam: "107740".into(),
                    }]
                );
            }
            other => panic!("expected fix form, got {:?}", other),
        }
    }

    #[test]
    fn scrapers() {
        assert_eq!(extract_csrf_token(LOGIN_PAGE).as_deref(), Some("07cbb476b9ffcdfc.abc"));
        assert_eq!(
            extract_player_id(r#"<span class="no-display" id="rt_user_idplayer">127696</span>"#),
            Some(127696)
        );
        assert_eq!(extract_player_id("<p>no marker</p>"), None);
        // A valueless input before the real one must not stop the search.
        assert_eq!(scrape_add_with_request_id(FIX_FORM).as_deref(), Some("189504"));
        assert_eq!(
            scrape_add_with_request_id(r#"<input type="hidden" name="add_with_request_id"/>"#),
            None
        );
    }

    #[test]
    fn success_json_and_stale_session() {
        assert!(expect_success_json("x", r#"{"success":true}"#).is_ok());
        assert!(matches!(
            expect_success_json("x", LOGIN_PAGE),
            Err(Error::SessionExpired)
        ));
        match expect_success_json("create_team", r#"{"error":"Такая команда уже есть"}"#) {
            Err(Error::Site(msg)) => assert_eq!(msg, "create_team: Такая команда уже есть"),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn summarize_strips_html_and_truncates() {
        assert_eq!(summarize_response("  "), "(empty response body)");
        assert_eq!(summarize_response("<p>Ошибка   импорта</p><br/>x"), "Ошибка импорта x");
        let long = "я".repeat(400);
        assert_eq!(summarize_response(&long).chars().count(), 301);
    }

    #[test]
    fn session_round_trips_and_exposes_cookie_header() {
        let jar = Arc::new(CookieStoreMutex::new(CookieStore::default()));
        {
            let mut store = jar.lock().unwrap();
            let url = Url::parse(SITE).unwrap();
            store
                .parse("PHPSESSID=abc123; Path=/", &url)
                .unwrap();
            store
                .parse("REMEMBERME=tok; Path=/; Max-Age=31536000", &url)
                .unwrap();
        }
        let session = Session {
            cookies_json: dump_jar(&jar).unwrap(),
            player_id: Some(127696),
            login: "someone@example.com".into(),
            logged_in_at: 1_700_000_000,
        };
        session.client().expect("client from session");
        assert_eq!(session.cookie_header().unwrap(), "PHPSESSID=abc123; REMEMBERME=tok");
        // Serde round trip keeps the stable field names.
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"cookies_json\"") && json.contains("\"logged_in_at\""));
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.player_id, Some(127696));

        // from_cookies is the inverse of cookie_header.
        let imported =
            Session::from_cookies(&[("REMEMBERME", "tok"), ("PHPSESSID", "abc123")], "x", None).unwrap();
        assert_eq!(imported.cookie_header().unwrap(), "PHPSESSID=abc123; REMEMBERME=tok");
    }
}
