//! `ApiClient` against a local server: error propagation and the
//! shapes the live API is known to produce.

mod common;

use chgk_rating::{ApiClient, Error};
use common::{json, serve, status, Reply, Seen};

/// `total` JSON rows built by `row(i)`, limited by the request's
/// `itemsPerPage` and `page`, so pagination behaves like the real API.
fn paged(r: &Seen, total: usize, row: impl Fn(usize) -> String) -> Reply {
    let per = items_per_page(r);
    let page: usize = r
        .path
        .split(['?', '&'])
        .find_map(|kv| kv.strip_prefix("page="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let from = (page - 1) * per + 1;
    let to = (from + per - 1).min(total);
    let rows: Vec<String> = (from..=to).map(row).collect();
    json(&format!("[{}]", rows.join(",")))
}
fn client(s: &common::Server) -> ApiClient {
    ApiClient::with_base_url(reqwest::Client::new(), &s.url()).unwrap()
}

fn not_found() -> Reply {
    status(404, r#"{"detail":"Not Found"}"#)
}

fn items_per_page(r: &Seen) -> usize {
    r.path
        .split(['?', '&'])
        .find_map(|kv| kv.strip_prefix("itemsPerPage="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

#[tokio::test]
async fn server_error_is_a_status_error_not_an_empty_list() {
    let s = serve(|_| status(500, "<h1>Internal Server Error</h1>")).await;
    let err = client(&s).search_teams("x", 5).await.unwrap_err();
    match err {
        Error::Status {
            status,
            url,
            summary,
        } => {
            assert_eq!(status, 500);
            assert!(url.contains("/teams"), "{}", url);
            assert!(summary.contains("Internal Server Error"), "{}", summary);
        }
        other => panic!("expected Status, got {:?}", other),
    }
}

#[tokio::test]
async fn unknown_id_is_status_404() {
    let s = serve(|_| not_found()).await;
    let err = client(&s).get_team(999).await.unwrap_err();
    assert_eq!(err.status(), Some(404), "{:?}", err);
}

#[tokio::test]
async fn find_teams_falls_through_to_search_only_on_404() {
    let s = serve(|r| match r.path.as_str() {
        "/teams/5" => not_found(),
        "/teams/7" => status(500, "boom"),
        p if p.starts_with("/teams?") => json(r#"[{"id":5,"name":"5 minutes"}]"#),
        _ => not_found(),
    })
    .await;
    let c = client(&s);
    let found = c.find_teams("5", 3).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "5 minutes");

    let err = c.find_teams("7", 3).await.unwrap_err();
    assert_eq!(err.status(), Some(500), "{:?}", err);
    assert!(
        !s.requests()
            .iter()
            .any(|r| r.path.starts_with("/teams?name=7")),
        "a non-404 failure must not fall through to the name search"
    );
}

#[tokio::test]
async fn venue_list_keeps_the_orgcommittee() {
    let s = serve(|r| match r.path.as_str() {
        p if p.starts_with("/venues/1/requests") => json(
            r#"[{"id":1,"status":"A","tournamentId":10,"dateStart":"2026-08-29T12:00:00+00:00","representative":{"id":127696,"name":"Роберт","surname":"Михайлюк"}}]"#,
        ),
        "/tournaments/10" => json(
            r#"{"id":10,"name":"T","dateStart":"2026-08-29T12:00:00+00:00","questionQty":[12,12],"orgcommittee":[{"id":7420},{"id":8}]}"#,
        ),
        _ => not_found(),
    })
    .await;
    let list = client(&s)
        .list_synch_tournaments_for_venue(1)
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].orgcommittee, vec![7420, 8]);
    assert_eq!(list[0].representative_id, Some(127696));
    assert_eq!(list[0].questions_per_round, vec![12, 12]);
}

#[tokio::test]
async fn venue_pager_propagates_a_failed_page_instead_of_truncating() {
    let s = serve(|r| {
        if r.path.contains("/venues/1/requests")
            && (r.path.contains("page=1&") || r.path.ends_with("page=1"))
        {
            let rows: Vec<String> = (1..=100)
                .map(|i| format!(r#"{{"id":{},"status":"A","tournamentId":{}}}"#, i, i))
                .collect();
            json(&format!("[{}]", rows.join(",")))
        } else if r.path.contains("/venues/1/requests") {
            status(502, "Bad Gateway")
        } else {
            json(r#"{"id":1,"name":"T"}"#)
        }
    })
    .await;
    let err = client(&s)
        .list_synch_tournaments_for_venue(1)
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(502), "{:?}", err);
}

#[tokio::test]
async fn pager_requests_the_next_page_after_an_exactly_full_one() {
    let s = serve(|r| {
        paged(r, 100, |i| {
            format!(r#"{{"id":{},"status":"A","tournamentId":{}}}"#, i, i)
        })
    })
    .await;
    let reqs = client(&s).get_tournament_requests(1).await.unwrap();
    assert_eq!(reqs.len(), 100);
    let paths: Vec<String> = s.requests().iter().map(|r| r.path.clone()).collect();
    assert_eq!(paths.len(), 2, "{:?}", paths);
    assert!(
        paths[0].contains("itemsPerPage=100") && paths[0].contains("page=1"),
        "{}",
        paths[0]
    );
    assert!(paths[1].contains("page=2"), "{}", paths[1]);
}

#[tokio::test]
async fn pager_refuses_to_return_a_truncated_list_at_its_cap() {
    // A server that keeps answering full pages; `list_venue_types` has the
    // smallest cap, so it must give up with an error, not a short list.
    let s = serve(|r| {
        paged(r, 1_000_000, |i| {
            format!(r#"{{"id":{},"name":"t{}"}}"#, i, i)
        })
    })
    .await;
    let err = client(&s).list_venue_types().await.unwrap_err();
    assert!(matches!(err, Error::Parse(_)), "{:?}", err);
    assert!(err.to_string().contains("page"), "{}", err);
    assert!(
        s.requests().len() < 20,
        "must stop at the cap, made {} requests",
        s.requests().len()
    );
}

#[tokio::test]
async fn undecodable_json_is_a_parse_error_naming_the_endpoint() {
    let s = serve(|_| json("<html>not json</html>")).await;
    let err = client(&s).search_teams("x", 5).await.unwrap_err();
    match err {
        Error::Parse(msg) => assert!(msg.contains("/teams"), "{}", msg),
        other => panic!("expected Parse, got {:?}", other),
    }
}

#[tokio::test]
async fn find_players_falls_through_to_search_only_on_404() {
    let s = serve(|r| match r.path.as_str() {
        "/players/5" => not_found(),
        "/players/7" => status(500, "boom"),
        p if p.starts_with("/players?") => json(r#"[{"id":5,"name":"Пять","surname":"Пятов"}]"#),
        _ => not_found(),
    })
    .await;
    let c = client(&s);
    let found = c.find_players("5", 3).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].surname, "Пятов");
    let err = c.find_players("7", 3).await.unwrap_err();
    assert_eq!(err.status(), Some(500), "{:?}", err);
    assert!(!s
        .requests()
        .iter()
        .any(|r| r.path.starts_with("/players?surname=7")));
}

const SEASONS_OF_TEAM_9: &str = r#"[
    {"idplayer": 1, "idseason": 57, "idteam": 9, "dateAdded": "2023-07-21T00:00:00+00:00", "dateRemoved": null, "playerNumber": 0},
    {"idplayer": 2, "idseason": 57, "idteam": 9, "dateAdded": "2023-07-21T00:00:00+00:00", "dateRemoved": "2024-01-01T00:00:00+00:00", "playerNumber": 0},
    {"idplayer": 3, "idseason": 57, "idteam": 9, "dateAdded": "2023-07-21T00:00:00+00:00", "dateRemoved": null, "playerNumber": 0},
    {"idplayer": 4, "idseason": 50, "idteam": 9, "dateAdded": "2017-07-21T00:00:00+00:00", "dateRemoved": null, "playerNumber": 0}
]"#;

#[tokio::test]
async fn base_roster_skips_removed_players_and_404s_but_propagates_500() {
    let s = serve(|r| match r.path.as_str() {
        p if p.starts_with("/teams/9/seasons") => json(SEASONS_OF_TEAM_9),
        "/players/1" => json(r#"{"id":1,"name":"A","surname":"B"}"#),
        _ => not_found(),
    })
    .await;
    let roster = client(&s).get_base_roster(9).await.unwrap();
    assert_eq!(roster.iter().map(|p| p.id).collect::<Vec<_>>(), vec![1]);
    assert!(
        !s.requests().iter().any(|r| r.path == "/players/2"),
        "removed player must not be looked up"
    );

    let s = serve(|r| match r.path.as_str() {
        p if p.starts_with("/teams/9/seasons") => json(SEASONS_OF_TEAM_9),
        "/players/1" => status(500, "boom"),
        _ => not_found(),
    })
    .await;
    let err = client(&s).get_base_roster(9).await.unwrap_err();
    assert_eq!(err.status(), Some(500), "{:?}", err);
}

const REQUESTS_OF_VENUE_1: &str = r#"[{"id":1,"status":"A","tournamentId":10,"dateStart":"2026-01-01T00:00:00+00:00"},
    {"id":2,"status":"A","tournamentId":11,"dateStart":"2026-02-01T00:00:00+00:00"}]"#;

#[tokio::test]
async fn venue_list_skips_404_tournaments_but_propagates_500() {
    let s = serve(|r| match r.path.as_str() {
        p if p.starts_with("/venues/1/requests") => json(REQUESTS_OF_VENUE_1),
        "/tournaments/10" => json(r#"{"id":10,"name":"T"}"#),
        _ => not_found(),
    })
    .await;
    let list = client(&s)
        .list_synch_tournaments_for_venue(1)
        .await
        .unwrap();
    assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), vec![10]);

    let s = serve(|r| match r.path.as_str() {
        p if p.starts_with("/venues/1/requests") => json(REQUESTS_OF_VENUE_1),
        "/tournaments/11" => status(500, "boom"),
        _ => json(r#"{"id":10,"name":"T"}"#),
    })
    .await;
    let err = client(&s)
        .list_synch_tournaments_for_venue(1)
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(500), "{:?}", err);
}

#[tokio::test]
async fn lookups_scan_a_wide_window_of_substring_matches() {
    // 20 towns contain the needle; the exact match is the 15th, beyond a
    // 10-row window.
    let s = serve(|r| {
        paged(r, 20, |i| {
            let name = if i == 15 {
                "Киров".to_string()
            } else {
                format!("Киров-{}", i)
            };
            format!(r#"{{"id":{},"name":"{}"}}"#, i, name)
        })
    })
    .await;
    assert_eq!(client(&s).lookup_town_id("киров").await.unwrap(), Some(15));
}

#[tokio::test]
async fn countries_carry_their_ids() {
    let s = serve(|r| {
        let all = [
            r#"{"id":36,"name":"Польша-Литва"}"#,
            r#"{"id":35,"name":"Польша"}"#,
        ];
        let n = items_per_page(r).min(all.len());
        json(&format!("[{}]", all[..n].join(",")))
    })
    .await;
    let c = client(&s);
    let found = c.search_countries("Поль", 10).await.unwrap();
    assert_eq!((found[1].id, found[1].name.as_str()), (35, "Польша"));
    let exact = c.lookup_country("польша").await.unwrap().unwrap();
    assert_eq!((exact.id, exact.name.as_str()), (35, "Польша"));
}

#[tokio::test]
async fn get_town_without_a_name_is_a_parse_error() {
    let s = serve(|_| json(r#"{"id": 2088}"#)).await;
    assert!(matches!(
        client(&s).get_town(2088).await,
        Err(Error::Parse(_))
    ));
}

#[tokio::test]
async fn oversized_bodies_are_refused() {
    let big = format!("[{}]", "\"x\",".repeat(2_500_000).trim_end_matches(','));
    assert!(big.len() > 8 << 20);
    let s = serve(move |_| json(&big)).await;
    let err = client(&s).search_countries("x", 1).await.unwrap_err();
    assert!(matches!(err, Error::Parse(_)), "{:?}", err);
    assert!(err.to_string().contains("large"), "{}", err);
}

#[tokio::test]
async fn bad_base_url_is_a_config_error() {
    assert!(matches!(
        ApiClient::with_base_url(reqwest::Client::new(), "not a url"),
        Err(Error::Config(_))
    ));
}

#[tokio::test]
async fn venues_keep_the_town_id() {
    let s = serve(|_| {
        json(r#"[{"id":3360,"name":"Краков","town":{"id":2088,"name":"Краков","country":{"id":35,"name":"Польша"}}}]"#)
    })
    .await;
    let v = client(&s).search_venues("Краков", 5).await.unwrap();
    assert_eq!(v[0].town.as_ref().unwrap().id, Some(2088));
}

#[tokio::test]
async fn venue_requests_without_a_tournament_are_skipped() {
    let s = serve(|r| match r.path.as_str() {
        p if p.starts_with("/venues/1/requests") => {
            json(r#"[{"id":1,"status":"A"},{"id":2,"status":"A","tournamentId":10}]"#)
        }
        "/tournaments/10" => json(r#"{"id":10,"name":"T"}"#),
        _ => not_found(),
    })
    .await;
    let list = client(&s)
        .list_synch_tournaments_for_venue(1)
        .await
        .unwrap();
    assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), vec![10]);
}

#[tokio::test]
async fn unpublished_result_fields_are_none() {
    let s = serve(|_| {
        json(
            r#"[{"team":{"id":86732,"name":"4:20","town":{"name":"Краков"}},
                 "mask":null,"questionsTotal":null,"position":null,"controversials":[]}]"#,
        )
    })
    .await;
    let rows = client(&s).get_tournament_results(14015).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].team_id, 86732);
    assert_eq!(rows[0].town.as_deref(), Some("Краков"));
    assert_eq!(rows[0].mask, None);
    assert_eq!(rows[0].questions_total, None);
    assert_eq!(rows[0].position, None);
    assert_eq!(rows[0].venue_id, None);
}

#[tokio::test]
async fn controversials_propagate_errors() {
    let s = serve(|_| status(500, "boom")).await;
    let err = client(&s)
        .get_tournament_controversials(1)
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(500), "{:?}", err);
}

#[tokio::test]
async fn towns_without_an_id_are_skipped() {
    let s = serve(|_| {
        json(r#"[{"name":"Nowhere"},{"id":2088,"name":"Краков","country":{"name":"Польша"}}]"#)
    })
    .await;
    let towns = client(&s).search_towns("x", 5).await.unwrap();
    assert_eq!(towns.len(), 1);
    assert_eq!(towns[0].id, 2088);
    assert_eq!(client(&s).lookup_town_id("Nowhere").await.unwrap(), None);
}

// ---- endpoints added after the coverage audit of the OpenAPI spec ------------

use chgk_rating::api::{ResultsQuery, TournamentQuery};

const TOURNAMENT_14015: &str = r#"{
  "idtown": null,
  "type": {"id": 3, "name": "Синхрон", "shortName": "С"},
  "id": 14015, "name": "Чудове Чудовисько", "longName": "Чудове Чудовисько (повна назва)",
  "lastEditDate": "2026-09-01T10:00:00+00:00", "regulationsUrl": "https://example.org/reg","dateStart": "2026-08-29T12:00:00+00:00", "dateEnd": "2026-09-08T20:59:00+00:00",
  "paymentCategories": [], "idseason": 61,
  "orgcommittee": [{"id": 7420, "name": "A", "surname": "B"}],
  "languages": [{"name": "Украинский", "id": "uk"}],
  "editors": [{"id": 1, "name": "E", "surname": "D"}], "gameJury": [{"id": 2, "name": "G", "surname": "J"}], "appealJury": [{"id": 3, "name": "A", "surname": "J"}],"ratingSystems": ["mak"], "maiiRating": true, "rating": true,"dateDownloadQuestionsFrom": "2026-08-29T15:00:00+00:00",
  "dateRequestsAllowedTo": "2026-09-08T20:59:00+00:00",
  "dateAppealAllowedTo": "2026-09-10T20:59:00+00:00",
  "hideResultsTo": "2026-09-08T20:59:00+00:00",
  "difficultyForecast": 5.5, "archive": false,
  "questionQty": {"1": 12, "2": 12, "3": 12}
}"#;

const REQUEST_189504: &str = r#"{"id": 189504, "status": "A",
  "venue": {"id": 3360, "name": "Краков", "town": {"id": 2088, "name": "Краков", "country": {"id": 35, "name": "Польша"}}, "type": {"id": 1, "name": "Постоянная"}, "urls": []},
  "representative": {"id": 127696, "name": "Роберт", "patronymic": "Олегович", "surname": "Михайлюк"},
  "narrator": null, "approximateTeamsCount": 5,
  "issuedAt": "2026-08-31T09:08:20+00:00", "dateStart": "2026-09-02T17:00:00+00:00", "tournamentId": 14015}"#;

#[tokio::test]
async fn get_tournament_keeps_the_extended_fields() {
    let s = serve(|_| json(TOURNAMENT_14015)).await;
    let t = client(&s).get_tournament(14015).await.unwrap();
    assert_eq!(t.type_id, Some(3));
    assert_eq!(t.type_name.as_deref(), Some("Синхрон"));
    assert_eq!(t.long_name, "Чудове Чудовисько (повна назва)");
    assert_eq!(t.date_end.as_deref(), Some("2026-09-08T20:59:00+00:00"));
    assert_eq!(t.season_id, Some(61));
    assert_eq!(t.editors, vec![1]);
    assert_eq!(t.game_jury, vec![2]);
    assert_eq!(t.appeal_jury, vec![3]);
    assert_eq!(t.orgcommittee, vec![7420]);
    assert_eq!(
        t.last_edit_date.as_deref(),
        Some("2026-09-01T10:00:00+00:00")
    );
    assert_eq!(
        t.date_requests_allowed_to.as_deref(),
        Some("2026-09-08T20:59:00+00:00")
    );
    assert_eq!(
        t.date_appeal_allowed_to.as_deref(),
        Some("2026-09-10T20:59:00+00:00")
    );
    assert_eq!(t.regulations_url, "https://example.org/reg");
    assert_eq!(t.languages, vec!["uk"]);
    assert_eq!(t.rating_systems, vec!["mak"]);
    assert_eq!(t.archive, Some(false));
    assert_eq!(
        t.date_download_questions_from.as_deref(),
        Some("2026-08-29T15:00:00+00:00")
    );
    assert_eq!(
        t.hide_results_to.as_deref(),
        Some("2026-09-08T20:59:00+00:00")
    );
    assert_eq!(t.difficulty_forecast, Some(5.5));
    assert_eq!(t.questions_per_round, vec![12, 12, 12]);
}

#[tokio::test]
async fn search_tournaments_sends_the_filters_and_ordering() {
    let s = serve(|_| json(&format!("[{}]", TOURNAMENT_14015))).await;
    let q = TournamentQuery::new()
        .name("Чудове")
        .type_id(3)
        .archive(false)
        .date_start_after("2026-08-01")
        .date_start_before("2026-12-31")
        .last_edit_after("2026-07-01")
        .newest_first(true)
        .page(2)
        .limit(20);
    let found = client(&s).search_tournaments(&q).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, 14015);
    let path = s.requests()[0].path.clone();
    assert!(path.starts_with("/tournaments?"), "{}", path);
    for needle in [
        "name=%D0%A7%D1%83%D0%B4%D0%BE%D0%B2%D0%B5",
        "type=3",
        "archive=false",
        "dateStart%5Bafter%5D=2026-08-01",
        "dateStart%5Bbefore%5D=2026-12-31",
        "lastEditDate%5Bafter%5D=2026-07-01",
        "order%5Bid%5D=desc",
        "page=2",
        "itemsPerPage=20",
    ] {
        assert!(path.contains(needle), "{} missing in {}", needle, path);
    }
}

#[tokio::test]
async fn tournament_requests_are_walked_page_by_page() {
    let s = serve(|r| {
        assert!(r.path.starts_with("/tournaments/14015/requests?"), "{}", r.path);
        let rows = |n: usize, from: usize| {
            let v: Vec<String> = (from..from + n)
                .map(|i| format!(r#"{{"id":{},"status":"A","dateStart":"2026-09-02T17:00:00+00:00","tournamentId":14015,"representative":{{"id":1,"name":"a","surname":"b"}}}}"#, i))
                .collect();
            format!("[{}]", v.join(","))
        };
        if r.path.contains("page=1&") || r.path.ends_with("page=1") {
            json(&rows(100, 1))
        } else {
            json(&rows(5, 101))
        }
    })
    .await;
    let reqs = client(&s).get_tournament_requests(14015).await.unwrap();
    assert_eq!(reqs.len(), 105);
    assert_eq!(reqs[104].id, 105);
    assert_eq!(reqs[0].representative.as_ref().unwrap().id, 1);
    assert_eq!(s.requests().len(), 2);
}

#[tokio::test]
async fn synch_request_and_venue_requests_parse_the_nested_records() {
    let s = serve(|r| match r.path.as_str() {
        "/tournament_synch_requests/189504" => json(REQUEST_189504),
        p if p.starts_with("/venues/3360/requests") => json(&format!("[{}]", REQUEST_189504)),
        _ => not_found(),
    })
    .await;
    let c = client(&s);
    let req = c.get_synch_request(189504).await.unwrap();
    assert_eq!(req.id, 189504);
    assert_eq!(req.status, "A");
    assert_eq!(req.tournament_id, Some(14015));
    assert_eq!(req.venue.as_ref().unwrap().id, 3360);
    assert_eq!(req.venue.as_ref().unwrap().town_name(), "Краков");
    assert_eq!(req.representative.as_ref().unwrap().surname, "Михайлюк");
    assert!(req.narrator.is_none());
    assert_eq!(req.approximate_teams_count, Some(5));
    assert_eq!(req.issued_at.as_deref(), Some("2026-08-31T09:08:20+00:00"));
    assert_eq!(req.date_start.as_deref(), Some("2026-09-02T17:00:00+00:00"));
    let by_venue = c.get_venue_requests(3360).await.unwrap();
    assert_eq!(by_venue.len(), 1);
    assert_eq!(by_venue[0].id, 189504);
}

#[tokio::test]
async fn appeals_and_controversials_carry_their_full_records() {
    let s = serve(|r| match r.path.as_str() {
        "/tournaments/9700/appeals" => json(
            r#"[{"id": 13205, "idtournament": 9700, "type": "A", "issuedAt": "2024-02-08T19:49:29+00:00", "status": "A",
                 "appeal": "Прошу зачесть", "comment": "ответ зачтён", "answer": "неон", "questionNumber": 19, "overriddenById": null}]"#,
        ),
        "/tournament_synch_controversials/215378" => json(
            r#"{"id": 215378, "questionNumber": 26, "answer": "элементарные частицы", "issuedAt": "2024-01-27T13:20:19+00:00",
                "status": "D", "comment": null, "resolvedAt": null, "appealJuryComment": "no"}"#,
        ),
        p if p.starts_with("/tournaments/9700/results") => json(
            r#"[{"team":{"id":1,"name":"T"},"mask":"1","questionsTotal":1,"position":1,
                 "controversials":[{"id": 215378, "questionNumber": 26, "answer": "элементарные частицы", "status": "D"}]}]"#,
        ),
        _ => not_found(),
    })
    .await;
    let c = client(&s);
    let appeals = c.get_tournament_appeals(9700).await.unwrap();
    assert_eq!(appeals.len(), 1);
    assert_eq!(appeals[0].id, 13205);
    assert_eq!(appeals[0].kind, "A");
    assert_eq!(appeals[0].status, "A");
    assert_eq!(appeals[0].question_number, 19);
    assert_eq!(appeals[0].answer, "неон");
    assert_eq!(appeals[0].appeal, "Прошу зачесть");
    assert_eq!(appeals[0].comment.as_deref(), Some("ответ зачтён"));
    assert_eq!(appeals[0].tournament_id, 9700);
    assert_eq!(
        appeals[0].issued_at.as_deref(),
        Some("2024-02-08T19:49:29+00:00")
    );
    assert_eq!(appeals[0].overridden_by_id, None);
    let cv = c.get_controversial(215378).await.unwrap();
    assert_eq!(cv.id, Some(215378));
    assert_eq!(cv.question_number, 26);
    assert_eq!(cv.status, "D");
    assert_eq!(cv.answer, "элементарные частицы");
    assert_eq!(cv.appeal_jury_comment.as_deref(), Some("no"));
    assert_eq!(cv.issued_at.as_deref(), Some("2024-01-27T13:20:19+00:00"));
    assert_eq!(cv.comment, None);
    assert_eq!(cv.resolved_at, None);
    let rows = c.get_tournament_results(9700).await.unwrap();
    assert_eq!(rows[0].controversials[0].id, Some(215378));
    assert_eq!(rows[0].controversials[0].answer, "элементарные частицы");
}

#[tokio::test]
async fn team_and_player_histories() {
    let s = serve(|r| match r.path.as_str() {
        "/teams/86732/tournaments" => json(r#"[{"idteam": 86732, "idtournament": 7775}, {"idteam": 86732, "idtournament": 7730}]"#),
        "/players/127696/tournaments" => json(r#"[{"idplayer": 127696, "idteam": 45648, "idtournament": 3328}]"#),
        p if p.starts_with("/players/127696/seasons") => json(
            r#"[{"idplayer": 127696, "idseason": 50, "idteam": 51521, "dateAdded": "2017-02-17T00:00:00+00:00", "dateRemoved": null, "playerNumber": 0}]"#,
        ),
        p if p.starts_with("/teams/86732/seasons") => json(
            r#"[{"idplayer": 7496, "idseason": 57, "idteam": 86732, "dateAdded": "2023-07-21T00:00:00+00:00", "dateRemoved": null, "playerNumber": 0}]"#,
        ),
        _ => not_found(),
    })
    .await;
    let c = client(&s);
    assert_eq!(
        c.get_team_tournaments(86732)
            .await
            .unwrap()
            .iter()
            .map(|t| t.tournament_id)
            .collect::<Vec<_>>(),
        vec![7775, 7730]
    );
    let pt = c.get_player_tournaments(127696).await.unwrap();
    assert_eq!((pt[0].team_id, pt[0].tournament_id), (45648, 3328));
    let ps = c.get_player_seasons(127696).await.unwrap();
    assert_eq!((ps[0].season_id, ps[0].team_id), (50, 51521));
    assert_eq!(
        ps[0].date_added.as_deref(),
        Some("2017-02-17T00:00:00+00:00")
    );
    assert_eq!(ps[0].date_removed, None);
    let ts = c.get_team_seasons(86732).await.unwrap();
    assert_eq!((ts[0].player_id, ts[0].season_id), (7496, 57));
}

#[tokio::test]
async fn reference_lists_and_by_id_lookups() {
    let s = serve(|r| match r.path.as_str() {
        "/seasons" => json(r#"[{"id": 1, "dateStart": "2002-09-01T00:00:00+00:00", "dateEnd": "2003-08-31T00:00:00+00:00"}]"#),
        "/seasons/1" => json(r#"{"id": 1, "dateStart": "2002-09-01T00:00:00+00:00", "dateEnd": "2003-08-31T00:00:00+00:00"}"#),
        "/releases/1" => json(r#"{"id": 1, "date": "2003-07-01T00:00:00+00:00", "realDate": "2003-07-01T00:00:00+00:00", "lastRunRefresh": "2014-09-06T09:10:34+00:00"}"#),
        p if p.starts_with("/releases") => json(r#"[{"id": 1, "date": "2003-07-01T00:00:00+00:00", "realDate": "2003-07-01T00:00:00+00:00", "lastRunRefresh": "2014-09-06T09:10:34+00:00"}]"#),
        "/languages" => json(r#"[{"name": "Азербайджанский", "value": "az", "id": "az"}]"#),
        p if p.starts_with("/venue_types") => json(r#"[{"id": 1, "name": "Постоянная"}]"#),
        "/tournament_team_flags" => json(r#"[{"name": "COMMON", "value": 1, "id": 1, "fullName": "Общий зачёт", "shortName": "!"}]"#),
        p if p.starts_with("/regions") => json(r#"[{"id": 5, "name": "Московская область", "country": {"id": 1, "name": "Россия"}}]"#),
        "/towns/2088" => json(r#"{"id": 2088, "name": "Краков", "country": {"id": 35, "name": "Польша"}}"#),
        "/countries/35" => json(r#"{"id": 35, "name": "Польша"}"#),
        _ => not_found(),
    })
    .await;
    let c = client(&s);
    assert_eq!(
        c.list_seasons().await.unwrap()[0].date_end,
        "2003-08-31T00:00:00+00:00"
    );
    assert_eq!(c.get_season(1).await.unwrap().id, 1);
    let rel = c.list_releases().await.unwrap();
    assert_eq!(
        (rel[0].date.as_str(), rel[0].real_date.as_str()),
        ("2003-07-01T00:00:00+00:00", "2003-07-01T00:00:00+00:00")
    );
    assert_eq!(rel[0].last_run_refresh, "2014-09-06T09:10:34+00:00");
    assert_eq!(c.get_release(1).await.unwrap().id, 1);
    let langs = c.list_languages().await.unwrap();
    assert_eq!(
        (langs[0].id.as_str(), langs[0].name.as_str()),
        ("az", "Азербайджанский")
    );
    assert_eq!(c.list_venue_types().await.unwrap()[0].name, "Постоянная");
    let flags = c.list_team_flags().await.unwrap();
    assert_eq!(
        (
            flags[0].id,
            flags[0].short_name.as_str(),
            flags[0].full_name.as_str()
        ),
        (1, "!", "Общий зачёт")
    );
    assert_eq!((flags[0].name.as_str(), flags[0].value), ("COMMON", 1));
    let regions = c.list_regions().await.unwrap();
    assert_eq!((regions[0].id, regions[0].country.as_str()), (5, "Россия"));
    let town = c.get_town(2088).await.unwrap();
    assert_eq!((town.id, town.country.as_str()), (2088, "Польша"));
    assert_eq!(c.get_country(35).await.unwrap().name, "Польша");
}

#[tokio::test]
async fn results_query_sends_the_includes_and_parses_members_and_flags() {
    let s = serve(|_| {
        json(
            r#"[{"team": {"id": 107740, "name": "Ладно", "town": {"id": 2088, "name": "Краков"}},
                 "current": {"name": "Ладно (now)", "town": {"id": 2088, "name": "Краков"}},
                 "questionsTotal": null, "synchRequest": {"id": 189504, "venue": {"id": 3360, "name": "Краков"}, "tournamentId": 14015},
                 "position": null, "flags": ["Школ"],
                 "teamMembers": [{"flag": "К", "rating": 5251, "player": {"id": 258605, "name": "Владислав", "patronymic": "Александрович", "surname": "Рябцев"}}]}]"#,
        )
    })
    .await;
    let q = ResultsQuery::new()
        .team_members(true)
        .team_flags(true)
        .masks_and_controversials(false)
        .venue(3360)
        .town(2088)
        .region(7)
        .country(35)
        .flag(3);
    let rows = client(&s)
        .get_tournament_results_with(14015, &q)
        .await
        .unwrap();
    let path = s.requests()[0].path.clone();
    for needle in [
        "includeTeamMembers=1",
        "includeTeamFlags=1",
        "includeMasksAndControversials=0",
        "venue=3360",
        "town=2088",
        "region=7",
        "country=35",
        "flag=3",
    ] {
        assert!(path.contains(needle), "{} missing in {}", needle, path);
    }
    assert_eq!(rows[0].current_name.as_deref(), Some("Ладно (now)"));
    assert_eq!(rows[0].flags, vec!["Школ"]);
    assert_eq!(rows[0].team_members.len(), 1);
    assert_eq!(rows[0].team_members[0].player.id, 258605);
    assert_eq!(rows[0].team_members[0].flag.as_deref(), Some("К"));
    assert_eq!(
        rows[0].team_members[0].roster_flag(),
        Some(chgk_rating::csv::RosterFlag::Captain)
    );
    assert_eq!(rows[0].team_members[0].rating, Some(5251));
    assert_eq!(rows[0].venue_id, Some(3360));
    assert_eq!(rows[0].synch_request_id, Some(189504));
}

#[tokio::test]
async fn intersections_are_tournaments() {
    let s = serve(|_| json(&format!("[{}]", TOURNAMENT_14015))).await;
    let t = client(&s)
        .get_tournament_intersections(14015)
        .await
        .unwrap();
    assert_eq!(t[0].id, 14015);
}
