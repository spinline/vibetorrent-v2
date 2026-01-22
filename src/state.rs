use tokio::sync::RwLock;
use std::collections::HashSet;

use crate::rtorrent::RtorrentClient;

pub struct AppState {
    pub rtorrent: RtorrentClient,
    pub starred_torrents: RwLock<HashSet<String>>,
}

impl AppState {
    pub fn new(scgi_socket: String) -> Self {
        Self {
            rtorrent: RtorrentClient::new(scgi_socket),
            starred_torrents: RwLock::new(HashSet::new()),
        }
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
}
