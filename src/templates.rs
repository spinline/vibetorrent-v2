use askama::Template;
use crate::rtorrent::{Torrent, GlobalStats, TorrentState};

#[derive(Template)]
#[template(path = "base.html")]
pub struct BaseTemplate {
    pub title: String,
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub stats: GlobalStats,
    pub torrents: Vec<TorrentView>,
    pub filter: String,
    pub total_count: usize,
    pub downloading_count: usize,
    pub seeding_count: usize,
    pub paused_count: usize,
    pub rtorrent_version: String,
}

#[derive(Template)]
#[template(path = "partials/torrent_list.html")]
pub struct TorrentListTemplate {
    pub torrents: Vec<TorrentView>,
    pub filter: String,
    pub total_count: usize,
}

#[derive(Template)]
#[template(path = "partials/stats.html")]
pub struct StatsTemplate {
    pub stats: GlobalStats,
}

#[derive(Template)]
#[template(path = "partials/torrent_row.html")]
pub struct TorrentRowTemplate {
    pub torrent: TorrentView,
}

#[derive(Template)]
#[template(path = "partials/add_torrent_modal.html")]
pub struct AddTorrentModalTemplate;

#[derive(Template)]
#[template(path = "partials/sidebar_counts.html")]
pub struct SidebarCountsTemplate {
    pub total_count: usize,
    pub downloading_count: usize,
    pub seeding_count: usize,
    pub paused_count: usize,
}

/// View model for torrent display
#[derive(Clone)]
pub struct TorrentView {
    pub hash: String,
    pub name: String,
    pub size: String,
    pub progress: f64,
    pub progress_rounded: i32,
    pub status: String,
    pub status_class: String,
    pub progress_bar_class: String,
    pub down_rate: String,
    pub up_rate: String,
    pub eta: String,
    pub is_paused: bool,
    pub is_starred: bool,
    pub state: TorrentState,
}

impl TorrentView {
    pub fn from_torrent(torrent: &Torrent, is_starred: bool) -> Self {
        let progress = torrent.progress_percent();
        Self {
            hash: torrent.hash.clone(),
            name: torrent.name.clone(),
            size: torrent.size_formatted(),
            progress,
            progress_rounded: progress.round() as i32,
            status: torrent.status_text().to_string(),
            status_class: torrent.status_class().to_string(),
            progress_bar_class: torrent.progress_bar_class().to_string(),
            down_rate: torrent.down_rate_formatted(),
            up_rate: torrent.up_rate_formatted(),
            eta: torrent.eta().unwrap_or_else(|| "∞".to_string()),
            is_paused: torrent.state == TorrentState::Paused,
            is_starred,
            state: torrent.state,
        }
    }
}
