//! The VPN settings page: saved for the container's entrypoint, secrets kept
//! out of every answer.

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, router};
use omp_db::Db;
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use serde_json::{Value, json};
use tower::ServiceExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &Path) -> Option<ProbeInfo> {
        None
    }
}

async fn call(
    app: &Arc<App>,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let key = app.api_key().await.unwrap();
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("x-api-key", key);
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let res = router(app.clone())
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn the_vpn_is_set_up_from_the_page_and_its_secrets_stay_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let app = App::new(
        AppConfig::new(data.clone()),
        Db::in_memory().await.unwrap(),
        None,
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();

    let (s, v) = call(&app, Method::GET, "/api/v1/system/vpn/settings", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["settings"]["provider"], "");
    assert!(
        v["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "mullvad")
    );

    let (s, v) = call(
        &app,
        Method::PUT,
        "/api/v1/system/vpn/settings",
        Some(json!({"provider": "nordvpn", "username": "svc-user", "password": "svc-pass", "server_countries": "Netherlands"})),
    )
    .await;
    assert_eq!((s, &v["restart_required"]), (StatusCode::OK, &json!(true)));

    let file = std::fs::read_to_string(data.join("vpn.env")).unwrap();
    assert!(
        file.contains("VPN_SERVICE_PROVIDER=nordvpn") && file.contains("OPENVPN_PASSWORD=svc-pass")
    );

    let (_, v) = call(&app, Method::GET, "/api/v1/system/vpn/settings", None).await;
    assert_eq!(v["settings"]["username"], "svc-user");
    assert_eq!(v["password_set"], true);
    assert!(
        !v.to_string().contains("svc-pass"),
        "the password must not be sent back"
    );

    // Saving again with the password left blank keeps it.
    call(
        &app,
        Method::PUT,
        "/api/v1/system/vpn/settings",
        Some(json!({"provider": "nordvpn", "username": "svc-user", "server_countries": "Norway"})),
    )
    .await;
    assert!(
        std::fs::read_to_string(data.join("vpn.env"))
            .unwrap()
            .contains("OPENVPN_PASSWORD=svc-pass")
    );

    let (s, _) = call(
        &app,
        Method::PUT,
        "/api/v1/system/vpn/settings",
        Some(json!({"provider": "nosuchvpn"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    call(
        &app,
        Method::PUT,
        "/api/v1/system/vpn/settings",
        Some(json!({"provider": ""})),
    )
    .await;
    assert!(!data.join("vpn.env").exists());
}
