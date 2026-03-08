use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
    routing::{delete, get, post},
    Json, Router,
};
use dotenvy::dotenv;
use nanoid::nanoid;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::env;
use tower_http::services::ServeDir;

// ── Database model ──────────────────────────────────────────────────
#[derive(Serialize, sqlx::FromRow)]
struct Link {
    id: i32,
    code: String,
    url: String,
    clicks: i32,
    created_at: chrono::DateTime<chrono::Utc>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    max_clicks: Option<i32>,
}

// ── Request/response types ───────────────────────────────────────────
#[derive(Deserialize)]
struct ShortenRequest {
    url: String,
    code: Option<String>,
    expires_in_minutes: Option<i64>,
    max_clicks: Option<i32>,
}

#[derive(Serialize)]
struct ShortenResponse {
    short_url: String,
    code: String,
}

// ── Shared app state ─────────────────────────────────────────────────
#[derive(Clone)]
struct AppState {
    db: PgPool,
    base_url: String,
    admin_token: String,
    banned_words: Vec<String>,
}

// ── Main ─────────────────────────────────────────────────────────────
#[tokio::main]
async fn main() {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let base_url = env::var("BASE_URL").expect("BASE_URL must be set");
    let admin_token = env::var("ADMIN_TOKEN").expect("ADMIN_TOKEN must be set");

    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    let banned_words = std::fs::read_to_string("banned_words.txt")
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
        .collect::<Vec<String>>();

    let state = AppState { db, base_url, admin_token, banned_words };
    let app = Router::new()
        .route("/", get(index))
        .route("/:code", get(redirect))
        .route("/api/shorten", post(shorten))
        .route("/admin", get(admin_page))
        .route("/admin/links", get(list_links))
        .route("/admin/links/:id", delete(delete_link))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    tracing::info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// ── Handlers ─────────────────────────────────────────────────────────

async fn index(State(state): State<AppState>) -> Html<String> {
    Html(format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>seraph / s</title>
  <link rel="icon" type="image/png" href="/favicon.png">
  <link rel="apple-touch-icon" href="/apple-touch-icon.png">
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/hack-font@3/build/web/hack.css">
  <link rel="stylesheet" href="/static/index.css">
  <script>window.BASE_URL = "{base_url}";</script>
</head>
<body>
  <header>
    <a class="logo" href="https://seraph.ws">seraph</a>
    <div class="barcode"></div>
  </header>
  <main>
    <h1>s.seraph.ws</h1>
    <p class="prompt">personal link shortener</p>
    <div class="shorten-form">
      <input id="url" type="url" placeholder="https://example.com" autocomplete="off" />
      <input id="code" type="text" placeholder="custom code (optional)" autocomplete="off" />
      <div style="display:flex;gap:0.5rem;align-items:center">
        <input id="expires-value" type="number" placeholder="expires after..." autocomplete="off" min="1" step="1" style="width:160px" />
        <select id="expires-unit">
          <option value="1">minutes</option>
          <option value="60" selected>hours</option>
          <option value="1440">days</option>
        </select>
      </div>
      <input id="max-clicks" type="number" placeholder="max clicks (optional)" autocomplete="off" min="1" style="width:180px" />
      <button onclick="shorten()">shorten</button>
    </div>
    <div id="result"></div>
    <p><a href="https://seraph.ws">← back to seraph.ws</a></p>
  </main>
  <footer><a href="https://seraph.ws">seraph.ws</a></footer>
  <script src="/static/index.js"></script>
  <script async src="https://analytics.seraph.ws/js/pa-p8pHwjggKya5Vyjukouh3.js"></script>
</body>
</html>"#, base_url = state.base_url))
}

async fn redirect(
    Path(code): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let result = sqlx::query_as::<_, Link>(
        "SELECT * FROM links WHERE code = $1"
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(link)) => {
            // Check time expiry
            if let Some(expires_at) = link.expires_at {
                if chrono::Utc::now() > expires_at {
                    return expired_page().into_response();
                }
            }
            // Check click limit
            if let Some(max) = link.max_clicks {
                if link.clicks >= max {
                    return expired_page().into_response();
                }
            }
            // Increment clicks and redirect
            let _ = sqlx::query(
                "UPDATE links SET clicks = clicks + 1 WHERE code = $1"
            )
            .bind(&code)
            .execute(&state.db)
            .await;

            Redirect::permanent(&link.url).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, not_found_page()).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    }
}

fn expired_page() -> Html<&'static str> {
    Html(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>seraph / link expired</title>
  <link rel="icon" type="image/png" href="/favicon.png">
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/hack-font@3/build/web/hack.css">
  <link rel="stylesheet" href="/static/error.css">
</head>
<body>
  <header>
    <a class="logo" href="https://seraph.ws">seraph</a>
    <div class="barcode"></div>
  </header>
  <main>
    <h1>link expired</h1>
    <p class="prompt">this link is no longer active</p>
    <p><a href="https://s.seraph.ws">← shorten a new link</a></p>
  </main>
</body>
</html>"#)
}

fn not_found_page() -> Html<&'static str> {
    Html(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>seraph / not found</title>
  <link rel="icon" type="image/png" href="/favicon.png">
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/hack-font@3/build/web/hack.css">
  <link rel="stylesheet" href="/static/error.css">
</head>
<body>
  <header>
    <a class="logo" href="https://seraph.ws">seraph</a>
    <div class="barcode"></div>
  </header>
  <main>
    <h1>404</h1>
    <p class="prompt">that link doesn't exist</p>
    <p><a href="https://s.seraph.ws">← shorten a new link</a></p>
  </main>
</body>
</html>"#)
}


async fn resolve_url(url: &str) -> Result<String, &'static str> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|_| "Failed to build HTTP client")?;

    // If http, try upgrading to https first
    let upgraded = if url.starts_with("http://") {
        let https_url = url.replacen("http://", "https://", 1);
        match client.head(&https_url).send().await {
            Ok(res) if res.status().is_success() || res.status().is_redirection() => https_url,
            _ => url.to_string(),
        }
    } else {
        url.to_string()
    };

    // Verify the URL actually resolves
    match client.head(&upgraded).send().await {
        Ok(res) if res.status().as_u16() < 500 => Ok(upgraded),
        Ok(_) => Err("URL returned a server error"),
        Err(_) => Err("URL could not be reached"),
    }
}

async fn shorten(
    State(state): State<AppState>,
    Json(payload): Json<ShortenRequest>,
) -> impl IntoResponse {
    // Validate URL
    let raw_url = {
        let trimmed = payload.url.trim();
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            format!("https://{}", trimmed)
        } else {
            trimmed.to_string()
        }
    };
        let url = match resolve_url(&raw_url).await {
        Ok(u) => u,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };

    if url.len() > 2048 {
        return (StatusCode::BAD_REQUEST, "URL too long").into_response();
    }

    // Validate/generate code
    let code = match payload.code {
        Some(c) if !c.is_empty() => {
            let c = c.trim().to_string();
            if c.len() > 32 {
                return (StatusCode::BAD_REQUEST, "Custom code too long").into_response();
            }
            if !c.chars().all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_') {
                return (StatusCode::BAD_REQUEST, "Code may only contain letters, numbers, hyphens, underscores").into_response();
            }
            c
        }
        _ => nanoid!(6),
    };
    
    // Check against banned words
    let code_lower = code.to_lowercase();
    if state.banned_words.iter().any(|w| code_lower.contains(w.as_str())) {
        return (StatusCode::BAD_REQUEST, "That code is not allowed").into_response();
    }

    // Return existing link if URL already shortened
    // Only dedup if this is a permanent link — expiring links always get their own entry
    if payload.expires_in_minutes.is_none() && payload.max_clicks.is_none() {
        let existing = sqlx::query_as::<_, Link>(
            "SELECT * FROM links WHERE url = $1
             AND expires_at IS NULL AND max_clicks IS NULL
             ORDER BY created_at ASC LIMIT 1"
        )
        .bind(&url)
        .fetch_optional(&state.db)
        .await;

        if let Ok(Some(link)) = existing {
            return Json(ShortenResponse {
                short_url: format!("{}/{}", state.base_url, link.code),
                code: link.code,
            }).into_response();
        }
    }

    let expires_at = payload.expires_in_minutes.map(|m| {
        let clamped = m.max(1).min(10080); // 1 min minimum, 1 week maximum
        chrono::Utc::now() + chrono::Duration::minutes(clamped)
    });

    let result = sqlx::query(
        "INSERT INTO links (code, url, expires_at, max_clicks) VALUES ($1, $2, $3, $4)"
    )
    .bind(&code)
    .bind(&url)
    .bind(&expires_at)
    .bind(&payload.max_clicks)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => Json(ShortenResponse {
            short_url: format!("{}/{}", state.base_url, code),
            code,
        }).into_response(),
        Err(e) if e.to_string().contains("unique") => {
            (StatusCode::CONFLICT, "Code already taken").into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    }
}

async fn admin_page() -> Html<&'static str> {
    Html(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>seraph / admin</title>
  <link rel="icon" type="image/png" href="/favicon.png">
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/hack-font@3/build/web/hack.css">
  <link rel="stylesheet" href="/static/admin.css">
</head>
<body>
  <h1>seraph / admin</h1>
  <div class="form">
    <input id="url" placeholder="https://example.com" />
    <input id="code" placeholder="custom code (optional)" style="width:180px" />
    <button onclick="shorten()">shorten</button>
  </div>
  <div id="result"></div>
  <h2>all links</h2>
  <table id="links-table"></table>
  <script src="/static/admin.js"></script>
  <script async src="https://analytics.seraph.ws/js/pa-p8pHwjggKya5Vyjukouh3.js"></script>
</body>
</html>"#)
}

async fn list_links(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_authorized(&headers, &state.admin_token) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let links = sqlx::query_as::<_, Link>("SELECT * FROM links ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await;

    match links {
        Ok(links) => Json(links).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    }
}

async fn delete_link(
    Path(id): Path<i32>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_authorized(&headers, &state.admin_token) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let result = sqlx::query("DELETE FROM links WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => (StatusCode::OK, "Deleted").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

fn is_authorized(headers: &HeaderMap, admin_token: &str) -> bool {
    headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == format!("Bearer {}", admin_token))
        .unwrap_or(false)
}
