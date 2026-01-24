use axum::{
    extract::{Path, Query, State, Multipart},
    http::StatusCode,
    response::{Html, IntoResponse},
};
use std::sync::Arc;
use serde::Deserialize;
use askama::Template;

use crate::error::{AppError, Result};
use crate::rtorrent::{TorrentState, GlobalStats};
use crate::state::AppState;
use crate::services::torrents as torrents_service;
use crate::templates::{
    IndexTemplate, TorrentRowTemplate, 
    AddTorrentModalTemplate, StatsTemplate, TorrentView,
};

#[derive(Debug, Clone, Deserialize)]
pub struct FilterQuery {
    pub search: Option<String>,
    pub sort: Option<String>,
    pub order: Option<String>,
}

/// Main index page - full SSR
pub async fn index(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse> {
    let torrents = state.rtorrent.get_torrents().await.unwrap_or_default();
    let stats = state.rtorrent.get_global_stats().await.unwrap_or_else(|_| GlobalStats {
        down_rate: 0,
        up_rate: 0,
        free_disk_space: 2_000_000_000_000,
        active_peers: 0,
    });
    let rtorrent_version = state.rtorrent.get_client_version().await.unwrap_or_else(|_| "Disconnected".to_string());
    
    let mut torrent_views = Vec::new();
    for t in &torrents {
        let is_starred = state.is_starred(&t.hash).await;
        torrent_views.push(TorrentView::from_torrent(t, is_starred));
    }
    
    let total_count = torrents.len();
    let downloading_count = torrents.iter().filter(|t| t.state == TorrentState::Downloading).count();
    let seeding_count = torrents.iter().filter(|t| t.state == TorrentState::Seeding).count();
    let paused_count = torrents.iter().filter(|t| t.state == TorrentState::Paused).count();
    
    let template = IndexTemplate {
        stats,
        torrents: torrent_views,
        total_count,
        downloading_count,
        seeding_count,
        paused_count,
        rtorrent_version,
        cache_version: crate::templates::CACHE_VERSION.clone(),
    };
    
    Ok(Html(template.render().map_err(|e| AppError::TemplateError(e.to_string()))?))
}

/// Get torrent list partial (for HTMX updates)
pub async fn torrents_list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FilterQuery>,
) -> Result<impl IntoResponse> {
    let all_torrents = state.rtorrent.get_torrents().await.unwrap_or_default();
    let html = torrents_service::render_torrents_html(&state, &query, None, &all_torrents).await?;
    Ok(Html(html))
}

/// Get filtered torrent list
pub async fn torrents_filtered(
    State(state): State<Arc<AppState>>,
    Path(filter): Path<String>,
    Query(query): Query<FilterQuery>,
) -> Result<impl IntoResponse> {
    let all_torrents = state.rtorrent.get_torrents().await.unwrap_or_default();
    let html = torrents_service::render_torrents_html(&state, &query, Some(filter.as_str()), &all_torrents).await?;
    Ok(Html(html))
}

/// Pause a torrent
pub async fn torrent_pause(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> Result<impl IntoResponse> {
    state.rtorrent.pause_torrent(&hash).await?;
    
    // Return updated row
    let torrents = state.rtorrent.get_torrents().await?;
    if let Some(torrent) = torrents.iter().find(|t| t.hash == hash) {
        let is_starred = state.is_starred(&hash).await;
        let view = TorrentView::from_torrent(torrent, is_starred);
        let template = TorrentRowTemplate { torrent: view };
        Ok(Html(template.render().map_err(|e| AppError::TemplateError(e.to_string()))?))
    } else {
        Err(AppError::NotFound("Torrent not found".to_string()))
    }
}

/// Resume a torrent
pub async fn torrent_resume(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> Result<impl IntoResponse> {
    state.rtorrent.resume_torrent(&hash).await?;
    
    // Return updated row
    let torrents = state.rtorrent.get_torrents().await?;
    if let Some(torrent) = torrents.iter().find(|t| t.hash == hash) {
        let is_starred = state.is_starred(&hash).await;
        let view = TorrentView::from_torrent(torrent, is_starred);
        let template = TorrentRowTemplate { torrent: view };
        Ok(Html(template.render().map_err(|e| AppError::TemplateError(e.to_string()))?))
    } else {
        Err(AppError::NotFound("Torrent not found".to_string()))
    }
}

/// Remove a torrent
pub async fn torrent_remove(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> Result<impl IntoResponse> {
    state.rtorrent.remove_torrent(&hash).await?;
    Ok(StatusCode::OK)
}

/// Toggle star on torrent
pub async fn torrent_toggle_star(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> Result<impl IntoResponse> {
    let is_starred = state.toggle_star(&hash).await;
    
    // Return updated row
    let torrents = state.rtorrent.get_torrents().await?;
    if let Some(torrent) = torrents.iter().find(|t| t.hash == hash) {
        let view = TorrentView::from_torrent(torrent, is_starred);
        let template = TorrentRowTemplate { torrent: view };
        Ok(Html(template.render().map_err(|e| AppError::TemplateError(e.to_string()))?))
    } else {
        Err(AppError::NotFound("Torrent not found".to_string()))
    }
}

/// Show add torrent modal
pub async fn add_torrent_modal() -> Result<impl IntoResponse> {
    let template = AddTorrentModalTemplate;
    Ok(Html(template.render().map_err(|e| AppError::TemplateError(e.to_string()))?))
}

/// Add torrent (URL or file upload)
pub async fn add_torrent(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse> {
    tracing::info!("add_torrent called");
    
    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::BadRequest(e.to_string()))? {
        let name = field.name().unwrap_or_default().to_string();
        tracing::debug!("Processing field: {}", name);
        
        match name.as_str() {
            "url" => {
                let url = field.text().await.map_err(|e| AppError::BadRequest(e.to_string()))?;
                tracing::info!("URL field value: '{}'", url);
                if !url.trim().is_empty() {
                    if let Err(e) = state.rtorrent.add_torrent_url(&url).await {
                        tracing::error!("Failed to add torrent URL: {:?}", e);
                        return Err(e);
                    }
                }
            }
            "file" => {
                let data = field.bytes().await.map_err(|e| AppError::BadRequest(e.to_string()))?;
                tracing::info!("File field size: {} bytes", data.len());
                if !data.is_empty() {
                    if let Err(e) = state.rtorrent.add_torrent_file(&data).await {
                        tracing::error!("Failed to add torrent file: {:?}", e);
                        return Err(e);
                    }
                }
            }
            _ => {
                tracing::debug!("Unknown field: {}", name);
            }
        }
    }
    
    // Return updated torrent list + sidebar counts with HX-Trigger to close modal
    let torrents = state.rtorrent.get_torrents().await.unwrap_or_default();
    let query = FilterQuery {
        search: None,
        sort: None,
        order: None,
    };
    let html = torrents_service::render_torrents_html(&state, &query, None, &torrents).await?;

    Ok(([("HX-Trigger", "closeModal")], Html(html)))
}

/// Get stats partial (for HTMX polling)
pub async fn stats_partial(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse> {
    let stats = state.rtorrent.get_global_stats().await.unwrap_or_else(|_| GlobalStats {
        down_rate: 0,
        up_rate: 0,
        free_disk_space: 2_000_000_000_000,
        active_peers: 0,
    });
    
    let template = StatsTemplate { stats };
    Ok(Html(template.render().map_err(|e| AppError::TemplateError(e.to_string()))?))
}
