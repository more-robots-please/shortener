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

// ── Database model ──────────────────────────────────────────────────
#[derive(Serialize, sqlx::FromRow)]
struct Link {
    id: i32,
    code: String,
    url: String,
    clicks: i32,
    created_at: chrono::DateTime<chrono::Utc>,
}

// ── Request/response types ───────────────────────────────────────────
#[derive(Deserialize)]
struct ShortenRequest {
    url: String,
    code: Option<String>, // optional custom code
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
  <link rel="apple-touch-icon" href="/apple-touch-icon.png">  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/hack-font@3/build/web/hack.css">
  <style>
    :root {{ --background: #0a0a0a; --color: #e0e0e0; --accent: #ff2d78; }}
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{ background: var(--background); color: var(--color); font-family: 'Hack', monospace; min-height: 100vh; display: flex; flex-direction: column; }}
    header {{ display: flex; align-items: center; gap: 1rem; padding: 1rem 2rem; }}
    .logo {{ background: var(--accent); color: var(--background); font-weight: bold; padding: 5px 10px; text-decoration: none; box-shadow: 0 0 8px #ff2d78, 0 0 16px #ff2d7844; }}
    .barcode {{ flex: 1; background: repeating-linear-gradient(90deg, var(--accent), var(--accent) 4px, transparent 0, transparent 10px); min-height: 28px; filter: drop-shadow(0 0 4px #ff2d78); }}
    main {{ flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 4rem 2rem; text-align: center; gap: 1.5rem; }}
    h1 {{ color: var(--accent); font-size: 2rem; text-shadow: 0 0 12px #ff2d7888; }}
    p {{ color: #888; font-size: 0.9rem; }}
    .prompt::before {{ content: '> '; color: var(--accent); }}
    .shorten-form {{ display: flex; gap: 0.5rem; flex-wrap: wrap; justify-content: center; margin-top: 0.5rem; }}
    input {{ background: #111; color: var(--color); border: 1px solid #ff2d78; padding: 8px 12px; font-family: 'Hack', monospace; font-size: 0.9rem; outline: none; }}
    input:focus {{ box-shadow: 0 0 8px #ff2d7866; }}
    input#url {{ width: 320px; }}
    input#code {{ width: 160px; }}
    button {{ background: var(--accent); color: var(--background); border: none; padding: 8px 18px; cursor: pointer; font-family: 'Hack', monospace; font-size: 0.9rem; transition: box-shadow 0.2s; }}
    button:hover {{ box-shadow: 0 0 12px #ff2d78, 0 0 24px #ff2d7866; }}
    #result {{ font-size: 0.85rem; min-height: 1.2rem; }}
    #result a {{ color: var(--accent); text-decoration: none; }}
    #result a:hover {{ text-shadow: 0 0 8px #ff2d78; }}
    #result.error {{ color: #ff6b6b; }}
    a {{ color: var(--accent); text-decoration: none; }}
    a:hover {{ text-shadow: 0 0 8px #ff2d78; }}
    footer {{ text-align: center; padding: 1rem; color: #444; font-size: 0.8rem; border-top: 1px solid #1a1a1a; }}
    footer a {{ color: #666; }}
    footer a:hover {{ color: var(--accent); }}
  </style>
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
      <button onclick="shorten()">shorten</button>
    </div>
    <div id="result"></div>
    <p><a href="https://seraph.ws">← back to seraph.ws</a></p>
  </main>
  <footer><a href="https://seraph.ws">seraph.ws</a></footer>
  <script>
    async function shorten() {{
      const url = document.getElementById('url').value.trim();
      const code = document.getElementById('code').value.trim();
      const result = document.getElementById('result');
      if (!url) {{ result.textContent = 'please enter a url'; result.className = 'error'; return; }}
      const res = await fetch('/api/shorten', {{
        method: 'POST',
        headers: {{ 'Content-Type': 'application/json' }},
        body: JSON.stringify({{ url, code: code || null }})
      }});
      const text = await res.text();
      if (res.ok) {{
        const data = JSON.parse(text);
        result.innerHTML = '> <a href="/' + data.code + '">{base_url}/' + data.code + '</a>';
        result.className = '';
      }} else {{
        result.textContent = '> error: ' + text;
        result.className = 'error';
      }}
    }}
    document.addEventListener('keydown', e => {{ if (e.key === 'Enter') shorten(); }});
  </script>
</body>
</html>"#, base_url = state.base_url))
}

async fn redirect(
    Path(code): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let result = sqlx::query_as::<_, Link>(
        "UPDATE links SET clicks = clicks + 1 WHERE code = $1 RETURNING *"
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(link)) => Redirect::permanent(&link.url).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Link not found").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    }
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
    let existing = sqlx::query_as::<_, Link>(
        "SELECT * FROM links WHERE url = $1 ORDER BY created_at ASC LIMIT 1"
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

    let result = sqlx::query(
        "INSERT INTO links (code, url) VALUES ($1, $2)"
    )
    .bind(&code)
    .bind(&url)
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

async fn admin_page(
    State(_state): State<AppState>,
) -> impl IntoResponse {
        Html(r#"<!DOCTYPE html>
<html>
<head>
  <title>seraph / admin</title>
  <style>
    button:hover { box-shadow: 0 0 10px #ff2d78, 0 0 20px #ff2d7866; }
    a:hover { text-shadow: 0 0 8px #ff2d78; }
    body { background: #0a0a0a; color: #e0e0e0; font-family: monospace; padding: 2rem; }
    h1 { color: #ff2d78; }
    table { width: 100%; border-collapse: collapse; margin-top: 1rem; }
    th, td { text-align: left; padding: 0.5rem; border-bottom: 1px solid #333; }
    th { color: #ff2d78; }
    a { color: #ff2d78; }
    button { background: #ff2d78; color: #0a0a0a; border: none; padding: 4px 10px; cursor: pointer; font-family: monospace; }
    button:hover { background: #ff69b4; }
    .form { margin-bottom: 2rem; }
    input { background: #111; color: #e0e0e0; border: 1px solid #ff2d78; padding: 4px 8px; font-family: monospace; width: 300px; }
    .submit { background: #ff2d78; color: #0a0a0a; border: none; padding: 5px 12px; cursor: pointer; font-family: monospace; }
  </style>
</head>
<body>
  <h1>seraph / admin</h1>
  <div class="form">
    <h2>shorten a url</h2>
    <input id="url" placeholder="https://example.com" />
    <input id="code" placeholder="custom code (optional)" style="width:180px" />
    <button class="submit" onclick="shorten()">shorten</button>
    <p id="result"></p>
  </div>
  <h2>all links</h2>
  <table id="links-table">
    <tr><th>code</th><th>url</th><th>clicks</th><th>created</th><th></th></tr>
  </table>
    <script>
        let TOKEN = localStorage.getItem('admin_token');
    
        if (!TOKEN) {
          document.body.innerHTML = `
            <div style="display:flex;flex-direction:column;align-items:center;justify-content:center;height:100vh;gap:1rem;">
              <h1 style="color:#ff2d78">seraph / admin</h1>
              <input id="token-input" type="text" placeholder="admin token" autocomplete="off"
              <button onclick="login()" 
                style="background:#ff2d78;color:#0a0a0a;border:none;padding:8px 20px;cursor:pointer;font-family:monospace;font-size:1rem">
                enter
              </button>
            </div>`;
        } else {
          loadLinks();
        }
    
        function login() {
          TOKEN = document.getElementById('token-input').value;
          localStorage.setItem('admin_token', TOKEN);
          location.reload();
        }
    
        async function loadLinks() {
          const res = await fetch("/admin/links", {
            headers: { "Authorization": "Bearer " + TOKEN }
          });
          if (res.status === 401) {
            localStorage.removeItem('admin_token');
            location.reload();
            return;
          }
          const links = await res.json();
          const table = document.getElementById("links-table");
          table.innerHTML = "<tr><th>code</th><th>url</th><th>clicks</th><th>created</th><th></th></tr>";
          for (const l of links) {
            table.innerHTML += `<tr>
              <td><a href="/${l.code}">${l.code}</a></td>
              <td><a href="${l.url}" target="_blank">${l.url.substring(0,50)}${l.url.length>50?"...":""}</a></td>
              <td>${l.clicks}</td>
              <td>${new Date(l.created_at).toLocaleDateString()}</td>
              <td><button onclick="del(${l.id})">delete</button></td>
            </tr>`;
          }
        }
    
        async function shorten() {
          const url = document.getElementById("url").value;
          const code = document.getElementById("code").value;
          const res = await fetch("/api/shorten", {
            method: "POST",
            headers: { "Content-Type": "application/json", "Authorization": "Bearer " + TOKEN },
            body: JSON.stringify({ url, code: code || null })
          });
          const data = await res.json();
          document.getElementById("result").textContent = res.ok ? "shortened: " + data.short_url : "error: " + JSON.stringify(data);
          if (res.ok) loadLinks();
        }
    
        async function del(id) {
          await fetch("/admin/links/" + id, {
            method: "DELETE",
            headers: { "Authorization": "Bearer " + TOKEN }
          });
          loadLinks();
        }
    </script>
</body>
</html>"#).into_response()
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
