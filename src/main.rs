mod config;
mod error;
mod routes;
mod rtorrent;
mod services;
mod sse;
mod state;
mod templates;

use askama::Template;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use clap::Parser;
use rust_embed::Embed;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::compression::CompressionLayer;

use crate::config::Config;
use crate::state::AppState;
use crate::templates::SetupTemplate;

/// Shared state that can be updated at runtime
pub struct SharedState {
    pub app_state: RwLock<Option<Arc<AppState>>>,
    pub config: RwLock<Option<Config>>,
}

impl SharedState {
    pub fn new(config: Option<Config>) -> Self {
        let app_state = config
            .as_ref()
            .map(|c| Arc::new(AppState::new(c.scgi_socket.clone())));
        Self {
            app_state: RwLock::new(app_state),
            config: RwLock::new(config),
        }
    }

    pub async fn update_config(&self, config: Config) {
        let app_state = Arc::new(AppState::new(config.scgi_socket.clone()));
        *self.app_state.write().await = Some(app_state);
        *self.config.write().await = Some(config);
    }

    pub async fn is_configured(&self) -> bool {
        self.config.read().await.is_some()
    }

    pub async fn get_app_state(&self) -> Option<Arc<AppState>> {
        self.app_state.read().await.clone()
    }
}

/// VibeTorrent - Modern rTorrent Web UI
#[derive(Parser, Debug)]
#[command(name = "vibetorrent")]
#[command(about = "Modern rTorrent Web UI", long_about = None)]
struct Args {
    /// rTorrent SCGI socket path or TCP address
    #[arg(short, long)]
    socket: Option<String>,

    /// Bind address (IP:PORT)
    #[arg(short, long)]
    bind: Option<String>,

    /// Run setup wizard (force)
    #[arg(long)]
    setup: bool,
}

// Embed static files into the binary
#[derive(Embed)]
#[folder = "static/"]
struct StaticFiles;

// Handler to serve embedded static files
async fn serve_static(Path(path): Path<String>) -> Response<Body> {
    let path = path.as_str();

    match StaticFiles::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let mut response = Response::new(Body::from(content.data.into_owned()));
            *response.status_mut() = StatusCode::OK;

            let headers = response.headers_mut();
            let content_type = HeaderValue::from_str(mime.as_ref())
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
            headers.insert(header::CONTENT_TYPE, content_type);
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000"),
            );

            response
        }
        None => {
            let mut response = Response::new(Body::from("Not Found"));
            *response.status_mut() = StatusCode::NOT_FOUND;
            response
        }
    }
}

#[derive(Deserialize)]
struct SetupForm {
    scgi_socket: String,
    bind_address: String,
}

async fn setup_page(error: Option<String>) -> Html<String> {
    let config = Config::load().unwrap_or_default();
    let template = SetupTemplate {
        scgi_socket: config.scgi_socket,
        bind_address: config.bind_address,
        error,
        cache_version: crate::templates::CACHE_VERSION.clone(),
    };

    match template.render() {
        Ok(html) => Html(html),
        Err(err) => {
            tracing::error!("Failed to render setup template: {}", err);
            Html(format!(
                r#"<!doctype html>
<html lang=\"en\">
    <head>
        <meta charset=\"utf-8\" />
        <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
        <title>Setup Error</title>
        <style>
            body {{ background:#0b1220; color:#e2e8f0; font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Helvetica, Arial; padding: 24px; }}
            .card {{ max-width: 900px; margin: 0 auto; background:#0f172a; border:1px solid #1f2937; border-radius: 12px; padding: 20px; }}
            h1 {{ margin: 0 0 12px; font-size: 20px; }}
            pre {{ white-space: pre-wrap; background:#020617; border:1px solid #1f2937; padding: 12px; border-radius: 10px; color:#fca5a5; }}
            .hint {{ color:#94a3b8; font-size: 14px; }}
        </style>
    </head>
    <body>
        <div class=\"card\">
            <h1>Setup page failed to render</h1>
            <p class=\"hint\">The server is running, but the HTML template render errored. Check server logs for details.</p>
            <pre>{}</pre>
        </div>
    </body>
</html>"#,
                err
            ))
        }
    }
}

async fn setup_get() -> Html<String> {
    setup_page(None).await
}

async fn setup_post(
    State(shared): State<Arc<SharedState>>,
    Form(form): Form<SetupForm>,
) -> Response<Body> {
    let config = Config {
        scgi_socket: form.scgi_socket.trim().to_string(),
        bind_address: form.bind_address.trim().to_string(),
    };

    // Validate socket path
    if config.scgi_socket.is_empty() {
        let html = setup_page(Some("SCGI socket path is required".to_string())).await;
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(header::CONTENT_TYPE, "text/html")
            .body(Body::from(html.0))
            .unwrap();
    }

    // Test rtorrent connection before saving
    let client = crate::rtorrent::RtorrentClient::new(config.scgi_socket.clone());
    if !client.test_connection().await {
        let html = setup_page(Some(format!(
            "Cannot connect to rtorrent at '{}'. Please check the socket path and ensure rtorrent is running.",
            config.scgi_socket
        ))).await;
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(header::CONTENT_TYPE, "text/html")
            .body(Body::from(html.0))
            .unwrap();
    }

    // Save config to file
    if let Err(e) = config.save() {
        let html = setup_page(Some(e)).await;
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header(header::CONTENT_TYPE, "text/html")
            .body(Body::from(html.0))
            .unwrap();
    }

    // Update runtime state - this enables the main app without restart!
    shared.update_config(config).await;

    // Redirect to main app
    Redirect::to("/").into_response()
}

// Middleware to check if setup is needed
async fn setup_guard(
    State(shared): State<Arc<SharedState>>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    let path = request.uri().path();

    // Always allow setup routes and static files
    if path.starts_with("/setup") || path.starts_with("/static/") {
        return next.run(request).await;
    }

    // Check if configured
    if !shared.is_configured().await {
        return Redirect::to("/setup").into_response();
    }

    next.run(request).await
}

fn create_router(shared: Arc<SharedState>, _force_setup: bool) -> Router {
    // Wrapper handlers that extract AppState from SharedState
    async fn index_handler(State(shared): State<Arc<SharedState>>) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            routes::index(State(state)).await.into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    async fn torrents_list_handler(
        State(shared): State<Arc<SharedState>>,
        query: axum::extract::Query<routes::FilterQuery>,
    ) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            routes::torrents_list(State(state), query)
                .await
                .into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    async fn torrents_filtered_handler(
        State(shared): State<Arc<SharedState>>,
        Path(filter): Path<String>,
        query: axum::extract::Query<routes::FilterQuery>,
    ) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            routes::torrents_filtered(State(state), Path(filter), query)
                .await
                .into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    async fn torrent_pause_handler(
        State(shared): State<Arc<SharedState>>,
        Path(hash): Path<String>,
    ) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            routes::torrent_pause(State(state), Path(hash))
                .await
                .into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    async fn torrent_resume_handler(
        State(shared): State<Arc<SharedState>>,
        Path(hash): Path<String>,
    ) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            routes::torrent_resume(State(state), Path(hash))
                .await
                .into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    async fn torrent_remove_handler(
        State(shared): State<Arc<SharedState>>,
        Path(hash): Path<String>,
    ) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            routes::torrent_remove(State(state), Path(hash))
                .await
                .into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    async fn torrent_toggle_star_handler(
        State(shared): State<Arc<SharedState>>,
        Path(hash): Path<String>,
    ) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            routes::torrent_toggle_star(State(state), Path(hash))
                .await
                .into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    async fn add_torrent_modal_handler() -> Response<Body> {
        routes::add_torrent_modal().await.into_response()
    }

    async fn add_torrent_handler(
        State(shared): State<Arc<SharedState>>,
        form: axum::extract::Multipart,
    ) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            routes::add_torrent(State(state), form)
                .await
                .into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    async fn stats_handler(State(shared): State<Arc<SharedState>>) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            routes::stats_partial(State(state)).await.into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    // SSE handlers for real-time updates
    async fn sse_torrents_handler(
        State(shared): State<Arc<SharedState>>,
        query: axum::extract::Query<routes::FilterQuery>,
    ) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            sse::torrent_events(State(state), query)
                .await
                .into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    async fn sse_torrents_filtered_handler(
        State(shared): State<Arc<SharedState>>,
        Path(filter): Path<String>,
        query: axum::extract::Query<routes::FilterQuery>,
    ) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            sse::torrent_filtered_events(State(state), Path(filter), query)
                .await
                .into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    async fn sse_stats_handler(State(shared): State<Arc<SharedState>>) -> Response<Body> {
        if let Some(state) = shared.get_app_state().await {
            sse::stats_events(State(state)).await.into_response()
        } else {
            Redirect::to("/setup").into_response()
        }
    }

    // Setup route for first-time or forced setup
    async fn setup_get_handler(State(_shared): State<Arc<SharedState>>) -> Response<Body> {
        setup_get().await.into_response()
    }

    let shared_clone = shared.clone();

    let router = Router::new()
        // Setup routes
        .route("/setup", get(setup_get_handler))
        .route("/setup", post(setup_post))
        // Main pages
        .route("/", get(index_handler))
        .route("/torrents", get(torrents_list_handler))
        .route("/torrents/filter/{filter}", get(torrents_filtered_handler))
        // Torrent actions
        .route("/torrent/{hash}/pause", post(torrent_pause_handler))
        .route("/torrent/{hash}/resume", post(torrent_resume_handler))
        .route("/torrent/{hash}/remove", post(torrent_remove_handler))
        .route(
            "/torrent/{hash}/toggle-star",
            post(torrent_toggle_star_handler),
        )
        // Add torrent
        .route("/add-torrent", get(add_torrent_modal_handler))
        .route("/add-torrent", post(add_torrent_handler))
        // Stats
        .route("/stats", get(stats_handler))
        // SSE endpoints for real-time updates
        .route("/events/torrents", get(sse_torrents_handler))
        .route(
            "/events/torrents/filter/{filter}",
            get(sse_torrents_filtered_handler),
        )
        .route("/events/stats", get(sse_stats_handler))
        // Static files (embedded in binary)
        .route("/static/{*path}", get(serve_static))
        // State
        .with_state(shared)
        // Middleware - redirect to setup if not configured
        .layer(middleware::from_fn_with_state(shared_clone, setup_guard))
        .layer(CompressionLayer::new());

    router
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments
    let args = Args::parse();

    // Load config if exists (CLI args can override)
    let mut config = if let Some(socket) = args.socket.as_ref() {
        // CLI socket provided - use it
        Some(Config {
            scgi_socket: socket.clone(),
            bind_address: args
                .bind
                .clone()
                .unwrap_or_else(|| "0.0.0.0:3000".to_string()),
        })
    } else if Config::exists() && !args.setup {
        // Config file exists and not forcing setup
        Config::load()
    } else {
        // No config - will show setup
        None
    };

    // Test rtorrent connection if config exists
    if let Some(ref cfg) = config {
        let client = crate::rtorrent::RtorrentClient::new(cfg.scgi_socket.clone());
        if !client.test_connection().await {
            eprintln!("⚠️  Cannot connect to rtorrent at {}", cfg.scgi_socket);
            eprintln!("   Starting setup wizard...");
            config = None; // Force setup mode
        }
    }

    // Determine bind address
    let bind_addr = args
        .bind
        .or_else(|| config.as_ref().map(|c| c.bind_address.clone()))
        .unwrap_or_else(|| "0.0.0.0:3000".to_string());

    // Create shared state
    let shared = Arc::new(SharedState::new(config.clone()));

    // Print startup message
    if config.is_some() && !args.setup {
        let cfg = config.as_ref().unwrap();
        println!("🚀 VibeTorrent");
        println!("   SCGI Socket: {}", cfg.scgi_socket);
        println!("   Listening:   http://{}", bind_addr);
    } else {
        println!("🔧 VibeTorrent Setup");
        println!("   Open http://{} in your browser", bind_addr);
    }

    // Create unified router
    let app = create_router(shared, args.setup);

    // Start server
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
