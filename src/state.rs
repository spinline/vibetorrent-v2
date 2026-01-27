use tokio::sync::{broadcast, watch, RwLock};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

use crate::rtorrent::RtorrentClient;
use crate::rtorrent::{GlobalStats, Torrent};

pub struct AppState {
    pub rtorrent: RtorrentClient,
    pub starred_torrents: RwLock<HashSet<String>>,

    torrents_tx: broadcast::Sender<Arc<Vec<Torrent>>>,
    stats_tx: broadcast::Sender<Arc<GlobalStats>>,

    last_torrents: Arc<RwLock<Option<Arc<Vec<Torrent>>>>>,
    last_stats: Arc<RwLock<Option<Arc<GlobalStats>>>>,

    shutdown_tx: watch::Sender<bool>,
}

impl AppState {
    pub fn new(scgi_socket: String) -> Self {
        let (torrents_tx, _torrents_rx) = broadcast::channel(16);
        let (stats_tx, _stats_rx) = broadcast::channel(16);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let state = Self {
            rtorrent: RtorrentClient::new(scgi_socket),
            starred_torrents: RwLock::new(HashSet::new()),

            torrents_tx,
            stats_tx,

            last_torrents: Arc::new(RwLock::new(None)),
            last_stats: Arc::new(RwLock::new(None)),

            shutdown_tx,
        };

        state.spawn_poller(shutdown_rx);
        state
    }
    
    pub async fn is_starred(&self, hash: &str) -> bool {
        self.starred_torrents.read().await.contains(hash)
    }
    
    pub async fn toggle_star(&self, hash: &str) -> bool {
        let mut starred = self.starred_torrents.write().await;
        if starred.contains(hash) {
            starred.remove(hash);
            false
        } else {
            starred.insert(hash.to_string());
            true
        }
    }

    pub fn subscribe_torrents(&self) -> broadcast::Receiver<Arc<Vec<Torrent>>> {
        self.torrents_tx.subscribe()
    }

    pub fn subscribe_stats(&self) -> broadcast::Receiver<Arc<GlobalStats>> {
        self.stats_tx.subscribe()
    }

    pub async fn latest_torrents(&self) -> Option<Arc<Vec<Torrent>>> {
        self.last_torrents.read().await.clone()
    }

    pub async fn latest_stats(&self) -> Option<Arc<GlobalStats>> {
        self.last_stats.read().await.clone()
    }

    /// Refresh the torrent cache immediately and broadcast to SSE clients.
    /// Call this after torrent operations (add/remove/pause/resume) to update UI instantly.
    pub async fn refresh_cache(&self) {
        match self.rtorrent.get_torrents().await {
            Ok(torrents) => {
                let snapshot = Arc::new(torrents);
                *self.last_torrents.write().await = Some(snapshot.clone());
                let _ = self.torrents_tx.send(snapshot);
            }
            Err(err) => {
                tracing::warn!("refresh_cache: get_torrents failed: {}", err);
            }
        }
    }

    fn spawn_poller(&self, mut shutdown_rx: watch::Receiver<bool>) {
        let rtorrent = self.rtorrent.clone();
        let torrents_tx = self.torrents_tx.clone();
        let stats_tx = self.stats_tx.clone();
        let last_torrents = self.last_torrents.clone();
        let last_stats = self.last_stats.clone();

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(2));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let need_torrents = torrents_tx.receiver_count() > 0;
                        let need_stats = stats_tx.receiver_count() > 0;

                        // Always fetch torrents to get accurate speed data
                        let torrents_result = rtorrent.get_torrents().await;
                        
                        if let Ok(ref torrents) = torrents_result {
                            if need_torrents {
                                let snapshot = Arc::new(torrents.clone());
                                *last_torrents.write().await = Some(snapshot.clone());
                                let _ = torrents_tx.send(snapshot);
                            }
                            
                            // Calculate global rates from individual torrent rates
                            if need_stats {
                                let total_down_rate: i64 = torrents.iter().map(|t| t.down_rate).sum();
                                let total_up_rate: i64 = torrents.iter().map(|t| t.up_rate).sum();
                                
                                // Get disk space from the first torrent if available
                                let free_disk_space = torrents.first()
                                    .map(|t| t.free_disk_space)
                                    .unwrap_or(0);
                                
                                // Get base stats and add calculated values
                                match rtorrent.get_global_stats().await {
                                    Ok(mut stats) => {
                                        stats.down_rate = total_down_rate;
                                        stats.up_rate = total_up_rate;
                                        if free_disk_space > 0 {
                                            stats.free_disk_space = free_disk_space;
                                        }
                                        let snapshot = Arc::new(stats);
                                        *last_stats.write().await = Some(snapshot.clone());
                                        let _ = stats_tx.send(snapshot);
                                    }
                                    Err(err) => {
                                        tracing::warn!("poller: get_global_stats failed: {}", err);
                                    }
                                }
                            }
                        } else if let Err(err) = torrents_result {
                            tracing::warn!("poller: get_torrents failed: {}", err);
                        }
                    }
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        if *shutdown_rx.borrow() {
                            break;
                        }
                    }
                }
            }
        });
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}
