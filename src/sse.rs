//! Server-Sent Events (SSE) implementation for real-time torrent updates.
//!
//! This module provides a clean SSE implementation that:
//! - Broadcasts torrent updates to all connected clients
//! - Supports filtering and sorting per-client via query parameters
//! - Handles reconnection gracefully
//! - Includes sidebar counts and stats updates

use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures::stream::{self, Stream};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::time::interval;

use crate::error::AppError;
use crate::routes::FilterQuery;
use crate::rtorrent::{GlobalStats, TorrentState};
use crate::state::AppState;
use crate::templates::{SidebarCountsTemplate, StatsTemplate, TorrentListTemplate, TorrentView};
use askama::Template;

/// SSE update interval in seconds
const SSE_UPDATE_INTERVAL: u64 = 2;

/// SSE endpoint for torrent list updates
/// 
/// Clients connect with optional filter/sort parameters:
/// GET /events/torrents?search=ubuntu&sort=name&order=asc
pub async fn torrent_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FilterQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::unfold(
        (state, query, interval(Duration::from_secs(SSE_UPDATE_INTERVAL))),
        |(state, query, mut ticker)| async move {
            ticker.tick().await;
            
            // Get and process torrents
            let html = match generate_torrent_html(&state, &query).await {
                Ok(html) => html,
                Err(_) => String::from("<div class=\"text-red-400\">Error loading torrents</div>"),
            };
            
            let event = Event::default()
                .event("torrent-update")
                .data(html);
            
            Some((Ok(event), (state, query, ticker)))
        },
    );

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// SSE endpoint for filtered torrent list updates
pub async fn torrent_filtered_events(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(filter): axum::extract::Path<String>,
    Query(query): Query<FilterQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::unfold(
        (state, filter, query, interval(Duration::from_secs(SSE_UPDATE_INTERVAL))),
        |(state, filter, query, mut ticker)| async move {
            ticker.tick().await;
            
            let html = match generate_filtered_torrent_html(&state, &filter, &query).await {
                Ok(html) => html,
                Err(_) => String::from("<div class=\"text-red-400\">Error loading torrents</div>"),
            };
            
            let event = Event::default()
                .event("torrent-update")
                .data(html);
            
            Some((Ok(event), (state, filter, query, ticker)))
        },
    );

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// SSE endpoint for stats updates (download/upload speed, disk space, peers)
pub async fn stats_events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::unfold(
        (state, interval(Duration::from_secs(SSE_UPDATE_INTERVAL))),
        |(state, mut ticker)| async move {
            ticker.tick().await;
            
            let stats = state.rtorrent.get_global_stats().await.unwrap_or_else(|_| GlobalStats {
                down_rate: 0,
                up_rate: 0,
                free_disk_space: 0,
                active_peers: 0,
            });
            
            let template = StatsTemplate { stats };
            let html = template.render().unwrap_or_default();
            
            let event = Event::default()
                .event("stats-update")
                .data(html);
            
            Some((Ok(event), (state, ticker)))
        },
    );

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// Generate HTML for torrent list with filtering and sorting
async fn generate_torrent_html(
    state: &Arc<AppState>,
    query: &FilterQuery,
) -> Result<String, AppError> {
    let all_torrents = state.rtorrent.get_torrents().await.unwrap_or_default();
    let mut torrents = all_torrents.clone();
    
    // Apply search filter
    if let Some(search) = &query.search {
        let search_lower = search.to_lowercase();
        torrents.retain(|t| t.name.to_lowercase().contains(&search_lower));
    }
    
    // Apply sorting
    apply_sorting(&mut torrents, query);
    
    // Convert to views
    let mut torrent_views = Vec::with_capacity(torrents.len());
    for t in &torrents {
        let is_starred = state.is_starred(&t.hash).await;
        torrent_views.push(TorrentView::from_torrent(t, is_starred));
    }
    
    // Calculate counts from all torrents
    let counts = calculate_counts(&all_torrents);
    
    // Render templates
    let list_template = TorrentListTemplate { torrents: torrent_views };
    let counts_template = SidebarCountsTemplate {
        total_count: counts.total,
        downloading_count: counts.downloading,
        seeding_count: counts.seeding,
        paused_count: counts.paused,
    };
    
    let list_html = list_template.render().map_err(|e| AppError::TemplateError(e.to_string()))?;
    let counts_html = counts_template.render().map_err(|e| AppError::TemplateError(e.to_string()))?;
    
    Ok(format!("{}{}", list_html, counts_html))
}

/// Generate HTML for filtered torrent list
async fn generate_filtered_torrent_html(
    state: &Arc<AppState>,
    filter: &str,
    query: &FilterQuery,
) -> Result<String, AppError> {
    let all_torrents = state.rtorrent.get_torrents().await.unwrap_or_default();
    let mut torrents = all_torrents.clone();
    
    // Apply status filter
    match filter {
        "downloading" => torrents.retain(|t| t.state == TorrentState::Downloading),
        "seeding" => torrents.retain(|t| t.state == TorrentState::Seeding),
        "paused" => torrents.retain(|t| t.state == TorrentState::Paused),
        _ => {} // "all" - no filter
    }
    
    // Apply search filter
    if let Some(search) = &query.search {
        let search_lower = search.to_lowercase();
        torrents.retain(|t| t.name.to_lowercase().contains(&search_lower));
    }
    
    // Apply sorting
    apply_sorting(&mut torrents, query);
    
    // Convert to views
    let mut torrent_views = Vec::with_capacity(torrents.len());
    for t in &torrents {
        let is_starred = state.is_starred(&t.hash).await;
        torrent_views.push(TorrentView::from_torrent(t, is_starred));
    }
    
    // Calculate counts from all torrents
    let counts = calculate_counts(&all_torrents);
    
    // Render templates
    let list_template = TorrentListTemplate { torrents: torrent_views };
    let counts_template = SidebarCountsTemplate {
        total_count: counts.total,
        downloading_count: counts.downloading,
        seeding_count: counts.seeding,
        paused_count: counts.paused,
    };
    
    let list_html = list_template.render().map_err(|e| AppError::TemplateError(e.to_string()))?;
    let counts_html = counts_template.render().map_err(|e| AppError::TemplateError(e.to_string()))?;
    
    Ok(format!("{}{}", list_html, counts_html))
}

/// Torrent counts structure
struct TorrentCounts {
    total: usize,
    downloading: usize,
    seeding: usize,
    paused: usize,
}

/// Calculate torrent counts by state
fn calculate_counts(torrents: &[crate::rtorrent::Torrent]) -> TorrentCounts {
    TorrentCounts {
        total: torrents.len(),
        downloading: torrents.iter().filter(|t| t.state == TorrentState::Downloading).count(),
        seeding: torrents.iter().filter(|t| t.state == TorrentState::Seeding).count(),
        paused: torrents.iter().filter(|t| t.state == TorrentState::Paused).count(),
    }
}

/// Apply sorting to torrent list based on query parameters
fn apply_sorting(torrents: &mut [crate::rtorrent::Torrent], query: &FilterQuery) {
    let is_desc = query.order.as_deref() != Some("asc");
    
    if let Some(sort) = &query.sort {
        match sort.as_str() {
            "name" => {
                torrents.sort_by(|a, b| {
                    let cmp = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                    if is_desc { cmp.reverse() } else { cmp }
                });
            }
            "size" => {
                torrents.sort_by(|a, b| {
                    let cmp = a.size_bytes.cmp(&b.size_bytes);
                    if is_desc { cmp.reverse() } else { cmp }
                });
            }
            "progress" => {
                torrents.sort_by(|a, b| {
                    let cmp = a.progress_percent().partial_cmp(&b.progress_percent())
                        .unwrap_or(std::cmp::Ordering::Equal);
                    if is_desc { cmp.reverse() } else { cmp }
                });
            }
            "down_rate" => {
                torrents.sort_by(|a, b| {
                    let cmp = a.down_rate.cmp(&b.down_rate);
                    if is_desc { cmp.reverse() } else { cmp }
                });
            }
            "up_rate" => {
                torrents.sort_by(|a, b| {
                    let cmp = a.up_rate.cmp(&b.up_rate);
                    if is_desc { cmp.reverse() } else { cmp }
                });
            }
            _ => {}
        }
    }
}
