mod config;
mod error;
mod routes;
mod rtorrent;
mod state;
mod templates;

use axum::{
    routing::{get, post},
    Router,
    response::{Response, Html, Redirect},
    http::{header, StatusCode},
    extract::Path,
    body::Body,
    Form,
};
use clap::Parser;
use rust_embed::Embed;
use serde::Deserialize;
use std::sync::Arc;
use tower_http::compression::CompressionLayer;
use askama::Template;

use crate::config::Config;
use crate::state::AppState;
use crate::templates::SetupTemplate;

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
    
    /// Run setup wizard
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
    };
    Html(template.render().unwrap_or_default())
}

async fn setup_get() -> Html<String> {
    setup_page(None).await
}

async fn setup_post(Form(form): Form<SetupForm>) -> Response<Body> {
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
    
    // Save config
    if let Err(e) = config.save() {
        let html = setup_page(Some(e)).await;
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header(header::CONTENT_TYPE, "text/html")
            .body(Body::from(html.0))
            .unwrap();
    }
    
    // Redirect with message to restart
    let html = r#"
    <!DOCTYPE html>
    <html>
    <head>
        <title>Setup Complete - VibeTorrent</title>
        <style>
            body { background: #0a0e14; color: #e6edf3; font-family: system-ui; display: flex; align-items: center; justify-content: center; min-height: 100vh; margin: 0; }
            .card { background: #1e293b; padding: 2rem; border-radius: 1rem; text-align: center; max-width: 400px; }
            h1 { color: #10b981; margin-bottom: 1rem; }
            p { color: #94a3b8; }
            code { background: #334155; padding: 0.25rem 0.5rem; border-radius: 0.25rem; color: #e6edf3; }
        </style>
    </head>
    <body>
        <div class="card">
            <h1>✓ Setup Complete</h1>
            <p>Configuration saved to <code>vibetorrent.json</code></p>
            <p style="margin-top: 1rem;">Please restart VibeTorrent to apply changes.</p>
        </div>
    </body>
    </html>
    "#;
    
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html")
        .body(Body::from(html))
        .unwrap()
}

fn setup_router() -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/setup") }))
        .route("/setup", get(setup_get))
        .route("/setup", post(setup_post))
        .route("/static/{*path}", get(serve_static))
}

fn main_router(state: Arc<AppState>) -> Router {
    Router::new()
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
        .layer(CompressionLayer::new())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments
    let args = Args::parse();
    
    // Determine if we need setup mode
    let needs_setup = args.setup || (!Config::exists() && args.socket.is_none());
    
    if needs_setup {
        // Run setup wizard
        let bind_addr = args.bind
            .or_else(|| Config::load().map(|c| c.bind_address))
            .unwrap_or_else(|| "0.0.0.0:3000".to_string());
        
        println!("🔧 VibeTorrent Setup");
        println!("   Open http://{} in your browser", bind_addr);
        
        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        let app = setup_router();
        axum::serve(listener, app).await?;
    } else {
        // Load config (CLI args override config file)
        let config = Config::load().unwrap_or_default();
        
        let scgi_socket = args.socket.unwrap_or(config.scgi_socket);
        let bind_addr = args.bind.unwrap_or(config.bind_address);
        
        println!("🚀 VibeTorrent");
        println!("   SCGI Socket: {}", scgi_socket);
        println!("   Listening:   http://{}", bind_addr);
        
        // Create application state
        let state = Arc::new(AppState::new(scgi_socket));
        
        // Start server
        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        let app = main_router(state);
        axum::serve(listener, app).await?;
    }
    
    Ok(())
}
