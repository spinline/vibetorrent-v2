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
use futures::StreamExt;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio_stream::wrappers::BroadcastStream;

use crate::error::AppError;
use crate::routes::FilterQuery;
use crate::rtorrent::TorrentState;
use crate::state::AppState;
use crate::templates::{SidebarCountsTemplate, StatsTemplate, TorrentListTemplate, TorrentView};
use askama::Template;

/// SSE endpoint for torrent list updates
/// 
/// Clients connect with optional filter/sort parameters:
/// GET /events/torrents?search=ubuntu&sort=name&order=asc
pub async fn torrent_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FilterQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let initial = match state.latest_torrents().await {
        Some(torrents) => {
            let html = match render_torrents_html(&state, &query, None, &torrents).await {
                Ok(html) => html,
                Err(_) => String::from("<div class=\"text-red-400\">Error loading torrents</div>"),
            };
            Some(Ok(Event::default().event("torrents").data(html)))
        }
        None => None,
    };

    let updates = BroadcastStream::new(state.subscribe_torrents()).filter_map({
        let state = state.clone();
        let query = query.clone();
        move |msg| {
            let state = state.clone();
            let query = query.clone();
            async move {
                match msg {
                    Ok(torrents) => {
                        let html = match render_torrents_html(&state, &query, None, &torrents).await {
                            Ok(html) => html,
                            Err(_) => String::from("<div class=\"text-red-400\">Error loading torrents</div>"),
                        };
                        Some(Ok(Event::default().event("torrents").data(html)))
                    }
                    Err(_) => None,
                }
            }
        }
    });

    let stream = stream::iter(initial.into_iter()).chain(updates);

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
    let initial = match state.latest_torrents().await {
        Some(torrents) => {
            let html = match render_torrents_html(&state, &query, Some(&filter), &torrents).await {
                Ok(html) => html,
                Err(_) => String::from("<div class=\"text-red-400\">Error loading torrents</div>"),
            };
            Some(Ok(Event::default().event("torrents").data(html)))
        }
        None => None,
    };

    let updates = BroadcastStream::new(state.subscribe_torrents()).filter_map({
        let state = state.clone();
        let query = query.clone();
        let filter = filter.clone();
        move |msg| {
            let state = state.clone();
            let query = query.clone();
            let filter = filter.clone();
            async move {
                match msg {
                    Ok(torrents) => {
                        let html = match render_torrents_html(&state, &query, Some(&filter), &torrents).await {
                            Ok(html) => html,
                            Err(_) => String::from("<div class=\"text-red-400\">Error loading torrents</div>"),
                        };
                        Some(Ok(Event::default().event("torrents").data(html)))
                    }
                    Err(_) => None,
                }
            }
        }
    });

    let stream = stream::iter(initial.into_iter()).chain(updates);

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
    let initial = match state.latest_stats().await {
        Some(stats) => {
            let template = StatsTemplate { stats: (*stats).clone() };
            let html = template.render().unwrap_or_default();
            Some(Ok(Event::default().event("stats").data(html)))
        }
        None => None,
    };

    let updates = BroadcastStream::new(state.subscribe_stats()).filter_map(|msg| async move {
        match msg {
            Ok(stats) => {
                let template = StatsTemplate { stats: (*stats).clone() };
                let html = template.render().unwrap_or_default();
                Some(Ok(Event::default().event("stats").data(html)))
            }
            Err(_) => None,
        }
    });

    let stream = stream::iter(initial.into_iter()).chain(updates);

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// Render torrent list HTML from a shared snapshot, applying optional filter/search/sort.
async fn render_torrents_html(
    state: &Arc<AppState>,
    query: &FilterQuery,
    filter: Option<&str>,
    all_torrents: &[crate::rtorrent::Torrent],
) -> Result<String, AppError> {
    let mut torrents = all_torrents.to_vec();

    // Apply status filter
    if let Some(filter) = filter {
        match filter {
            "downloading" => torrents.retain(|t| t.state == TorrentState::Downloading),
            "seeding" => torrents.retain(|t| t.state == TorrentState::Seeding),
            "paused" => torrents.retain(|t| t.state == TorrentState::Paused),
            _ => {}
        }
    }

    // Apply search filter
    if let Some(search) = &query.search {
        let search_lower = search.to_lowercase();
        torrents.retain(|t| t.name.to_lowercase().contains(&search_lower));
    }

    // Apply sorting
    apply_sorting(&mut torrents, query);

    // Starred set snapshot (avoid per-row await)
    let starred = state.starred_torrents.read().await.clone();

    // Convert to views
    let mut torrent_views = Vec::with_capacity(torrents.len());
    for t in &torrents {
        let is_starred = starred.contains(&t.hash);
        torrent_views.push(TorrentView::from_torrent(t, is_starred));
    }

    // Calculate counts from all torrents (not filtered)
    let counts = calculate_counts(all_torrents);

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
