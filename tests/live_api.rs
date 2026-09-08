//! Read-only checks against the real api.rating.chgk.info. Ignored by
//! default (network); run with `cargo test --test live_api -- --ignored`.
//! Fixtures are real, stable public records.

use chgk_rating::{api, ApiClient};

fn client() -> ApiClient {
    ApiClient::new().unwrap()
}

#[tokio::test]
#[ignore]
async fn team_town_country_and_quoted_name() {
    let t = client().get_team(107740).await.unwrap();
    assert_eq!(t.id, 107740);
    // The registered name has carried literal quote characters at times;
    // don't pin that.
    assert!(t.name.contains("Ладно, погнали"), "{}", t.name);
    assert_eq!(t.town, "Краков");
    assert_eq!(t.country, "Польша");
}

#[tokio::test]
#[ignore]
async fn find_teams_accepts_id_and_name() {
    let by_id = client().find_teams("86732", 5).await.unwrap();
    assert_eq!(by_id.len(), 1);
    assert_eq!(by_id[0].name, "4:20");
    let by_name = client()
        .search_teams("Polish Space Marines", 5)
        .await
        .unwrap();
    assert!(by_name.iter().any(|t| t.id == 77174));
}

#[tokio::test]
#[ignore]
async fn player_and_search() {
    let p = client().get_player(127696).await.unwrap();
    assert_eq!(p.surname, "Михайлюк");
    let found = client().find_players("Михайлюк Роберт", 10).await.unwrap();
    assert!(found.iter().any(|f| f.id == 127696));
}

#[tokio::test]
#[ignore]
async fn base_roster_of_a_real_team() {
    let roster = client().get_base_roster(86108).await.unwrap();
    assert!(!roster.is_empty(), "Mafia has a base roster");
}

#[tokio::test]
#[ignore]
async fn towns_countries_venues() {
    let c = client();
    let id = c.lookup_town_id("Краков").await.unwrap();
    assert_eq!(id, Some(2088));
    let towns = c.search_towns("Краков", 5).await.unwrap();
    assert!(towns.iter().any(|t| t.id == 2088 && t.country == "Польша"));
    // The API's name filter is a case-insensitive substring match; the exact
    // match is picked on our side.
    let country = c.lookup_country("польша").await.unwrap().unwrap();
    assert_eq!((country.id, country.name.as_str()), (35, "Польша"));
    let v = c.get_venue(3360).await.unwrap();
    assert_eq!(v.name, "Краков");
    assert_eq!(v.town_name(), "Краков");
    assert_eq!(v.display(), "Краков (Краков, Польша)");
    assert_eq!(c.get_venue(3053).await.unwrap().town_name(), "Берлин");
    let found = c.search_venues("Краков", 5).await.unwrap();
    assert!(found.iter().any(|f| f.id == 3360));
}

#[tokio::test]
#[ignore]
async fn tournament_rounds_results_and_controversials() {
    let c = client();
    let t = c.get_tournament(14015).await.unwrap();
    assert_eq!(t.name, "Чудове Чудовисько");
    assert_eq!(t.questions_per_round, vec![12, 12, 12]);
    assert!(t.orgcommittee.contains(&7420));
    assert_eq!(t.date_start.as_deref(), Some("2026-08-29T12:00:00+00:00"));

    let rows = c.get_tournament_results(14015).await.unwrap();
    let krakow: Vec<_> = rows.iter().filter(|r| r.venue_id == Some(3360)).collect();
    assert!(
        krakow.iter().any(|r| r.team_id == 86732),
        "4:20 played in Krakow"
    );

    // Must not fail on tournaments without controversials.
    let _ = c.get_tournament_controversials(14015).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn venue_synch_list() {
    let list = client()
        .list_synch_tournaments_for_venue(3360)
        .await
        .unwrap();
    assert!(!list.is_empty());
    let t = list
        .iter()
        .find(|t| t.id == 14015)
        .expect("T14015 in Krakow's list");
    assert_eq!(t.status.as_deref(), Some("A"));
    assert_eq!(t.representative_id, Some(127696));
    assert_eq!(t.questions_per_round, vec![12, 12, 12]);
    // Newest first.
    assert!(list.windows(2).all(|w| w[0].date >= w[1].date));
}

#[tokio::test]
#[ignore]
async fn endpoints_added_after_the_spec_audit() {
    let c = client();
    let t = c.get_tournament(14015).await.unwrap();
    assert_eq!(t.type_id, Some(3));
    assert!(t.is_synchronous());
    assert_eq!(t.season_id, Some(61));
    assert!(t.date_end.is_some());

    let q = api::TournamentQuery::new()
        .name("Чудове")
        .type_id(3)
        .newest_first(true)
        .limit(5);
    let found = c.search_tournaments(&q).await.unwrap();
    assert!(found.iter().any(|t| t.id == 14015), "{:?}", found);
    assert!(found.windows(2).all(|w| w[0].id > w[1].id), "newest first");

    let req = c.get_synch_request(189504).await.unwrap();
    assert_eq!(req.tournament_id, Some(14015));
    assert_eq!(req.venue.as_ref().map(|v| v.id), Some(3360));
    assert_eq!(req.representative.as_ref().map(|p| p.id), Some(127696));
    let reqs = c.get_tournament_requests(14015).await.unwrap();
    assert!(reqs.iter().any(|r| r.id == 189504));

    let appeals = c.get_tournament_appeals(9700).await.unwrap();
    assert!(appeals
        .iter()
        .any(|a| a.id == 13205 && a.question_number == 19));
    let cv = c.get_controversial(215378).await.unwrap();
    assert_eq!(cv.question_number, 26);
    assert_eq!(cv.answer, "элементарные частицы");

    assert!(c.get_team_tournaments(86732).await.unwrap().len() > 100);
    assert!(c
        .get_player_tournaments(127696)
        .await
        .unwrap()
        .iter()
        .any(|t| t.tournament_id == 3328));
    assert!(c
        .get_player_seasons(127696)
        .await
        .unwrap()
        .iter()
        .any(|s| s.season_id == 50));

    let seasons = c.list_seasons().await.unwrap();
    assert!(seasons.iter().any(|s| s.id == 61));
    assert!(c.list_releases().await.unwrap().len() > 100);
    assert!(c
        .list_languages()
        .await
        .unwrap()
        .iter()
        .any(|l| l.id == "ru"));
    assert!(c
        .list_venue_types()
        .await
        .unwrap()
        .iter()
        .any(|v| v.id == 1));
    assert!(c
        .list_team_flags()
        .await
        .unwrap()
        .iter()
        .any(|f| f.name == "SCHOOL"));
    assert_eq!(c.get_town(2088).await.unwrap().country, "Польша");
    assert_eq!(c.get_country(35).await.unwrap().name, "Польша");

    let rows = c
        .get_tournament_results_with(
            14015,
            &api::ResultsQuery::new().team_members(true).venue(3360),
        )
        .await
        .unwrap();
    assert!(rows.iter().all(|r| r.venue_id == Some(3360)));
    assert!(rows
        .iter()
        .any(|r| r.team_id == 107740 && !r.team_members.is_empty()));
}
