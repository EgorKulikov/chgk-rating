//! Read-only checks against the real api.rating.chgk.info. Ignored by
//! default (network); run with `cargo test --test live_api -- --ignored`.
//! Fixtures are real, stable public records.

use chgk_rating::api;

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(chgk_rating::USER_AGENT)
        .build()
        .unwrap()
}

#[tokio::test]
#[ignore]
async fn team_town_country_and_quoted_name() {
    let t = api::get_team(&client(), 107740).await.unwrap();
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
    let by_id = api::find_teams(&client(), "86732", 5).await.unwrap();
    assert_eq!(by_id.len(), 1);
    assert_eq!(by_id[0].name, "4:20");
    let by_name = api::search_teams(&client(), "Polish Space Marines", 5).await.unwrap();
    assert!(by_name.iter().any(|t| t.id == 77174));
}

#[tokio::test]
#[ignore]
async fn player_and_search() {
    let p = api::get_player(&client(), 127696).await.unwrap();
    assert_eq!(p.surname, "Михайлюк");
    let found = api::find_players(&client(), "Михайлюк Роберт", 10).await.unwrap();
    assert!(found.iter().any(|f| f.id == 127696));
}

#[tokio::test]
#[ignore]
async fn base_roster_of_a_real_team() {
    let roster = api::get_base_roster(&client(), 86108).await.unwrap();
    assert!(!roster.is_empty(), "Mafia has a base roster");
}

#[tokio::test]
#[ignore]
async fn towns_countries_venues() {
    let c = client();
    let id = api::lookup_town_id(&c, "Краков").await.unwrap();
    assert_eq!(id, Some(2088));
    let towns = api::search_towns(&c, "Краков", 5).await.unwrap();
    assert!(towns.iter().any(|t| t.id == 2088 && t.country == "Польша"));
    // The API's name filter is case-sensitive; the match on our side is not.
    assert_eq!(
        api::get_country_by_name(&c, "Польша").await.unwrap().as_deref(),
        Some("Польша")
    );
    let v = api::get_venue(&c, 3360).await.unwrap();
    assert_eq!(v.name, "Краков");
    assert_eq!(v.town_name(), "Краков");
    assert_eq!(v.display(), "Краков (Краков, Польша)");
    assert_eq!(api::get_venue_town(&c, 3053).await.unwrap(), "Берлин");
    let found = api::search_venues(&c, "Краков", 5).await.unwrap();
    assert!(found.iter().any(|f| f.id == 3360));
}

#[tokio::test]
#[ignore]
async fn tournament_rounds_results_and_controversials() {
    let c = client();
    let t = api::get_tournament(&c, 14015).await.unwrap();
    assert_eq!(t.name, "Чудове Чудовисько");
    assert_eq!(t.questions_per_round, vec![12, 12, 12]);
    assert!(t.orgcommittee.contains(&7420));
    assert_eq!(t.date_start.as_deref(), Some("2026-08-29T12:00:00+00:00"));

    let rows = api::get_tournament_results(&c, 14015).await.unwrap();
    let krakow: Vec<_> = rows.iter().filter(|r| r.venue_id == Some(3360)).collect();
    assert!(krakow.iter().any(|r| r.team_id == 86732), "4:20 played in Krakow");

    // Must not fail on tournaments without controversials.
    let _ = api::get_tournament_controversials(&c, 14015).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn venue_synch_list() {
    let list = api::list_synch_tournaments_for_venue(&client(), 3360).await.unwrap();
    assert!(!list.is_empty());
    let t = list.iter().find(|t| t.id == 14015).expect("T14015 in Krakow's list");
    assert_eq!(t.status.as_deref(), Some("A"));
    assert_eq!(t.representative_id, Some(127696));
    assert_eq!(t.questions_per_round, vec![12, 12, 12]);
    // Newest first.
    assert!(list.windows(2).all(|w| w[0].date >= w[1].date));
}
