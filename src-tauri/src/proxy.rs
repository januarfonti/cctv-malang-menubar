use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use reqwest::Client;
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

const STREAM_BASE: &str = "https://cctv.malangkota.go.id";
// Visit this page to obtain the session cookies required by the stream server
const SITE_PAGE: &str = "https://cctv.malangkota.go.id/sebaran-cctv";
const CAMERA_API_HOSTNAME: &str = "api.cctv.malangkota.go.id";
const CAMERA_API_IP: &str = "36.94.95.188";
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";
// Refresh session cookies every 2 hours; the upstream issues cookies with ~3 hour expiry
const COOKIE_TTL_SECS: u64 = 7200;

static STATIC_CAMERAS: &str = include_str!("data/cameras.json");

// --- Shared proxy state ---

struct ProxyState {
    client: Client,
    cookies_fetched_at: Instant,
}

type SharedState = Arc<RwLock<ProxyState>>;

/// Build a fresh reqwest Client with an active session cookie jar.
/// Visits the CCTV site to obtain the session cookies required for stream access.
async fn build_session_client() -> Client {
    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .use_rustls_tls()
        .cookie_store(true)
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to build HTTP client");

    if let Err(e) = client
        .get(SITE_PAGE)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
    {
        eprintln!("Session cookie warm-up failed: {e}");
    }

    client
}

// --- Stream proxy ---

async fn proxy_stream(State(state): State<SharedState>, Path(path): Path<String>) -> Response {
    let url = format!("{STREAM_BASE}/cctv-stream/{path}");

    // Periodic cookie refresh
    let needs_refresh =
        state.read().await.cookies_fetched_at.elapsed() > Duration::from_secs(COOKIE_TTL_SECS);
    if needs_refresh {
        let new_client = build_session_client().await;
        let mut w = state.write().await;
        w.client = new_client;
        w.cookies_fetched_at = Instant::now();
    }

    // Clone is cheap — Client is Arc-backed and shares the cookie jar
    let client = state.read().await.client.clone();

    match send_stream_request(&client, &url).await {
        // On 403 the session cookie has expired — refresh and retry once
        Ok(r) if r.status() == 403 => {
            eprintln!("Stream 403 on {path} — refreshing session cookies");
            let new_client = build_session_client().await;
            {
                let mut w = state.write().await;
                w.client = new_client.clone();
                w.cookies_fetched_at = Instant::now();
            }
            match send_stream_request(&new_client, &url).await {
                Ok(r) => make_stream_response(r, &path).await,
                Err(e) => {
                    eprintln!("Stream retry error: {e}");
                    (StatusCode::BAD_GATEWAY, "Error proxying stream").into_response()
                }
            }
        }
        Ok(r) => make_stream_response(r, &path).await,
        Err(e) => {
            eprintln!("Stream proxy error: {e}");
            (StatusCode::BAD_GATEWAY, "Error proxying stream").into_response()
        }
    }
}

async fn send_stream_request(client: &Client, url: &str) -> reqwest::Result<reqwest::Response> {
    client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Referer", SITE_PAGE)
        .send()
        .await
}

async fn make_stream_response(resp: reqwest::Response, path: &str) -> Response {
    let status = resp.status();
    if status.is_client_error() || status.is_server_error() {
        return (
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            format!("Stream error: {}", status),
        )
            .into_response();
    }

    let upstream_ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let content_type = upstream_ct.unwrap_or_else(|| {
        if path.ends_with(".m3u8") {
            "application/vnd.apple.mpegurl".into()
        } else if path.ends_with(".ts") {
            "video/mp2t".into()
        } else {
            "application/octet-stream".into()
        }
    });

    let cache_control = if path.ends_with(".m3u8") {
        "no-cache, no-store, must-revalidate"
    } else {
        "public, max-age=2"
    };

    let body = resp.bytes().await.unwrap_or_default();
    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_str(&content_type).unwrap(),
    );
    headers.insert(
        "Cache-Control",
        HeaderValue::from_str(cache_control).unwrap(),
    );

    (StatusCode::OK, headers, Body::from(body)).into_response()
}

// --- Camera API proxy ---

async fn proxy_cameras() -> Response {
    if let Ok(body) =
        fetch_cameras_from("http://api.cctv.malangkota.go.id/records/cameras", None).await
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
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
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
        .timeout(Duration::from_secs(10))
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
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    (StatusCode::OK, headers, Body::from(body.to_string())).into_response()
}

// --- Server setup ---

pub async fn start_proxy_server() -> u16 {
    let state: SharedState = Arc::new(RwLock::new(ProxyState {
        client: build_session_client().await,
        cookies_fetched_at: Instant::now(),
    }));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/stream/*path", get(proxy_stream))
        .route("/cameras", get(proxy_cameras))
        .layer(cors)
        .with_state(state);

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
