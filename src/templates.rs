use askama::Template;
use crate::rtorrent::{Torrent, GlobalStats, TorrentState};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[allow(unused_imports)]
use TorrentState as _TS; // Used in from_torrent comparison

// Cache version - auto-generated on app start for cache busting
pub static CACHE_VERSION: LazyLock<String> = LazyLock::new(|| {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "1".to_string())
});

#[derive(Template)]
#[template(path = "setup.html")]
pub struct SetupTemplate {
    pub scgi_socket: String,
    pub bind_address: String,
    pub error: Option<String>,
    pub cache_version: String,
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub stats: GlobalStats,
    pub torrents: Vec<TorrentView>,
    pub total_count: usize,
    pub downloading_count: usize,
    pub seeding_count: usize,
    pub paused_count: usize,
    pub rtorrent_version: String,
    pub cache_version: String,
}

#[derive(Template)]
#[template(path = "partials/torrent_list.html")]
pub struct TorrentListTemplate {
    pub torrents: Vec<TorrentView>,
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

/// OOB template for updating only dynamic torrent fields via SSE
/// This prevents flickering by only updating: progress, status, speeds, eta
/// Static fields (name, size, star) are NOT touched
#[derive(Template)]
#[template(source = r#"<div id="progress-{{ torrent.hash }}" class="col-span-2 flex items-center gap-3" hx-swap-oob="true"><div class="flex-1 h-2 bg-bg-secondary rounded-full overflow-hidden"><div class="{{ torrent.progress_bar_class }} h-full rounded-full transition-all duration-300" style="width: {{ torrent.progress }}%"></div></div><span class="text-xs text-text-muted w-12 text-right">{{ torrent.progress_rounded }}%</span></div>
<div id="status-{{ torrent.hash }}" class="col-span-1 flex items-center justify-center" hx-swap-oob="true"><span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium {% if torrent.is_paused %}bg-orange-500/10 text-orange-400{% else %}{% if torrent.status == "Seeding" %}bg-emerald-500/10 text-emerald-400{% else %}{% if torrent.status == "Downloading" %}bg-blue-500/10 text-blue-400{% else %}{% if torrent.status == "Hashing" %}bg-yellow-500/10 text-yellow-400{% else %}{% if torrent.status == "Error" %}bg-red-500/10 text-red-400{% endif %}{% endif %}{% endif %}{% endif %}{% endif %}">{% if torrent.is_paused %}<svg class="w-3 h-3" fill="currentColor" viewBox="0 0 24 24"><path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z"/></svg>{% else %}{% if torrent.status == "Seeding" %}<svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 10l7-7m0 0l7 7m-7-7v18"/></svg>{% else %}{% if torrent.status == "Downloading" %}<svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 14l-7 7m0 0l-7-7m7 7V3"/></svg>{% else %}{% if torrent.status == "Error" %}<svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/></svg>{% endif %}{% endif %}{% endif %}{% endif %}{{ torrent.status }}</span></div>
<div id="down-{{ torrent.hash }}" class="col-span-1 text-right text-text-muted text-xs flex items-center justify-end" hx-swap-oob="true">{{ torrent.down_rate }}</div>
<div id="up-{{ torrent.hash }}" class="col-span-1 text-right text-text-muted text-xs flex items-center justify-end" hx-swap-oob="true">{{ torrent.up_rate }}</div>
<span id="eta-{{ torrent.hash }}" class="text-text-muted text-xs" hx-swap-oob="true">{{ torrent.eta }}</span>"#, ext = "html")]
pub struct TorrentOobTemplate {
    pub torrent: TorrentView,
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
    pub progress_bar_class: String,
    pub down_rate: String,
    pub up_rate: String,
    pub eta: String,
    pub ratio: String,
    pub is_paused: bool,
    pub is_starred: bool,
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
            progress_bar_class: torrent.progress_bar_class().to_string(),
            down_rate: torrent.down_rate_formatted(),
            up_rate: torrent.up_rate_formatted(),
            eta: torrent.eta().unwrap_or_else(|| "∞".to_string()),
            ratio: format!("{:.1}", torrent.ratio),
            is_paused: torrent.state == TorrentState::Paused,
            is_starred,
        }
    }
}
