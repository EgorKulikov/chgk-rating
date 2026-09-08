//! `SiteClient` write flows against a local server.

mod common;

use chgk_rating::site::{login_with, LoginOptions};
use chgk_rating::{Error, SiteClient};
use common::{html, json, redirect, serve, status};
fn site(s: &common::Server) -> SiteClient {
    SiteClient::with_base_url(reqwest::Client::new(), &s.url()).unwrap()
}

const LOGIN_PAGE: &str = r#"<form action="/login" method="post">
    <input type="hidden" name="_csrf_token" value="tok.abc">
    <input type="text" name="_username"> <input type="password" name="_password">
    </form>"#;

const HOME_LOGGED_IN: &str = r#"<span class="no-display" id="rt_user_idplayer">127696</span>
    <a class="dropdown-item" href="/logout">Выход</a>"#;

fn form_field(body: &str, name: &str) -> Option<String> {
    body.split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{}=", name)))
        .map(|v| {
            let mut out = Vec::new();
            let bytes = v.replace('+', " ").into_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'%' && i + 2 < bytes.len() {
                    let h = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                    out.push(u8::from_str_radix(h, 16).unwrap());
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            String::from_utf8(out).unwrap()
        })
}

// ---- create player / team ----------------------------------------------------

#[tokio::test]
async fn create_player_accepts_json_success_with_whitespace() {
    let s = serve(|_| json("{ \"success\" : true }")).await;
    site(&s)
        .create_player("Малкин", "Михаил", "")
        .await
        .unwrap();
    let r = &s.requests()[0];
    assert_eq!(r.path, "/player/create");
    assert_eq!(r.header("x-requested-with"), Some("XMLHttpRequest"));
    assert_eq!(form_field(&r.body, "surname").as_deref(), Some("Малкин"));
}

#[tokio::test]
async fn create_player_rejects_false_nested_and_malformed_success() {
    for body in [
        r#"{"success":false}"#,
        r#"{"data":{"success":true}}"#,
        "<html>oops</html>",
        "",
    ] {
        let s = serve(move |_| json(body)).await;
        match site(&s).create_player("A", "B", "").await {
            Err(Error::Site { action, .. }) => assert_eq!(action, "create_player"),
            other => panic!("body {:?}: expected Site error, got {:?}", body, other),
        }
    }
}

#[tokio::test]
async fn create_team_posts_name_and_town_id() {
    let s = serve(|_| json(r#"{"success":true}"#)).await;
    site(&s).create_team("Q&A", 2088).await.unwrap();
    let r = &s.requests()[0];
    assert_eq!(r.path, "/teams/create");
    assert_eq!(form_field(&r.body, "name").as_deref(), Some("Q&A"));
    assert_eq!(
        form_field(&r.body, "new-team-town").as_deref(),
        Some("2088")
    );
}

#[tokio::test]
async fn create_team_with_stale_session_is_session_expired() {
    let s = serve(|_| html(LOGIN_PAGE)).await;
    assert!(matches!(
        site(&s).create_team("X", 1).await,
        Err(Error::SessionExpired)
    ));
}

// Roster/results contracts are tested in submission_mock.rs.

// ---- login --------------------------------------------------------------------

fn login_opts(s: &common::Server) -> LoginOptions {
    LoginOptions::new().base_url(s.url())
}

#[tokio::test]
async fn login_keeps_the_cookies_and_the_base_url_in_the_session() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/login") => html(LOGIN_PAGE),
        ("POST", "/login") => redirect("/").header("set-cookie", "PHPSESSID=abc123; Path=/"),
        ("GET", "/") => {
            if r.header("cookie")
                .is_some_and(|c| c.contains("PHPSESSID=abc123"))
            {
                html(HOME_LOGGED_IN)
            } else {
                html(LOGIN_PAGE)
            }
        }
        _ => status(404, "nope"),
    })
    .await;
    let session = login_with("me@example.com", "pw", &login_opts(&s))
        .await
        .unwrap();
    assert_eq!(session.player_id, Some(127696));
    assert_eq!(session.cookie_header().unwrap(), "PHPSESSID=abc123");
    assert_eq!(session.base_url, s.url());
    // A client built from the session talks to the same server with the same cookies.
    let client = session.client().unwrap();
    assert_eq!(client.verify().await.unwrap(), Some(127696));
    // The persisted form round-trips, and an old file without base_url defaults to the real site.
    let json = serde_json::to_string(&session).unwrap();
    let back: chgk_rating::Session = serde_json::from_str(&json).unwrap();
    assert_eq!(back.base_url, s.url());
    let old: chgk_rating::Session =
        serde_json::from_str(r#"{"cookies_json":"[]","player_id":1,"login":"x","logged_in_at":0}"#)
            .unwrap();
    assert_eq!(old.base_url, chgk_rating::site::SITE_BASE);
}

#[tokio::test]
async fn login_reports_a_server_error_as_status_not_as_bad_credentials() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/login") => status(503, "<h1>Maintenance</h1>"),
        _ => html(LOGIN_PAGE),
    })
    .await;
    let err = login_with("me@example.com", "pw", &login_opts(&s))
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(503), "{:?}", err);

    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/login") => html(LOGIN_PAGE),
        ("POST", "/login") => status(500, "<h1>Error</h1>"),
        _ => html(LOGIN_PAGE),
    })
    .await;
    let err = login_with("me@example.com", "pw", &login_opts(&s))
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(500), "{:?}", err);

    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/login") => html(LOGIN_PAGE),
        ("POST", "/login") => html(""),
        ("GET", "/") => status(503, "<h1>Maintenance</h1>"),
        _ => status(404, "nope"),
    })
    .await;
    let err = login_with("me@example.com", "pw", &login_opts(&s))
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(503), "{:?}", err);
}

#[tokio::test]
async fn verify_distinguishes_logged_in_login_page_and_outage() {
    let s = serve(|_| html(HOME_LOGGED_IN)).await;
    assert_eq!(site(&s).verify().await.unwrap(), Some(127696));
    let s = serve(|_| html(LOGIN_PAGE)).await;
    assert!(matches!(
        site(&s).verify().await,
        Err(Error::SessionExpired)
    ));
    let s = serve(|_| status(502, "Bad Gateway")).await;
    assert_eq!(site(&s).verify().await.unwrap_err().status(), Some(502));
}
#[tokio::test]
async fn login_posts_the_csrf_token_and_reads_the_player_id() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/login") => html(LOGIN_PAGE),
        ("POST", "/login") => html(""),
        ("GET", "/") => html(HOME_LOGGED_IN),
        _ => status(404, "nope"),
    })
    .await;
    let session = login_with("me@example.com", "pw", &login_opts(&s))
        .await
        .unwrap();
    assert_eq!(session.player_id, Some(127696));
    assert_eq!(session.login, "me@example.com");
    let post = s
        .requests()
        .into_iter()
        .find(|r| r.method == "POST")
        .unwrap();
    assert_eq!(
        form_field(&post.body, "_csrf_token").as_deref(),
        Some("tok.abc")
    );
    assert_eq!(form_field(&post.body, "_password").as_deref(), Some("pw"));
    assert_eq!(
        form_field(&post.body, "_remember_me").as_deref(),
        Some("on")
    );
    assert!(
        !format!("{:?}", session).contains("cookies_json\": \""),
        "Debug must redact cookies"
    );
}

#[tokio::test]
async fn login_without_remember_me_omits_the_field() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/login") => html(LOGIN_PAGE),
        ("GET", "/") => html(HOME_LOGGED_IN),
        _ => html(""),
    })
    .await;
    let opts = login_opts(&s).remember_me(false);
    login_with("me@example.com", "pw", &opts).await.unwrap();
    let post = s
        .requests()
        .into_iter()
        .find(|r| r.method == "POST")
        .unwrap();
    assert!(
        form_field(&post.body, "_remember_me").is_none(),
        "{}",
        post.body
    );
}

#[tokio::test]
async fn login_failure_carries_the_sites_flash_message() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/login") => html(LOGIN_PAGE),
        ("POST", "/login") => html(&format!(
            "<div class=\"alert alert-danger\">Неверные &#27;[31mучётные данные.</div>{}",
            LOGIN_PAGE
        )),
        _ => html(LOGIN_PAGE),
    })
    .await;
    match login_with("me@example.com", "wrong", &login_opts(&s)).await {
        Err(Error::LoginFailed(msg)) => {
            assert!(msg.contains("Неверные"), "{}", msg);
            assert!(
                !msg.contains('\u{1b}'),
                "control characters must not survive: {:?}",
                msg
            );
        }
        other => panic!("expected LoginFailed, got {:?}", other),
    }
}
