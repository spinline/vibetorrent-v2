//! Server-Sent Events (SSE) implementation for real-time torrent updates.
//!
//! This module provides a clean SSE implementation that:
//! - Broadcasts torrent updates to all connected clients
//! - Supports filtering and sorting per-client via query parameters
//! - Handles reconnection gracefully
//! - Uses OOB (Out-of-Band) updates to prevent UI flickering
//! - Includes sidebar counts and stats updates

use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures::stream::{self, Stream};
use futures::StreamExt;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio_stream::wrappers::BroadcastStream;

use crate::routes::FilterQuery;
use crate::services::torrents as torrents_service;
use crate::state::AppState;
use crate::templates::StatsTemplate;
use askama::Template;

/// SSE endpoint for torrent list updates
/// 
/// Clients connect with optional filter/sort parameters:
/// GET /events/torrents?search=ubuntu&sort=name&order=asc
///
/// First event: Full torrent list HTML (for initial render)
/// Subsequent events: OOB updates for dynamic fields only (prevents flicker)
pub async fn torrent_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FilterQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // First event: send full list for initial render
    let initial = match state.latest_torrents().await {
        Some(torrents) => {
            let html = match torrents_service::render_torrents_html(&state, &query, None, &torrents).await {
                Ok(html) => html,
                Err(_) => String::from("<div class=\"text-red-400\">Error loading torrents</div>"),
            };
            Some(Ok(Event::default().event("torrents").data(html)))
        }
        None => None,
    };

    // Subsequent events: send OOB updates for dynamic fields only
    let updates = BroadcastStream::new(state.subscribe_torrents()).filter_map({
        let state = state.clone();
        let query = query.clone();
        move |msg| {
            let state = state.clone();
            let query = query.clone();
            async move {
                match msg {
                    Ok(torrents) => {
                        // Use OOB updates for subsequent events to prevent flickering
                        let html = match torrents_service::render_torrents_oob_html(&state, &query, None, &torrents).await {
                            Ok(html) => html,
                            Err(_) => String::from(""),
                        };
                        // Only send if there's content
                        if html.is_empty() {
                            None
                        } else {
                            Some(Ok(Event::default().event("torrents").data(html)))
                        }
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
    // First event: send full list for initial render
    let initial = match state.latest_torrents().await {
        Some(torrents) => {
            let html = match torrents_service::render_torrents_html(&state, &query, Some(&filter), &torrents).await {
                Ok(html) => html,
                Err(_) => String::from("<div class=\"text-red-400\">Error loading torrents</div>"),
            };
            Some(Ok(Event::default().event("torrents").data(html)))
        }
        None => None,
    };

    // Subsequent events: send OOB updates for dynamic fields only
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
                        // Use OOB updates for subsequent events to prevent flickering
                        let html = match torrents_service::render_torrents_oob_html(&state, &query, Some(&filter), &torrents).await {
                            Ok(html) => html,
                            Err(_) => String::from(""),
                        };
                        if html.is_empty() {
                            None
                        } else {
                            Some(Ok(Event::default().event("torrents").data(html)))
                        }
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
