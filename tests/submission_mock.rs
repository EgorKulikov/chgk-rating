//! Submission contracts taken from rating-site's representative controllers.
mod common;

use chgk_rating::csv::{Mark, ResultsRoundRow, RosterFlag, RosterRow};
use chgk_rating::{ApiClient, Error, SiteClient};
use common::{json, serve, status, Reply, Seen};
use serde_json::{json as value, Value};
use std::collections::BTreeMap;

const ROSTER_PATH: &str = "/api/tournaments/42/representative/roster";
const RESULTS_PATH: &str = "/api/tournaments/42/representative/results";

fn clients(s: &common::Server) -> (SiteClient, ApiClient) {
    (
        SiteClient::with_base_url(reqwest::Client::new(), &s.url()).unwrap(),
        ApiClient::with_base_url(reqwest::Client::new(), &s.url()).unwrap(),
    )
}

fn row(player_id: i64, flag: Option<RosterFlag>) -> RosterRow {
    RosterRow {
        team_id: 9,
        team_name: "Q&A \"2024\"".into(),
        town: "Berlin".into(),
        player_id,
        surname: "Иванов".into(),
        name: "Иван".into(),
        patronymic: String::new(),
        flag,
    }
}

fn roster_reads(r: &Seen) -> Reply {
    if r.method == "GET" && r.path == ROSTER_PATH {
        return json("[]");
    }
    match r.path.as_str() {
        p if p.starts_with("/players/") => {
            let id: i64 = p.trim_start_matches("/players/").parse().unwrap();
            json(&value!({"id":id,"surname":"Иванов","name":"Иван","patronymic":""}).to_string())
        }
        "/tournaments/42" => json(
            r#"{"id":42,"name":"Old tournament","idseason":57,"dateEnd":"2024-02-01T12:00:00+03:00","questionQty":{"1":2,"2":3}}"#,
        ),
        "/teams/9" => json(r#"{"id":9,"name":"Registered team"}"#),
        p if p.starts_with("/teams/9/seasons?") => json(
            r#"[
            {"idplayer":3,"idteam":9,"idseason":57},
            {"idplayer":4,"idteam":9,"idseason":57,"dateAdded":"2023-07-21T00:00:00+00:00"},
            {"idplayer":5,"idteam":9,"idseason":60},
            {"idplayer":6,"idteam":99,"idseason":57},
            {"idplayer":7,"idteam":9,"idseason":57,"dateRemoved":"2024-01-01T00:00:00+00:00"},
            {"idplayer":8,"idteam":9,"idseason":57,"dateRemoved":"2024-03-01T00:00:00+00:00"},
            {"idplayer":9,"idteam":9,"idseason":57,"dateAdded":"2024-02-01T10:00:00+00:00"},
            {"idplayer":10,"idteam":9,"idseason":57,"dateRemoved":"2024-02-01T09:00:00+00:00"}
        ]"#,
        ),
        _ => status(404, "unexpected request"),
    }
}

#[tokio::test]
async fn rosters_preserve_explicit_flags_and_infer_only_missing_flags_in_tournament_season() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("POST", ROSTER_PATH) => json(r#"{"saved":true}"#),
        _ => roster_reads(r),
    })
    .await;
    let (site, api) = clients(&s);
    let rows = vec![
        row(1, Some(RosterFlag::Captain)),
        row(2, Some(RosterFlag::Base)),
        row(3, Some(RosterFlag::Legionnaire)),
        row(4, None),
        row(5, None),
        row(6, None),
        row(7, None),
        row(8, None),
        row(9, None),
        row(10, None),
    ];
    let original = rows.clone();
    let report = site.upload_rosters(&api, 42, &rows).await.unwrap();
    assert_eq!(
        rows, original,
        "inference must not change the caller's rows"
    );
    assert!(report.sanitized.is_empty());
    assert_eq!(report.renamed_on_tournament[0].name, "Q&A \"2024\"");
    let requests = s.requests();
    let posts: Vec<_> = requests.iter().filter(|r| r.method == "POST").collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].path, ROSTER_PATH);
    assert_eq!(posts[0].header("content-type"), Some("application/json"));
    let body: Value = serde_json::from_str(&posts[0].body).unwrap();
    assert_eq!(body["teams"][0]["teamName"], "Q&A \"2024\"");
    assert_eq!(body["teams"][0]["teamId"], 9);
    let members = body["teams"][0]["rows"].as_array().unwrap();
    let flags: Vec<_> = members
        .iter()
        .map(|p| p["flag"].as_str().unwrap())
        .collect();
    assert_eq!(flags, ["К", "Б", "Л", "Б", "Л", "Л", "Л", "Б", "Л", "Л"]);
    assert!(members.iter().all(|p| p["checked"] == true));
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.path.starts_with("/teams/9/seasons?"))
            .count(),
        1
    );
}

#[tokio::test]
async fn rosters_with_explicit_flags_need_no_season_lookup() {
    let s = serve(|r| match r.path.as_str() {
        ROSTER_PATH => json(r#"{"saved":true}"#),
        "/teams/9" => json(r#"{"id":9,"name":"Registered team"}"#),
        p if p.starts_with("/players/") => roster_reads(r),
        _ => status(500, "season lookup must not be needed"),
    })
    .await;
    let (site, api) = clients(&s);
    site.upload_rosters(&api, 42, &[row(1, Some(RosterFlag::Captain))])
        .await
        .unwrap();
    assert!(s
        .requests()
        .iter()
        .all(|r| !r.path.contains("season") && !r.path.starts_with("/tournaments")));
}

#[tokio::test]
async fn unknown_player_prevents_roster_replacement() {
    let s = serve(|r| match r.path.as_str() {
        "/players/12345" => status(404, "Player not found"),
        ROSTER_PATH => json(r#"{"saved":true}"#),
        _ => roster_reads(r),
    })
    .await;
    let (site, api) = clients(&s);
    let err = site
        .upload_rosters(&api, 42, &[row(12345, Some(RosterFlag::Captain))])
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(404));
    assert!(s.requests().iter().all(|r| r.method == "GET"));
}

#[tokio::test]
async fn redirected_player_id_does_not_silently_drop_a_roster_member() {
    let s = serve(|r| match r.path.as_str() {
        "/players/12345" => json(r#"{"id":999,"surname":"Иванов","name":"Иван"}"#),
        ROSTER_PATH => json(r#"{"saved":true}"#),
        _ => roster_reads(r),
    })
    .await;
    let (site, api) = clients(&s);
    assert!(site
        .upload_rosters(&api, 42, &[row(12345, Some(RosterFlag::Captain))])
        .await
        .is_err());
    assert!(s.requests().iter().all(|r| r.method == "GET"));
}

#[tokio::test]
async fn roster_lookup_failures_never_fall_back_to_legionnaire_or_write() {
    for fail_path in ["/tournaments/42", "/teams/9/seasons", "/teams/9"] {
        let s = serve(move |r| {
            if r.path.split('?').next() == Some(fail_path) {
                status(503, "unavailable")
            } else {
                roster_reads(r)
            }
        })
        .await;
        let (site, api) = clients(&s);
        assert_eq!(
            site.upload_rosters(&api, 42, &[row(4, None)])
                .await
                .unwrap_err()
                .status(),
            Some(503)
        );
        assert!(s.requests().iter().all(|r| r.method == "GET"));
    }
}

#[tokio::test]
async fn missing_tournament_season_does_not_guess_from_latest_roster() {
    let s = serve(|r| {
        if r.path == "/tournaments/42" {
            json(r#"{"id":42,"name":"No season"}"#)
        } else {
            roster_reads(r)
        }
    })
    .await;
    let (site, api) = clients(&s);
    let err = site
        .upload_rosters(&api, 42, &[row(4, None)])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Site { .. }));
    assert!(err.to_string().contains("season"));
    assert!(s.requests().iter().all(|r| r.method == "GET"));
}

#[tokio::test]
async fn roster_saved_true_with_errors_is_failure() {
    let s = serve(|r| {
        if r.path == ROSTER_PATH { json(r#"{"saved":true,"errors":[{"teamId":9,"playerId":4,"message":"Wrong base roster"}]}"#) }
        else { roster_reads(r) }
    }).await;
    let (site, api) = clients(&s);
    let err = site
        .upload_rosters(&api, 42, &[row(4, None)])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Site { .. }));
    assert!(err.to_string().contains("Wrong base roster"));
}

#[tokio::test]
async fn roster_upload_rejects_duplicate_players_and_inconsistent_team_names_before_requests() {
    let mut conflicting = row(2, None);
    conflicting.team_name = "Another name".into();
    for rows in [
        vec![row(1, None), row(1, None)],
        vec![row(1, None), conflicting],
        vec![],
    ] {
        let s = serve(|_| json(r#"{"saved":true}"#)).await;
        let (site, api) = clients(&s);
        assert!(site.upload_rosters(&api, 42, &rows).await.is_err());
        assert!(s.requests().is_empty());
    }
}

fn result_row(round: u32, marks: Vec<Mark>) -> ResultsRoundRow {
    ResultsRoundRow {
        team_id: 9,
        team_name: "Q&A \"2024\"".into(),
        town: "Berlin".into(),
        round,
        marks,
        controversials: BTreeMap::new(),
    }
}

fn results_reads(r: &Seen) -> Reply {
    if r.path == format!("{RESULTS_PATH}?admin=1") {
        let mut request = r.clone();
        request.path = RESULTS_PATH.into();
        return results_reads(&request);
    }
    match (r.method.as_str(), r.path.as_str()) {
        ("GET", RESULTS_PATH) => json(
            r#"{
            "meta":{"tournamentId":42,"questionCount":5},
            "teams":[
                {"id":9,"name":"Q&A \"2024\"","town":"Berlin","questions":{"1":1,"2":"старый спорный","3":1,"4":0,"5":0}},
                {"id":99,"name":"Other team","town":"Paris","questions":{"1":1,"2":1,"3":1,"4":1,"5":1}}
            ],"controversials":[]
        }"#,
        ),
        _ => roster_reads(r),
    }
}

#[tokio::test]
async fn results_preserve_existing_answers_of_teams_outside_the_default_read_scope() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", RESULTS_PATH) => {
            json(r#"{"meta":{"tournamentId":42,"questionCount":5},"teams":[]}"#)
        }
        ("GET", p) if p == format!("{RESULTS_PATH}?admin=1") => {
            let mut request = r.clone();
            request.path = RESULTS_PATH.into();
            results_reads(&request)
        }
        ("POST", RESULTS_PATH) => json(r#"{"saved":true}"#),
        _ => roster_reads(r),
    })
    .await;
    let (site, api) = clients(&s);
    site.upload_results_with(&api, 42, &[result_row(2, vec![Mark::Wrong; 3])])
        .await
        .unwrap();
    let requests = s.requests();
    let post = requests.iter().find(|r| r.method == "POST").unwrap();
    let payload: Value = serde_json::from_str(&post.body).unwrap();
    assert_eq!(payload["teams"][0]["questions"]["1"], 1);
    assert_eq!(payload["teams"][0]["questions"]["2"], "старый спорный");
}

#[tokio::test]
async fn organizer_without_own_request_resolves_admin_scope_before_either_write() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", ROSTER_PATH | RESULTS_PATH) => status(403, "No own request"),
        ("GET", p) if p == format!("{ROSTER_PATH}?admin=1") => json("[]"),
        ("GET", p) if p == format!("{RESULTS_PATH}?admin=1") => results_reads(r),
        ("POST", p) if p.ends_with("?admin=1") => json(r#"{"saved":true}"#),
        _ => roster_reads(r),
    })
    .await;
    let (site, api) = clients(&s);
    site.upload_rosters(&api, 42, &[row(1, Some(RosterFlag::Captain))])
        .await
        .unwrap();
    site.upload_results_with(&api, 42, &[result_row(2, vec![Mark::Wrong; 3])])
        .await
        .unwrap();
    let requests = s.requests();
    let posts: Vec<_> = requests.iter().filter(|r| r.method == "POST").collect();
    assert_eq!(posts.len(), 2);
    assert!(posts.iter().all(|r| r.path.ends_with("?admin=1")));
}

#[tokio::test]
async fn incomplete_legacy_snapshot_requires_all_rounds_before_replacing_results() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", RESULTS_PATH) => json(
            r#"{
            "meta":{"tournamentId":42,"questionCount":5},
            "teams":[{"id":9,"questions":{"1":1,"2":0,"3":1,"4":null,"5":null}}]
        }"#,
        ),
        ("POST", RESULTS_PATH) => json(r#"{"saved":true}"#),
        _ => roster_reads(r),
    })
    .await;
    let (site, api) = clients(&s);
    // Backend concatenates the only saved round (round 2) into questions 1..3.
    let err = site
        .upload_results_with(&api, 42, &[result_row(1, vec![Mark::Wrong; 2])])
        .await
        .unwrap_err();
    assert!(err.to_string().contains("all rounds"));
    assert!(s.requests().iter().all(|r| r.method == "GET"));
    site.upload_results_with(
        &api,
        42,
        &[
            result_row(1, vec![Mark::Wrong; 2]),
            result_row(2, vec![Mark::Correct; 3]),
        ],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn failed_or_inconsistent_full_snapshot_prevents_results_write() {
    for reply in [
        status(503, "unavailable"),
        json(r#"{"meta":{"tournamentId":99,"questionCount":5},"teams":[]}"#),
    ] {
        let s = serve(move |r| match (r.method.as_str(), r.path.as_str()) {
            ("GET", RESULTS_PATH) => {
                json(r#"{"meta":{"tournamentId":42,"questionCount":5},"teams":[]}"#)
            }
            ("GET", p) if p.ends_with("?admin=1") => reply.clone(),
            ("POST", _) => json(r#"{"saved":true}"#),
            _ => roster_reads(r),
        })
        .await;
        let (site, api) = clients(&s);
        assert!(site
            .upload_results_with(&api, 42, &[result_row(1, vec![Mark::Correct; 2])])
            .await
            .is_err());
        assert!(s.requests().iter().all(|r| r.method == "GET"));
    }
}

#[tokio::test]
async fn controversials_cannot_conceal_sparse_legacy_rounds_during_partial_update() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("GET", RESULTS_PATH) => json(r#"{
            "meta":{"tournamentId":42,"questionCount":5},
            "teams":[{"id":9,"questions":{"1":1,"2":0,"3":0,"4":"first disputed answer","5":"second disputed answer"}}]
        }"#),
        ("POST", RESULTS_PATH) => json(r#"{"saved":true}"#),
        _ => roster_reads(r),
    }).await;
    let (site, api) = clients(&s);
    let err = site
        .upload_results_with(&api, 42, &[result_row(1, vec![Mark::Wrong; 2])])
        .await
        .unwrap_err();
    assert!(err.to_string().contains("all rounds"));
    assert!(s.requests().iter().all(|r| r.method == "GET"));
}

#[tokio::test]
async fn results_never_accept_html_as_a_successful_save() {
    let s = serve(|_| common::html("<h1>Tournament page</h1>")).await;
    let (site, _) = clients(&s);
    assert!(site
        .upload_results(42, &[result_row(1, vec![Mark::Correct, Mark::Wrong])])
        .await
        .is_err());
}

#[tokio::test]
async fn results_merge_rounds_using_actual_question_counts_and_preserve_other_answers() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("POST", RESULTS_PATH) => json(r#"{"saved":true}"#),
        _ => results_reads(r),
    })
    .await;
    let (site, api) = clients(&s);
    let mut row = result_row(2, vec![Mark::Correct, Mark::Contested, Mark::Blank]);
    row.controversials.insert(1, "Q&A, \"ответ\"".into());
    site.upload_results_with(&api, 42, &[row]).await.unwrap();
    let requests = s.requests();
    let posts: Vec<_> = requests.iter().filter(|r| r.method == "POST").collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].path, RESULTS_PATH);
    let body: Value = serde_json::from_str(&posts[0].body).unwrap();
    assert_eq!(
        body,
        value!({"teams":[{"id":9,"questions":{
            "1":1,"2":"старый спорный","3":1,"4":"Q&A, \"ответ\"","5":0
        }}]})
    );
}

#[tokio::test]
async fn results_replace_submitted_rounds_and_fill_unplayed_rounds_for_new_teams() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("POST", RESULTS_PATH) => json(r#"{"saved":true}"#),
        _ => results_reads(r),
    })
    .await;
    let (site, api) = clients(&s);
    let mut new_team = result_row(2, vec![Mark::Contested, Mark::Wrong, Mark::Correct]);
    new_team.team_id = 100;
    site.upload_results_with(
        &api,
        42,
        &[
            new_team,
            result_row(2, vec![Mark::Wrong; 3]),
            result_row(1, vec![Mark::Wrong, Mark::Correct]),
        ],
    )
    .await
    .unwrap();
    let requests = s.requests();
    let post = requests.iter().find(|r| r.method == "POST").unwrap();
    let body: Value = serde_json::from_str(&post.body).unwrap();
    assert_eq!(
        body,
        value!({"teams":[
            {"id":9,"questions":{"1":0,"2":1,"3":0,"4":0,"5":0}},
            {"id":100,"questions":{"1":0,"2":0,"3":"?","4":0,"5":1}}
        ]})
    );
}

#[tokio::test]
async fn results_reject_duplicate_invalid_or_incomplete_rounds_before_writing() {
    for rows in [
        vec![],
        vec![result_row(0, vec![Mark::Correct; 2])],
        vec![result_row(3, vec![Mark::Correct; 2])],
        vec![result_row(2, vec![Mark::Correct; 2])],
        vec![result_row(1, vec![Mark::Correct; 3])],
        vec![result_row(1, vec![Mark::Correct; 2]); 2],
    ] {
        let s = serve(results_reads).await;
        let (site, api) = clients(&s);
        assert!(site.upload_results_with(&api, 42, &rows).await.is_err());
        assert!(s.requests().iter().all(|r| r.method == "GET"));
    }
}

#[tokio::test]
async fn results_report_partial_saves_instead_of_silently_accepting_warnings() {
    let s = serve(|r| match (r.method.as_str(), r.path.as_str()) {
        ("POST", RESULTS_PATH) => json(
            r#"{"saved":true,"warnings":["Team 9 belongs to a different request — skipped."]}"#,
        ),
        _ => results_reads(r),
    })
    .await;
    let (site, api) = clients(&s);
    let err = site
        .upload_results_with(&api, 42, &[result_row(1, vec![Mark::Correct; 2])])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Site { .. }));
    assert!(err.to_string().contains("skipped"));
    assert!(err.to_string().contains("may have been saved"));
}

#[tokio::test]
async fn submission_responses_require_explicit_success_and_reject_json_errors() {
    for response in [
        r#"{"saved":false}"#,
        r#"{}"#,
        r#"{"success":true}"#,
        r#"{"data":{"saved":true}}"#,
        r#"{"saved":true,"error":"rejected"}"#,
        r#"{"saved":true,"errors":["rejected"]}"#,
        r#"{"saved":true,"warnings":"bad shape"}"#,
        "<h1>Tournament</h1>",
        "",
    ] {
        let s = serve(move |r| {
            if r.method == "POST" {
                json(response)
            } else {
                results_reads(r)
            }
        })
        .await;
        let (site, api) = clients(&s);
        assert!(
            site.upload_rosters(&api, 42, &[row(1, Some(RosterFlag::Captain))])
                .await
                .is_err(),
            "roster: {response}"
        );
        assert!(
            site.upload_results_with(&api, 42, &[result_row(1, vec![Mark::Correct; 2])])
                .await
                .is_err(),
            "results: {response}"
        );
    }
}

#[tokio::test]
async fn roster_warnings_are_returned_after_successful_save() {
    let s = serve(|r| {
        if r.method == "POST" { json(r#"{"saved":true,"warnings":[{"teamId":9,"playerId":1,"message":"Player is disqualified"}]}"#) }
        else { roster_reads(r) }
    }).await;
    let (site, api) = clients(&s);
    let report = site
        .upload_rosters(&api, 42, &[row(1, Some(RosterFlag::Captain))])
        .await
        .unwrap();
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("Player is disqualified"));
}

#[tokio::test]
async fn session_expiry_and_permission_errors_are_distinguished_on_new_endpoints() {
    const LOGIN: &str = r#"<form><input name="_csrf_token"><input name="_username"></form>"#;
    for (code, body, expired) in [
        (200, LOGIN, true),
        (403, LOGIN, true),
        (401, "{}", true),
        (403, "Access denied", false),
        (422, "Validation failed", false),
        (409, "Write lock busy", false),
    ] {
        let s = serve(move |r| {
            if matches!(r.path.split('?').next(), Some(ROSTER_PATH | RESULTS_PATH)) {
                status(code, body)
            } else {
                roster_reads(r)
            }
        })
        .await;
        let (site, api) = clients(&s);
        for err in [
            site.upload_rosters(&api, 42, &[row(1, Some(RosterFlag::Captain))])
                .await
                .unwrap_err(),
            site.upload_results_with(&api, 42, &[result_row(1, vec![Mark::Correct; 2])])
                .await
                .unwrap_err(),
        ] {
            assert_eq!(err.is_session_expired(), expired, "{err:?}");
            if !expired {
                assert_eq!(err.status(), Some(code));
            }
        }
    }
}
