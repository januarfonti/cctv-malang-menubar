use axum::{
    body::Body,
    extract::Path,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use reqwest::Client;
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};

const STREAM_HOST_IP: &str = "103.135.14.67";
const STREAM_HOSTNAME: &str = "cctv.malangkota.go.id";
const CAMERA_API_HOSTNAME: &str = "api.cctv.malangkota.go.id";
const CAMERA_API_IP: &str = "36.94.95.188";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

static STATIC_CAMERAS: &str = include_str!("data/cameras.json");

fn build_client() -> Client {
    Client::builder()
        .danger_accept_invalid_certs(true)
        .use_rustls_tls()
        .build()
        .expect("Failed to build HTTP client")
}

async fn proxy_stream(Path(path): Path<String>) -> Response {
    let client = build_client();
    let url = format!("https://{STREAM_HOST_IP}/cctv-stream/{path}");

    let result = client
        .get(&url)
        .header("Host", STREAM_HOSTNAME)
        .header("Referer", format!("https://{STREAM_HOSTNAME}/"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await;

    match result {
        Ok(resp) => {
            let status = resp.status();
            if status.is_client_error() || status.is_server_error() {
                return (
                    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                    format!("Stream error: {}", status),
                )
                    .into_response();
            }

            let upstream_content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            let content_type = upstream_content_type.unwrap_or_else(|| {
                if path.ends_with(".m3u8") {
                    "application/vnd.apple.mpegurl".to_string()
                } else if path.ends_with(".ts") {
                    "video/mp2t".to_string()
                } else {
                    "application/octet-stream".to_string()
                }
            });

            let cache_control = if path.ends_with(".m3u8") {
                "no-cache, no-store, must-revalidate"
            } else {
                "public, max-age=2"
            };

            let body = resp.bytes().await.unwrap_or_default();

            let mut headers = HeaderMap::new();
            headers.insert("Content-Type", HeaderValue::from_str(&content_type).unwrap());
            headers.insert(
                "Cache-Control",
                HeaderValue::from_str(cache_control).unwrap(),
            );

            (StatusCode::OK, headers, Body::from(body)).into_response()
        }
        Err(e) => {
            eprintln!("Stream proxy error: {e}");
            (StatusCode::BAD_GATEWAY, "Error proxying stream").into_response()
        }
    }
}

async fn proxy_cameras() -> Response {
    if let Ok(body) = fetch_cameras_from("http://api.cctv.malangkota.go.id/records/cameras", None)
        .await
    {
        return json_response(&body);
    }

    if let Ok(body) = fetch_cameras_from(
        &format!("http://{CAMERA_API_IP}/records/cameras"),
        Some(CAMERA_API_HOSTNAME),
    )
    .await
    {
        return json_response(&body);
    }

    eprintln!("Camera API: serving static fallback data");
    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/json"),
    );
    (StatusCode::OK, headers, Body::from(STATIC_CAMERAS)).into_response()
}

async fn fetch_cameras_from(url: &str, host_override: Option<&str>) -> Result<String, String> {
    let client = Client::builder()
        .build()
        .map_err(|e| format!("Client build error: {e}"))?;

    let mut req = client
        .get(url)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT);

    if let Some(host) = host_override {
        req = req.header("Host", host);
    }

    let resp = req
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Fetch error: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP error: {}", resp.status()));
    }

    let text = resp.text().await.map_err(|e| format!("Read error: {e}"))?;

    if text.starts_with('<') {
        return Err("Upstream returned HTML instead of JSON".to_string());
    }

    Ok(text)
}

fn json_response(body: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/json"),
    );
    (StatusCode::OK, headers, Body::from(body.to_string())).into_response()
}

pub async fn start_proxy_server() -> u16 {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/stream/*path", get(proxy_stream))
        .route("/cameras", get(proxy_cameras))
        .layer(cors);

    let addr = SocketAddr::from(([127, 0, 0, 1], 9877));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind proxy server");

    let port = listener.local_addr().unwrap().port();
    println!("Menubar proxy server listening on http://127.0.0.1:{port}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    port
}
