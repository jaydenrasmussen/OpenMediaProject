use axum::{Router, routing::get};
use omp_metadata::scene::{HttpSceneSource, SceneSource};

async fn serve() -> String {
    let app = Router::new()
        .route(
            "/mapping",
            get(|| async {
                r#"[{"mappingId":"a","tvdbId":77,"title":"Show: Alt","searchTitle":"Show Alt","season":-1},
                    {"mappingId":"b","tvdbId":77,"title":"Show S2","searchTitle":"Show S2","season":2}]"#
            }),
        )
        .route("/xem/map/havemap", get(|| async { r#"{"result":"success","data":["77","78"]}"# }))
        .route(
            "/xem/map/all",
            get(|| async {
                r#"{"result":"success","data":[
                  {"scene":{"season":1,"episode":1,"absolute":1},"tvdb":{"season":1,"episode":1,"absolute":1}},
                  {"scene":{"season":2,"episode":1,"absolute":0},"tvdb":{"season":1,"episode":13,"absolute":13}},
                  {"scene":{"season":2,"episode":2,"absolute":14}}]}"#
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    base
}

#[tokio::test]
async fn reads_titles_and_xem_numbering() {
    let base = serve().await;
    let src = HttpSceneSource::new().with_urls(format!("{base}/xem"), format!("{base}/mapping"));

    let titles = src.scene_titles().await.unwrap();
    assert_eq!(titles.len(), 2);
    assert_eq!(
        (
            titles[0].tvdb_id,
            titles[0].season,
            titles[0].search_title.as_str()
        ),
        (77, -1, "Show Alt")
    );
    assert_eq!(titles[1].season, 2);

    assert_eq!(
        src.xem_known_ids().await.unwrap(),
        [77, 78].into_iter().collect()
    );

    let map = src.xem_map(77).await.unwrap();
    assert_eq!(map.len(), 2, "an entry without a tvdb side is skipped");
    assert_eq!((map[1].tvdb.season, map[1].tvdb.episode), (1, 13));
    assert_eq!((map[1].scene.season, map[1].scene.episode), (2, 1));
    assert_eq!(map[1].scene.absolute, None, "absolute 0 means none");
}
