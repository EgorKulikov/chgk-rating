//! Write-side client for `https://rating.chgk.info` — the admin website,
//! using its cookie-authenticated JSON endpoints for rosters and results,
//! and forms for login and player/team creation. See `docs/submission_api.md`
//! for submissions and `docs/har_notes.md` for the remaining form flows.
//!
//! Flow: [`login`] once to obtain a [`Session`] (the cookies the site
//! issued — never the password), persist it, and build a [`SiteClient`]
//! from it with [`Session::client`] for each write. When the site stops
//! accepting the cookies, writes fail with [`Error::SessionExpired`]; log
//! in again and retry.
//!
//! A serialised `Session` is a bearer credential: the `REMEMBERME` cookie
//! stays valid for about a year. Store it with owner-only permissions and
//! keep it out of logs (its `Debug` output redacts the cookies).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::{Client, Url};
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Deserialize, Serialize};

use crate::api::ApiClient;
use crate::csv::RosterRow;

mod submission;
use crate::http::read_body;
use crate::text::{collapse_whitespace, summarize_response};
use crate::{Error, Result};

/// Base URL of the admin website.
pub const SITE_BASE: &str = "https://rating.chgk.info";

// ---- sessions --------------------------------------------------------------

/// A logged-in rating.chgk.info session: the site's cookies (`PHPSESSID`,
/// `REMEMBERME`) serialised as JSON, plus who they belong to.
///
/// Serialisable; field names are stable so callers can persist it. Treat
/// the serialised form as a password-equivalent secret.
#[derive(Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Session {
    /// `CookieStore::save_json` output.
    pub cookies_json: String,
    /// The account's own player id, scraped from the homepage after login.
    #[serde(default)]
    pub player_id: Option<i64>,
    /// The login name used (an e-mail address), for display.
    #[serde(default)]
    pub login: String,
    /// Unix timestamp of the login; `0` for sessions persisted before this
    /// field existed.
    #[serde(default)]
    pub logged_in_at: i64,
    /// The site the cookies belong to: [`SITE_BASE`], or the mirror / test
    /// server given to [`login_with`]. Sessions persisted before this
    /// field existed load as [`SITE_BASE`].
    #[serde(default = "default_base_url")]
    pub base_url: String,
}

fn default_base_url() -> String {
    SITE_BASE.to_string()
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("cookies_json", &"<redacted>")
            .field("player_id", &self.player_id)
            .field("login", &self.login)
            .field("logged_in_at", &self.logged_in_at)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl Session {
    /// Build a session from raw cookie name/value pairs (e.g. copied from a
    /// browser or produced by [`Session::cookie_header`]). The inverse of
    /// `cookie_header`; `player_id` is whatever the caller knows.
    ///
    /// # Errors
    /// [`Error::Config`] if a cookie name or value is not valid cookie
    /// syntax.
    pub fn from_cookies(
        cookies: &[(&str, &str)],
        login: &str,
        player_id: Option<i64>,
    ) -> Result<Session> {
        let jar = Arc::new(CookieStoreMutex::new(CookieStore::default()));
        {
            let mut store = jar.lock().expect("cookie jar mutex poisoned");
            let url = Url::parse(SITE_BASE).expect("SITE_BASE is a valid URL");
            for (name, value) in cookies {
                validate_cookie(name, value)?;
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
            base_url: SITE_BASE.to_string(),
        })
    }

    fn base(&self) -> Result<Url> {
        Url::parse(&self.base_url)
            .map_err(|e| Error::Config(format!("session base url {:?}: {}", self.base_url, e)))
    }

    /// A client carrying this session's cookies, ready for write calls,/// with the crate's [`USER_AGENT`](crate::USER_AGENT) and default
    /// timeouts. Does not verify the cookies are still accepted — use
    /// [`SiteClient::verify`] or just attempt the write and handle
    /// [`Error::SessionExpired`].
    ///
    /// # Errors
    /// [`Error::Parse`] if the stored cookies are unreadable or contain
    /// characters that cannot go into a `Cookie` header, [`Error::Config`]
    /// if `base_url` does not parse, [`Error::Http`] if the TLS backend
    /// cannot be initialised.
    pub fn client(&self) -> Result<SiteClient> {
        self.client_with(crate::USER_AGENT)
    }

    /// [`Session::client`] with a custom `User-Agent`.
    ///
    /// # Errors
    /// As [`Session::client`].
    pub fn client_with(&self, user_agent: &str) -> Result<SiteClient> {
        let base = self.base()?;
        let store = self.validated_store(&base)?;
        let jar = Arc::new(CookieStoreMutex::new(store));
        let http = build_client(jar, user_agent, &base)?;
        Ok(SiteClient { http, base })
    }

    /// [`Session::client`] with a custom `User-Agent`.
    ///
    /// # Errors
    /// As [`Session::client`].
    #[deprecated(since = "0.2.0", note = "renamed to `client_with`")]
    pub fn client_with_user_agent(&self, user_agent: &str) -> Result<SiteClient> {
        self.client_with(user_agent)
    }

    /// The `Cookie:` header value a browser would send to the site with
    /// these cookies — for handing the session to other tools.
    ///
    /// # Errors
    /// [`Error::Parse`] if the stored cookies are unreadable or contain
    /// characters that cannot go into a `Cookie` header.
    pub fn cookie_header(&self) -> Result<String> {
        let base = self.base()?;
        let store = self.validated_store(&base)?;
        let mut parts: Vec<String> = store
            .get_request_values(&base)
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        parts.sort();
        Ok(parts.join("; "))
    }

    /// The persisted store, after checking that every cookie it would send
    /// to `base` is still valid header material (the file may have been
    /// edited by hand).
    fn validated_store(&self, base: &Url) -> Result<CookieStore> {
        let store = store_from_json(&self.cookies_json)?;
        for (name, value) in store.get_request_values(base) {
            validate_cookie(name, value).map_err(|_| {
                Error::Parse(format!("stored cookie {:?} has an invalid value", name))
            })?;
        }
        Ok(store)
    }
}

/// RFC 6265 syntax: a non-empty `token` name and a `cookie-octet` value —
/// no control characters, whitespace, separators or quotes that could
/// smuggle attributes or header lines.
fn validate_cookie(name: &str, value: &str) -> Result<()> {
    const SEPARATORS: &str = "()<>@,;:\\\"/[]?={} \t";
    let name_ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii() && !c.is_ascii_control() && !SEPARATORS.contains(c));
    let value_ok = value.chars().all(|c| {
        c.is_ascii() && !c.is_ascii_control() && !c.is_ascii_whitespace() && !"\",;\\".contains(c)
    });
    if !name_ok || !value_ok {
        return Err(Error::Config(format!(
            "cookie {:?}: invalid name or value",
            name
        )));
    }
    Ok(())
}

/// A cookie-carrying client for `base`. HTTPS-only unless `base` itself
/// is plain `http` (a local test server); redirects are followed only
/// within `base`'s origin, so a cross-site redirect can never replay the
/// login form or carry the cookies elsewhere.
fn build_client(jar: Arc<CookieStoreMutex>, user_agent: &str, base: &Url) -> Result<Client> {
    let origin = base.origin();
    let redirects = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 10 {
            attempt.error("too many redirects")
        } else if attempt.url().origin() == origin {
            attempt.follow()
        } else {
            attempt.stop()
        }
    });
    Ok(Client::builder()
        .cookie_provider(jar)
        .user_agent(user_agent)
        .timeout(crate::REQUEST_TIMEOUT)
        .connect_timeout(crate::CONNECT_TIMEOUT)
        .https_only(base.scheme() == "https")
        .redirect(redirects)
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

#[allow(deprecated)]
fn dump_jar(jar: &Arc<CookieStoreMutex>) -> Result<String> {
    let store = jar.lock().expect("cookie jar mutex poisoned");
    let mut buf: Vec<u8> = Vec::new();
    // `PHPSESSID` is a session cookie (no expiry); plain `save_json` would
    // drop it and leave only `REMEMBERME`, which works (the site re-creates
    // the session from it) but costs an extra round trip and depends on the
    // remember-me path staying enabled. Keep both.
    store
        .save_incl_expired_and_nonpersistent_json(&mut buf)
        .map_err(|e| Error::Parse(format!("could not serialise cookies: {}", e)))?;
    String::from_utf8(buf).map_err(|e| Error::Parse(format!("cookie json not UTF-8: {}", e)))
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---- login -----------------------------------------------------------------

/// Knobs for [`login_with`]. Build with [`LoginOptions::new`] and the
/// chained setters (the struct is `#[non_exhaustive]`).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoginOptions {
    /// `User-Agent` for the login requests and the resulting session.
    pub user_agent: String,
    /// Site base URL; [`SITE_BASE`] unless talking to a mirror or a test
    /// server. An `http://` base sends the password in clear — only ever
    /// use one for a local test server.
    pub base_url: String,
    /// Ask for the long-lived `REMEMBERME` cookie (about a year). Without
    /// it the session lasts only as long as the site's `PHPSESSID`.
    pub remember_me: bool,
}

impl Default for LoginOptions {
    fn default() -> Self {
        LoginOptions {
            user_agent: crate::USER_AGENT.to_string(),
            base_url: SITE_BASE.to_string(),
            remember_me: true,
        }
    }
}

impl LoginOptions {
    /// The defaults: the crate's user agent, the real site, remember-me on.
    pub fn new() -> LoginOptions {
        LoginOptions::default()
    }

    /// `User-Agent` for the login requests and the resulting session.
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// Site base URL (see the field doc for the `http://` caveat).
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Whether to ask for the long-lived `REMEMBERME` cookie.
    pub fn remember_me(mut self, remember_me: bool) -> Self {
        self.remember_me = remember_me;
        self
    }
}

/// Log in and return the resulting [`Session`].
///
/// Three requests: `GET /login` for the CSRF token, `POST /login` with the
/// credentials (`_remember_me=on`, so the site also issues a long-lived
/// `REMEMBERME` cookie), then `GET /` to confirm the logout link is there
/// and read the account's player id.
///
/// # Errors
/// [`Error::LoginFailed`] when the site did not accept the credentials or
/// its login form changed; [`Error::Http`] on transport failures.
pub async fn login(login: &str, password: &str) -> Result<Session> {
    login_with(login, password, &LoginOptions::default()).await
}

/// [`login`] with a custom `User-Agent`.
///
/// # Errors
/// As [`login`].
#[deprecated(
    since = "0.2.0",
    note = "use `login_with(login, password, &LoginOptions::new().user_agent(ua))`"
)]
pub async fn login_with_user_agent(
    login: &str,
    password: &str,
    user_agent: &str,
) -> Result<Session> {
    login_with(login, password, &LoginOptions::new().user_agent(user_agent)).await
}

/// [`login`] with explicit [`LoginOptions`].
///
/// # Errors
/// As [`login`]; [`Error::Status`] when the site answers any of the three
/// requests with an error status (an outage is not a wrong password);
/// [`Error::Config`] if `opts.base_url` is not an absolute URL.
pub async fn login_with(login: &str, password: &str, opts: &LoginOptions) -> Result<Session> {
    let base = Url::parse(&opts.base_url)
        .map_err(|e| Error::Config(format!("base url {:?}: {}", opts.base_url, e)))?;
    let jar = Arc::new(CookieStoreMutex::new(CookieStore::default()));
    let http = build_client(jar.clone(), &opts.user_agent, &base)?;
    let site = SiteClient { http, base };

    let resp = site.http.get(site.url("/login")).send().await?;
    let (status, html) = read_body(resp).await?;
    if !status.is_success() {
        return Err(Error::Status {
            status: status.as_u16(),
            url: "/login".into(),
            summary: summarize_response(&html),
        });
    }
    let token = extract_csrf_token(&html)
        .ok_or_else(|| Error::LoginFailed("no _csrf_token on the login page".into()))?;

    let mut form = vec![
        ("_csrf_token", token.as_str()),
        ("_username", login),
        ("_password", password),
    ];
    if opts.remember_me {
        form.push(("_remember_me", "on"));
    }
    form.push(("go", "Вход"));
    let resp = site
        .http
        .post(site.url("/login"))
        .header("Origin", site.origin())
        .header("Referer", site.url("/login"))
        .form(&form)
        .send()
        .await?;
    let (status, posted) = read_body(resp).await?;
    if !status.is_success() && !status.is_redirection() && !is_login_page(&posted) {
        return Err(Error::Status {
            status: status.as_u16(),
            url: "/login".into(),
            summary: summarize_response(&posted),
        });
    }

    let player_id = match site.verify().await {
        Ok(id) => id,
        Err(Error::SessionExpired) => {
            let reason = extract_login_error(&posted).unwrap_or_else(|| {
                "the site did not accept the credentials (wrong password, or a captcha)".into()
            });
            return Err(Error::LoginFailed(reason));
        }
        Err(e) => return Err(e),
    };

    Ok(Session {
        cookies_json: dump_jar(&jar)?,
        player_id,
        login: login.to_string(),
        logged_in_at: now_unix(),
        base_url: opts.base_url.clone(),
    })
}

// ---- client ----------------------------------------------------------------

/// A client for the admin website, carrying a [`Session`]'s cookies.
/// Obtained from [`Session::client`].
#[derive(Debug, Clone)]
pub struct SiteClient {
    http: Client,
    base: Url,
}

impl SiteClient {
    /// Wrap a caller-built `reqwest::Client` (which must carry the session
    /// cookies itself) against an arbitrary base URL — for mirrors and
    /// local test servers.
    ///
    /// # Errors
    /// [`Error::Config`] if `base` is not an absolute URL.
    pub fn with_base_url(http: Client, base: &str) -> Result<SiteClient> {
        let base =
            Url::parse(base).map_err(|e| Error::Config(format!("base url {:?}: {}", base, e)))?;
        Ok(SiteClient { http, base })
    }

    /// The underlying `reqwest::Client`.
    pub fn http(&self) -> &Client {
        &self.http
    }

    fn origin(&self) -> String {
        self.base.as_str().trim_end_matches('/').to_string()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.origin(), path)
    }

    /// Check that the session is logged in: `GET /` must show the logout
    /// link. Returns the account's player id when the homepage exposes it.
    ///
    /// # Errors
    /// [`Error::SessionExpired`] when the homepage is the login page or has
    /// no logout link; [`Error::Status`] when the site answers with an
    /// error status (an outage, not an expired session); [`Error::Http`].
    pub async fn verify(&self) -> Result<Option<i64>> {
        let resp = self.http.get(self.url("/")).send().await?;
        let (status, home) = read_body(resp).await?;
        if is_login_page(&home) {
            return Err(Error::SessionExpired);
        }
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                url: "/".into(),
                summary: summarize_response(&home),
            });
        }
        classify_home(&home)
    }

    // ---- create team (players: see `submission`) ---------------------------

    /// `POST /teams/create`; returns the new team's id. `town_id` is the
    /// numeric id from [`ApiClient::lookup_town_id`]. Any logged-in account
    /// may do this; no tournament is involved, and the creator is not
    /// added to the team's roster.
    ///
    /// # Errors
    /// [`Error::SessionExpired`]; [`Error::Site`] when the answer carries
    /// no `teamId` (the message is the site's own, if it gave one);
    /// [`Error::Status`], [`Error::Http`].
    pub async fn create_team(&self, name: &str, town_id: i64) -> Result<i64> {
        let town = town_id.to_string();
        let body = self
            .post_ajax("/teams/create", &[("name", name), ("new-team-town", &town)])
            .await?;
        created_team_id(&body)
    }

    async fn post_ajax(&self, path: &str, form: &[(&str, &str)]) -> Result<String> {
        let url = self.url(path);
        let resp = self
            .http
            .post(&url)
            .header("Origin", self.origin())
            .header("Referer", self.url("/"))
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "*/*")
            .form(form)
            .send()
            .await?;
        let (status, body) = read_response(resp).await?;
        if is_login_page(&body) {
            return Err(Error::SessionExpired);
        }
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                url: path.to_string(),
                summary: summarize_response(&body),
            });
        }
        Ok(body)
    }
}

/// Status and body of a response; a body that cannot be read (or is too
/// large) is an error, never an empty success.
async fn read_response(resp: reqwest::Response) -> Result<(reqwest::StatusCode, String)> {
    read_body(resp).await
}
/// `/teams/create` answers `{"teamId": <id>}`; a stale session gets the
/// login page with HTTP 200, which must not pass.
fn created_team_id(body: &str) -> Result<i64> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(id) = v
            .get("teamId")
            .and_then(|x| x.as_i64())
            .filter(|id| *id > 0)
        {
            return Ok(id);
        }
    }
    if is_login_page(body) {
        return Err(Error::SessionExpired);
    }
    Err(Error::site("create_team", summarize_response(body)))
}

// ---- roster sanitising -----------------------------------------------------

/// Details of a successful [`SiteClient::upload_rosters`] JSON save.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RosterUploadReport {
    /// Legacy CSV sanitising changes. Always empty for JSON submissions,
    /// which preserve quotes in team names.
    pub sanitized: Vec<SanitizedName>,
    /// Teams whose name differed from the registered one and were given
    /// the row's name as a one-off name for this tournament.
    pub renamed_on_tournament: Vec<RenamedTeam>,
    /// Nonblocking notices returned by the server (for example, a disqualified player).
    pub warnings: Vec<String>,
}

/// One team given a one-off name by [`SiteClient::upload_rosters`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenamedTeam {
    /// The site's numeric team id.
    pub team_id: i64,
    /// The name used for this tournament.
    pub name: String,
}

/// One team whose name was changed by [`sanitize_roster_rows`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SanitizedName {
    /// The site's numeric team id.
    pub team_id: i64,
    /// The name as given in the rows.
    pub from: String,
    /// The name actually uploaded.
    pub to: String,
}

/// Make roster rows safe for the legacy CSV importer.
/// [`SiteClient::upload_rosters`] uses JSON and does not call this helper.
///
/// The importer matches a row's team by id and name; a name that differs
/// from the registered one goes to the fix form — unless it contains a `"`
/// character, in which case the site drops the whole file silently (in
/// CSV and XLSX alike; a quoted name that *matches* imports fine). So for
/// each team whose name contains `"`, the registered name is looked up
/// through `api` and, if it differs, the quotes are removed from that
/// team's rows. See [`sanitize_roster_rows_with`] for the pure part.
///
/// # Errors
/// Whatever [`ApiClient::get_team`] returns for a team that needed a
/// lookup: nothing is guessed when the registered name is unknown.
pub async fn sanitize_roster_rows(
    api: &ApiClient,
    rows: &[RosterRow],
) -> Result<(Vec<RosterRow>, Vec<SanitizedName>)> {
    let mut registered: BTreeMap<i64, String> = BTreeMap::new();
    for id in teams_needing_lookup(rows) {
        registered.insert(id, api.get_team(id).await?.name);
    }
    Ok(sanitize_roster_rows_with(rows, |id| {
        registered.get(&id).cloned()
    }))
}

/// [`sanitize_roster_rows`] with a caller-supplied lookup of the
/// registered team name, called once per distinct team whose name contains
/// a `"`. `None` means "unknown": the row is left untouched rather than
/// renamed on a guess.
pub fn sanitize_roster_rows_with(
    rows: &[RosterRow],
    mut registered_name: impl FnMut(i64) -> Option<String>,
) -> (Vec<RosterRow>, Vec<SanitizedName>) {
    let mut replacements: BTreeMap<i64, String> = BTreeMap::new();
    let mut sanitized = Vec::new();
    for r in rows {
        if !needs_lookup(r) || replacements.contains_key(&r.team_id) {
            continue;
        }
        let Some(registered) = registered_name(r.team_id) else {
            tracing::warn!(
                "sanitize_roster_rows: team {} name {:?} has quotes but the registered name is unknown; left as is",
                r.team_id,
                r.team_name
            );
            continue;
        };
        if registered.trim() == r.team_name.trim() {
            continue;
        }
        let to = r.team_name.replace('"', "");
        tracing::info!(
            "sanitize_roster_rows: team {} name {:?} has quotes and differs from registered {:?}; using {:?}",
            r.team_id,
            r.team_name,
            registered,
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

fn needs_lookup(r: &RosterRow) -> bool {
    r.team_id > 0 && r.team_name.contains('"')
}

/// Distinct team ids whose rows carry a quoted name, in first-seen order.
fn teams_needing_lookup(rows: &[RosterRow]) -> Vec<i64> {
    let mut ids = Vec::new();
    for r in rows {
        if needs_lookup(r) && !ids.contains(&r.team_id) {
            ids.push(r.team_id);
        }
    }
    ids
}

// ---- response classification / scraping -----------------------------------

/// The logged-in homepage must carry the logout link; the login page (or
/// anything else) means the session is not accepted. Returns the player
/// id when the page exposes it.
fn classify_home(html: &str) -> Result<Option<i64>> {
    if !html.contains("/logout") {
        return Err(Error::SessionExpired);
    }
    Ok(extract_player_id(html))
}

/// Symfony's flash on a failed login: the text of the first
/// `alert-danger` block on the page (any `alert` block if there is none).
fn extract_login_error(html: &str) -> Option<String> {
    let i = html
        .find("class=\"alert-danger")
        .or_else(|| html.find("class=\"alert alert-danger"))
        .or_else(|| html.find("class=\"alert"))?;
    let rest = &html[i..];
    let start = rest.find('>')? + 1;
    let inner = &rest[start..];
    let end = inner.find("</div>").unwrap_or(inner.len());
    let text = summarize_response(&decode_entities(&inner[..end]));
    let text = collapse_whitespace(&text);
    (!text.is_empty() && text != "(empty response body)").then_some(text)
}

/// The site's login form: a `_csrf_token` plus `_username` input.
pub(crate) fn is_login_page(body: &str) -> bool {
    body.contains("name=\"_csrf_token\"") && body.contains("name=\"_username\"")
}

fn extract_csrf_token(html: &str) -> Option<String> {
    attr_value_of(html, "_csrf_token")
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

/// The `value` of the input whose `name` is `name`, entity-decoded; only
/// looked for inside that input's own tag.
fn attr_value_of(html: &str, name: &str) -> Option<String> {
    let i = html.find(&format!("name=\"{}\"", name))?;
    value_in_tag(tag_around(html, i))
}

/// The tag enclosing byte offset `at`: from the last `<` before it to the
/// first `>` after it.
fn tag_around(html: &str, at: usize) -> &str {
    let start = html[..at].rfind('<').unwrap_or(0);
    let end = html[at..].find('>').map(|e| at + e).unwrap_or(html.len());
    &html[start..end]
}

/// `value="…"` inside one tag (the attribute, not `data-value`),
/// entity-decoded, control characters dropped.
fn value_in_tag(tag: &str) -> Option<String> {
    let mut from = 0;
    let v = loop {
        let i = from + tag[from..].find("value=\"")?;
        let preceded_by_space = i == 0 || tag.as_bytes()[i - 1].is_ascii_whitespace();
        if preceded_by_space {
            break i;
        }
        from = i + 1;
    };
    let after = &tag[v + "value=\"".len()..];
    let end = after.find('"')?;
    Some(
        decode_entities(&after[..end])
            .chars()
            .filter(|c| !c.is_control())
            .collect(),
    )
}

/// Decode the entities the site emits in attribute values: the five named
/// XML ones plus numeric references. Unknown or unterminated entities are
/// left as they are.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(semi) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGIN_PAGE: &str = r#"<form action="/login" method="post">
        <input type="hidden" name="_csrf_token" value="07cbb476b9ffcdfc.abc">
        <input type="text" name="_username"> <input type="password" name="_password">
        </form>"#;

    #[test]
    fn scrapers() {
        assert_eq!(
            extract_csrf_token(LOGIN_PAGE).as_deref(),
            Some("07cbb476b9ffcdfc.abc")
        );
        assert_eq!(
            extract_player_id(r#"<span class="no-display" id="rt_user_idplayer">127696</span>"#),
            Some(127696)
        );
        assert_eq!(extract_player_id("<p>no marker</p>"), None);
    }

    #[test]
    fn attribute_values_are_scoped_to_their_own_tag() {
        // `value` before `name`.
        assert_eq!(
            attr_value_of(
                r#"<input value="v1" name="x"><input name="y" value="v2">"#,
                "x"
            )
            .as_deref(),
            Some("v1")
        );
        // Valueless input must not borrow the next tag's value.
        assert_eq!(
            attr_value_of(r#"<input name="x"><input name="y" value="v2">"#, "x"),
            None
        );
        assert_eq!(
            attr_value_of(r#"<input name="x"><input name="y" value="v2">"#, "y").as_deref(),
            Some("v2")
        );
        assert_eq!(attr_value_of("<p>no such input</p>", "x"), None);
    }

    #[test]
    fn html_entities_are_decoded() {
        assert_eq!(
            decode_entities("Q&amp;A &#39;26 &quot;bis&quot; &lt;x&gt; &#x41;&#65;"),
            "Q&A '26 \"bis\" <x> AA"
        );
        assert_eq!(decode_entities("&unknown; &amp"), "&unknown; &amp");
    }

    #[test]
    fn home_page_classification() {
        assert_eq!(
            classify_home(
                r#"<span id="rt_user_idplayer">127696</span><a href="/logout">Выход</a>"#
            )
            .unwrap(),
            Some(127696)
        );
        assert_eq!(
            classify_home(r#"<a href="/logout">Выход</a>"#).unwrap(),
            None
        );
        assert!(matches!(
            classify_home(LOGIN_PAGE),
            Err(Error::SessionExpired)
        ));
    }

    #[test]
    fn login_flash_is_extracted() {
        let page = format!(
            "<div class=\"alert alert-danger\">Неверные учётные данные.</div>{}",
            LOGIN_PAGE
        );
        assert_eq!(
            extract_login_error(&page).as_deref(),
            Some("Неверные учётные данные.")
        );
        assert_eq!(extract_login_error(LOGIN_PAGE), None);
    }

    #[test]
    fn sanitize_strips_quotes_only_when_the_name_differs_from_the_registered_one() {
        let row = |team_id: i64, name: &str| RosterRow {
            team_id,
            team_name: name.into(),
            town: "Краков".into(),
            player_id: 1,
            surname: "A".into(),
            name: "B".into(),
            patronymic: String::new(),
            flag: None,
        };
        let rows = vec![
            row(1, "\"Same\" name"),
            row(2, "\"Other\" name"),
            row(3, "\"Unknown\" team"),
            row(4, "plain name"),
            row(2, "\"Other\" name"),
        ];
        let mut looked_up = Vec::new();
        let (out, sanitized) = sanitize_roster_rows_with(&rows, |id| {
            looked_up.push(id);
            match id {
                1 => Some(" \"Same\" name ".to_string()),
                2 => Some("Other name registered".to_string()),
                _ => None,
            }
        });
        assert_eq!(
            looked_up,
            vec![1, 2, 3],
            "one lookup per quoted team, none for plain names"
        );
        let names: Vec<&str> = out.iter().map(|r| r.team_name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "\"Same\" name",
                "Other name",
                "\"Unknown\" team",
                "plain name",
                "Other name"
            ]
        );
        assert_eq!(
            sanitized,
            vec![SanitizedName {
                team_id: 2,
                from: "\"Other\" name".into(),
                to: "Other name".into()
            }]
        );
    }

    #[test]
    fn session_debug_redacts_cookies_and_tolerates_old_json() {
        let session = Session::from_cookies(&[("PHPSESSID", "secret123")], "x", None).unwrap();
        let dbg = format!("{:?}", session);
        assert!(!dbg.contains("secret123"), "{}", dbg);
        assert!(dbg.contains("<redacted>"), "{}", dbg);
        // Sessions persisted before `logged_in_at` existed still load.
        let old: Session = serde_json::from_str(&format!(
            r#"{{"cookies_json":{},"player_id":5}}"#,
            serde_json::to_string(&session.cookies_json).unwrap()
        ))
        .unwrap();
        assert_eq!(old.player_id, Some(5));
        assert_eq!(old.logged_in_at, 0);
    }

    #[test]
    fn from_cookies_rejects_separators_and_control_characters() {
        for (name, value) in [
            ("PHPSESSID", "a;Domain=.chgk.info"),
            ("PHPSESSID", "a\r\nX-Injected: 1"),
            ("PHP SESSID", "a"),
            ("PHPSESSID", "a,b"),
            ("", "a"),
            ("PHPSESSID", "a\"b"),
        ] {
            assert!(
                matches!(
                    Session::from_cookies(&[(name, value)], "x", None),
                    Err(Error::Config(_))
                ),
                "{:?}={:?} must be rejected",
                name,
                value
            );
        }
        assert!(Session::from_cookies(&[("REMEMBERME", "a1b2-C3_d4%3D")], "x", None).is_ok());
        assert!(Session::from_cookies(&[("REMEMBERME", "QUJD/ZGVm+Zz==")], "x", None).is_ok());
    }

    #[test]
    fn cookie_header_revalidates_persisted_values() {
        let session = Session::from_cookies(&[("PHPSESSID", "abc")], "x", None).unwrap();
        let mut tampered = session.clone();
        tampered.cookies_json = session
            .cookies_json
            .replace("abc", "abc\\r\\nX-Injected: 1");
        assert!(
            tampered.cookies_json.contains("X-Injected"),
            "{}",
            tampered.cookies_json
        );
        assert!(matches!(tampered.cookie_header(), Err(Error::Parse(_))));
        assert!(matches!(tampered.client(), Err(Error::Parse(_))));
    }

    #[test]
    fn attribute_values_drop_control_characters() {
        assert_eq!(
            value_in_tag(r#"<input value="a&#0;b&#x1b;c">"#).as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn malformed_entities_are_left_alone() {
        assert_eq!(
            decode_entities("&#;x &#xZZ; &#x110000; &#xD800;"),
            "&#;x &#xZZ; &#x110000; &#xD800;"
        );
    }

    #[test]
    fn created_team_id_needs_a_positive_team_id() {
        assert_eq!(created_team_id(r#"{"teamId": 110234}"#).unwrap(), 110234);
        for body in [
            r#"{"success":true}"#,
            "[]",
            r#"{"teamId":"x"}"#,
            r#"{"data":{"teamId":5}}"#,
        ] {
            assert!(
                matches!(created_team_id(body), Err(Error::Site { .. })),
                "{}",
                body
            );
        }
        assert!(matches!(
            created_team_id(LOGIN_PAGE),
            Err(Error::SessionExpired)
        ));
        match created_team_id(r#"{"message":"Неправильные аргументы"}"#) {
            Err(Error::Site { action, message }) => {
                assert_eq!(action, "create_team");
                assert_eq!(message, "Неправильные аргументы");
            }
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn session_round_trips_and_exposes_cookie_header() {
        let jar = Arc::new(CookieStoreMutex::new(CookieStore::default()));
        {
            let mut store = jar.lock().unwrap();
            let url = Url::parse(SITE_BASE).unwrap();
            store.parse("PHPSESSID=abc123; Path=/", &url).unwrap();
            store
                .parse("REMEMBERME=tok; Path=/; Max-Age=31536000", &url)
                .unwrap();
        }
        let session = Session {
            cookies_json: dump_jar(&jar).unwrap(),
            player_id: Some(127696),
            login: "someone@example.com".into(),
            logged_in_at: 1_700_000_000,
            base_url: SITE_BASE.into(),
        };
        session.client().expect("client from session");
        assert_eq!(
            session.cookie_header().unwrap(),
            "PHPSESSID=abc123; REMEMBERME=tok"
        );
        // Serde round trip keeps the stable field names.
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"cookies_json\"") && json.contains("\"logged_in_at\""));
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.player_id, Some(127696));

        // from_cookies is the inverse of cookie_header.
        let imported =
            Session::from_cookies(&[("REMEMBERME", "tok"), ("PHPSESSID", "abc123")], "x", None)
                .unwrap();
        assert_eq!(
            imported.cookie_header().unwrap(),
            "PHPSESSID=abc123; REMEMBERME=tok"
        );
    }
}
