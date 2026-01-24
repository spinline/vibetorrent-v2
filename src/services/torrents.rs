use askama::Template;
use std::sync::Arc;

use crate::error::AppError;
use crate::routes::FilterQuery;
use crate::rtorrent::{Torrent, TorrentState};
use crate::state::AppState;
use crate::templates::{SidebarCountsTemplate, TorrentListTemplate, TorrentView};

/// Render torrent list + sidebar counts from a shared snapshot, applying optional filter/search/sort.
///
/// Returns HTML that concatenates:
/// - torrent list partial
/// - sidebar counts partial
pub async fn render_torrents_html(
    state: &Arc<AppState>,
    query: &FilterQuery,
    filter: Option<&str>,
    all_torrents: &[Torrent],
) -> Result<String, AppError> {
    let torrents = apply_filter_sort(all_torrents, filter, query);

    // Starred set snapshot (avoid per-row await)
    let starred = state.starred_torrents.read().await.clone();

    let mut torrent_views = Vec::with_capacity(torrents.len());
    for t in &torrents {
        let is_starred = starred.contains(&t.hash);
        torrent_views.push(TorrentView::from_torrent(t, is_starred));
    }

    let counts = calculate_counts(all_torrents);

    let list_template = TorrentListTemplate { torrents: torrent_views };
    let counts_template = SidebarCountsTemplate {
        total_count: counts.total,
        downloading_count: counts.downloading,
        seeding_count: counts.seeding,
        paused_count: counts.paused,
    };

    let list_html = list_template
        .render()
        .map_err(|e| AppError::TemplateError(e.to_string()))?;
    let counts_html = counts_template
        .render()
        .map_err(|e| AppError::TemplateError(e.to_string()))?;

    Ok(format!("{}{}", list_html, counts_html))
}

pub fn apply_filter_sort(
    all_torrents: &[Torrent],
    filter: Option<&str>,
    query: &FilterQuery,
) -> Vec<Torrent> {
    let mut torrents = all_torrents.to_vec();

    // Status filter
    if let Some(filter) = filter {
        match filter {
            "downloading" => torrents.retain(|t| t.state == TorrentState::Downloading),
            "seeding" => torrents.retain(|t| t.state == TorrentState::Seeding),
            "paused" => torrents.retain(|t| t.state == TorrentState::Paused),
            _ => {}
        }
    }

    // Search filter
    if let Some(search) = &query.search {
        let search_lower = search.to_lowercase();
        torrents.retain(|t| t.name.to_lowercase().contains(&search_lower));
    }

    // Sorting
    apply_sorting(&mut torrents, query);

    torrents
}

struct TorrentCounts {
    total: usize,
    downloading: usize,
    seeding: usize,
    paused: usize,
}

fn calculate_counts(torrents: &[Torrent]) -> TorrentCounts {
    TorrentCounts {
        total: torrents.len(),
        downloading: torrents
            .iter()
            .filter(|t| t.state == TorrentState::Downloading)
            .count(),
        seeding: torrents.iter().filter(|t| t.state == TorrentState::Seeding).count(),
        paused: torrents.iter().filter(|t| t.state == TorrentState::Paused).count(),
    }
}

fn apply_sorting(torrents: &mut [Torrent], query: &FilterQuery) {
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
                    let cmp = a
                        .progress_percent()
                        .partial_cmp(&b.progress_percent())
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
