mod error;
mod routes;
mod rtorrent;
mod state;
mod templates;

use axum::{
    routing::{get, post},
    Router,
    response::Response,
    http::{header, StatusCode},
    extract::Path,
    body::Body,
};
use rust_embed::Embed;
use std::sync::Arc;
use tower_http::compression::CompressionLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::state::AppState;

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
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(header::CACHE_CONTROL, "public, max-age=31536000")
                .body(Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vibetorrent=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load environment variables
    dotenvy::dotenv().ok();

    // Get SCGI socket path from environment or use default
    let scgi_socket = std::env::var("RTORRENT_SCGI_SOCKET")
        .unwrap_or_else(|_| "/tmp/rtorrent.sock".to_string());

    tracing::info!("Connecting to rTorrent SCGI socket: {}", scgi_socket);

    // Create application state
    let state = Arc::new(AppState::new(scgi_socket));

    // Build router
    let app = Router::new()
        // Main pages
        .route("/", get(routes::index))
        .route("/torrents", get(routes::torrents_list))
        .route("/torrents/filter/{filter}", get(routes::torrents_filtered))
        // Torrent actions
        .route("/torrent/{hash}/pause", post(routes::torrent_pause))
        .route("/torrent/{hash}/resume", post(routes::torrent_resume))
        .route("/torrent/{hash}/remove", post(routes::torrent_remove))
        .route("/torrent/{hash}/toggle-star", post(routes::torrent_toggle_star))
        // Add torrent
        .route("/add-torrent", get(routes::add_torrent_modal))
        .route("/add-torrent", post(routes::add_torrent))
        // Stats
        .route("/stats", get(routes::stats_partial))
        // Static files (embedded in binary)
        .route("/static/{*path}", get(serve_static))
        // State and middleware
        .with_state(state)
        .layer(CompressionLayer::new());

    // Get bind address
    let addr = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("🚀 VibeTorrent listening on http://{}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
