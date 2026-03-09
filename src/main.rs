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
use bcrypt::{hash, verify as bcrypt_verify, DEFAULT_COST};
use woothee::parser::Parser as UaParser;

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
    password_hash: Option<String>,
}

// ── Request/response types ───────────────────────────────────────────
#[derive(Deserialize)]
struct ShortenRequest {
    url: String,
    code: Option<String>,
    expires_in_minutes: Option<i64>,
    max_clicks: Option<i32>,
    password: Option<String>,
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
        .route("/admin/analytics", get(get_analytics))
        .route("/verify/:code", post(verify_link))
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
      <input id="password" type="password" placeholder="password (optional)" autocomplete="off" style="width:200px" />
      <button onclick="shorten()">shorten</button>
    </div>
    <div style="display:flex;align-items:center;gap:0.5rem;font-size:0.9rem;color:#aaa">
      <input type="checkbox" id="gen-qr" style="accent-color:#ff2d78;width:16px;height:16px;cursor:pointer" />
      <label for="gen-qr">generate qr code</label>
    </div>
    <div id="result"></div>
    <div id="qr-result" style="display:flex;flex-direction:column;align-items:center"></div>
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
    headers: HeaderMap,
) -> impl IntoResponse {
    let result = sqlx::query_as::<_, Link>(
        "SELECT * FROM links WHERE code = $1"
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(link)) => {
            if let Some(expires_at) = link.expires_at {
                if chrono::Utc::now() > expires_at {
                    return expired_page().into_response();
                }
            }
            if let Some(max) = link.max_clicks {
                if link.clicks >= max {
                    return expired_page().into_response();
                }
            }
            if link.password_hash.is_some() {
                return password_page(&code).into_response();
            }

            let _ = sqlx::query(
                "UPDATE links SET clicks = clicks + 1 WHERE code = $1"
            )
            .bind(&code)
            .execute(&state.db)
            .await;

            // Spawn background task — never blocks the redirect
            let db = state.db.clone();
            let ua = headers.get("user-agent")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let ip = headers.get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let link_id = link.id;
            tokio::spawn(async move {
                log_click(db, link_id, ua, ip).await;
            });

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
        let clamped = m.max(1).min(10080);
        chrono::Utc::now() + chrono::Duration::minutes(clamped)
    });

    let password_hash = match payload.password.as_deref() {
        Some(p) if !p.is_empty() => {
            match hash(p, DEFAULT_COST) {
                Ok(h) => Some(h),
                Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to hash password").into_response(),
            }
        }
        _ => None,
    };

    let result = sqlx::query(
        "INSERT INTO links (code, url, expires_at, max_clicks, password_hash) VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(&code)
    .bind(&url)
    .bind(&expires_at)
    .bind(&payload.max_clicks)
    .bind(&password_hash)
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
  <script src="https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js"></script>
</head>
<body>
  <h1>seraph / admin</h1>

  <div class="analytics">
    <h2>overview</h2>
    <div class="stats-cards">
      <div class="stat-card"><div class="value" id="stat-total-links">—</div><div class="label">total links</div></div>
      <div class="stat-card"><div class="value" id="stat-total-clicks">—</div><div class="label">total clicks</div></div>
      <div class="stat-card"><div class="value" id="stat-active">—</div><div class="label">active links</div></div>
      <div class="stat-card"><div class="value" id="stat-expired">—</div><div class="label">expired links</div></div>
    </div>

    <h2>clicks per day — last 30 days</h2>
    <div class="chart-container">
      <canvas id="clicks-chart"></canvas>
    </div>

    <div class="breakdown-grid">
      <div class="breakdown">
        <h3>top countries</h3>
        <div id="breakdown-countries"></div>
      </div>
      <div class="breakdown">
        <h3>top browsers</h3>
        <div id="breakdown-browsers"></div>
      </div>
      <div class="breakdown">
        <h3>top operating systems</h3>
        <div id="breakdown-os"></div>
      </div>
    </div>

    <h2>top links</h2>
    <table id="top-links-table" style="margin-bottom:2rem"></table>
  </div>

  <h2>shorten a url</h2>
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


#[derive(Deserialize)]
struct VerifyRequest {
    password: String,
}

async fn verify_link(
    Path(code): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<VerifyRequest>,
) -> impl IntoResponse {
    let result = sqlx::query_as::<_, Link>(
        "SELECT * FROM links WHERE code = $1"
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(link)) => {
            match link.password_hash {
                Some(hash) => {
                    match bcrypt_verify(&payload.password, &hash) {
                        Ok(true) => {
                            let _ = sqlx::query(
                                    "UPDATE links SET clicks = clicks + 1 WHERE code = $1"
                            )
                            .bind(&code)
                            .execute(&state.db)
                            .await;

                            let db = state.db.clone();
                            let ua = headers.get("user-agent")
                                .and_then(|v| v.to_str().ok())
                                .map(|s| s.to_string());
                            let ip = headers.get("x-real-ip")
                                .and_then(|v| v.to_str().ok())
                                .map(|s| s.to_string());
                            let link_id = link.id;
                            tokio::spawn(async move {
                                log_click(db, link_id, ua, ip).await;
                            });

                            Json(serde_json::json!({ "url": link.url })).into_response()
                        }
                        _ => (StatusCode::UNAUTHORIZED, "incorrect password").into_response(),
                    }
                }
                None => (StatusCode::BAD_REQUEST, "link is not password protected").into_response(),
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "link not found").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    }
}

#[derive(Serialize)]
struct AnalyticsData {
    total_links: i64,
    total_clicks: i64,
    active_links: i64,
    expired_links: i64,
    clicks_per_day: Vec<ClicksPerDay>,
    top_links: Vec<TopLink>,
    top_countries: Vec<StatRow>,
    top_browsers: Vec<StatRow>,
    top_os: Vec<StatRow>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ClicksPerDay {
    day: String,
    clicks: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct TopLink {
    code: String,
    url: String,
    clicks: i32,
}

#[derive(Serialize, sqlx::FromRow)]
struct StatRow {
    label: String,
    count: i64,
}

async fn get_analytics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_authorized(&headers, &state.admin_token) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let total_links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM links")
        .fetch_one(&state.db).await.unwrap_or(0);

    let total_clicks: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(clicks), 0) FROM links")
        .fetch_one(&state.db).await.unwrap_or(0);

    let active_links: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM links
         WHERE (expires_at IS NULL OR expires_at > NOW())
         AND (max_clicks IS NULL OR clicks < max_clicks)"
    ).fetch_one(&state.db).await.unwrap_or(0);

    let expired_links = total_links - active_links;

    let clicks_per_day: Vec<ClicksPerDay> = sqlx::query_as(
        "SELECT TO_CHAR(clicked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') as day,
                COUNT(*) as clicks
         FROM click_events
         WHERE clicked_at > NOW() - INTERVAL '30 days'
         GROUP BY day ORDER BY day ASC"
    ).fetch_all(&state.db).await.unwrap_or_default();

    let top_links: Vec<TopLink> = sqlx::query_as(
        "SELECT code, url, clicks FROM links ORDER BY clicks DESC LIMIT 10"
    ).fetch_all(&state.db).await.unwrap_or_default();

    let top_countries: Vec<StatRow> = sqlx::query_as(
        "SELECT COALESCE(country, 'Unknown') as label, COUNT(*) as count
         FROM click_events GROUP BY country ORDER BY count DESC LIMIT 10"
    ).fetch_all(&state.db).await.unwrap_or_default();

    let top_browsers: Vec<StatRow> = sqlx::query_as(
        "SELECT COALESCE(browser, 'Unknown') as label, COUNT(*) as count
         FROM click_events GROUP BY browser ORDER BY count DESC LIMIT 10"
    ).fetch_all(&state.db).await.unwrap_or_default();

    let top_os: Vec<StatRow> = sqlx::query_as(
        "SELECT COALESCE(os, 'Unknown') as label, COUNT(*) as count
         FROM click_events GROUP BY os ORDER BY count DESC LIMIT 10"
    ).fetch_all(&state.db).await.unwrap_or_default();

    Json(AnalyticsData {
        total_links,
        total_clicks,
        active_links,
        expired_links,
        clicks_per_day,
        top_links,
        top_countries,
        top_browsers,
        top_os,
    }).into_response()
}

// ── Helpers ───────────────────────────────────────────────────────────

fn is_authorized(headers: &HeaderMap, admin_token: &str) -> bool {
    headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == format!("Bearer {}", admin_token))
        .unwrap_or(false)
}


fn password_page(code: &str) -> Html<String> {
    Html(format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>seraph / protected link</title>
  <link rel="icon" type="image/png" href="/favicon.png">
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/hack-font@3/build/web/hack.css">
  <link rel="stylesheet" href="/static/error.css">
  <style>
    input {{ background: #111; color: #e0e0e0; border: 1px solid #ff2d78; padding: 8px 12px; font-family: 'Hack', monospace; font-size: 0.9rem; outline: none; width: 220px; }}
    input:focus {{ box-shadow: 0 0 8px #ff2d7866; }}
    button {{ background: #ff2d78; color: #0a0a0a; border: none; padding: 8px 18px; cursor: pointer; font-family: 'Hack', monospace; font-size: 0.9rem; transition: box-shadow 0.2s; margin-top: 0.5rem; }}
    button:hover {{ box-shadow: 0 0 12px #ff2d78, 0 0 24px #ff2d7866; }}
    .error-msg {{ color: #ff6b6b; font-size: 0.85rem; min-height: 1rem; }}
    form {{ display: flex; flex-direction: column; align-items: center; gap: 0.75rem; }}
  </style>
</head>
<body>
  <header>
    <a class="logo" href="https://seraph.ws">seraph</a>
    <div class="barcode"></div>
  </header>
  <main>
    <h1>protected link</h1>
    <p class="prompt">enter the password to continue</p>
    <form onsubmit="return false">
      <input id="password" type="password" placeholder="password" autocomplete="current-password" />
      <button onclick="verify()">unlock</button>
      <div class="error-msg" id="error"></div>
    </form>
  </main>
  <script>
    const CODE = "{code}";
    const SESSION_KEY = "pwd:" + CODE;

    // Auto-submit if we verified this session already
    const saved = sessionStorage.getItem(SESSION_KEY);
    if (saved) submitPassword(saved);

    async function verify() {{
      const password = document.getElementById('password').value;
      if (!password) return;
      await submitPassword(password);
    }}

    async function submitPassword(password) {{
      const res = await fetch('/verify/' + CODE, {{
        method: 'POST',
        headers: {{ 'Content-Type': 'application/json' }},
        body: JSON.stringify({{ password }})
      }});
      if (res.ok) {{
        sessionStorage.setItem(SESSION_KEY, password);
        const data = await res.json();
        window.location.href = data.url;
      }} else {{
        sessionStorage.removeItem(SESSION_KEY);
        document.getElementById('error').textContent = 'incorrect password';
        document.getElementById('password').value = '';
        document.getElementById('password').focus();
      }}
    }}

    document.addEventListener('keydown', e => {{ if (e.key === 'Enter') verify(); }});
  </script>
</body>
</html>"#, code = code))
}

async fn log_click(db: PgPool, link_id: i32, user_agent: Option<String>, ip: Option<String>) {
    let (browser, os) = match &user_agent {
        Some(ua) => {
            let parser = UaParser::new();
            match parser.parse(ua) {
                Some(result) => (
                    Some(result.name.to_string()),
                    Some(result.os.to_string()),
                ),
                None => (None, None),
            }
        }
        None => (None, None),
    };

    // Resolve country from IP asynchronously
    let country = if let Some(ip) = ip {
        let url = format!("http://ip-api.com/json/{}?fields=country", ip);
        match reqwest::get(&url).await {
            Ok(res) => {
                match res.json::<serde_json::Value>().await {
                    Ok(json) => json["country"].as_str().map(|s: &str| s.to_string()),
                    Err(_) => None,
                }
            }
            Err(_) => None,
        }
    } else {
        None
    };

    let _ = sqlx::query(
        "INSERT INTO click_events (link_id, country, browser, os) VALUES ($1, $2, $3, $4)"
    )
    .bind(link_id)
    .bind(&country)
    .bind(&browser)
    .bind(&os)
    .execute(&db)
    .await;
}