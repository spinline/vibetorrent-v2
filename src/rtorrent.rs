//! rTorrent SCGI Client
//! 
//! This module implements the SCGI protocol to communicate with rTorrent's
//! XML-RPC interface over a Unix socket.

use bytes::{BufMut, BytesMut};
use quick_xml::{Reader, Writer, events::{Event, BytesStart, BytesText, BytesEnd}};
use std::io::Cursor;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::error::{AppError, Result};

#[derive(Debug, Clone)]
pub struct RtorrentClient {
    socket_path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Torrent {
    pub hash: String,
    pub name: String,
    pub size_bytes: i64,
    pub completed_bytes: i64,
    pub down_rate: i64,
    pub up_rate: i64,
    pub state: TorrentState,
    pub ratio: f64,
    pub is_active: bool,
    pub is_open: bool,
    pub is_hashing: bool,
    pub complete: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum TorrentState {
    Downloading,
    Seeding,
    Paused,
    Hashing,
    Error,
}

impl Torrent {
    pub fn progress_percent(&self) -> f64 {
        if self.size_bytes == 0 {
            0.0
        } else {
            (self.completed_bytes as f64 / self.size_bytes as f64) * 100.0
        }
    }
    
    pub fn size_formatted(&self) -> String {
        format_bytes(self.size_bytes)
    }
    
    pub fn down_rate_formatted(&self) -> String {
        format!("{}/s", format_bytes(self.down_rate))
    }
    
    pub fn up_rate_formatted(&self) -> String {
        format!("{}/s", format_bytes(self.up_rate))
    }
    
    pub fn eta(&self) -> Option<String> {
        if self.complete || self.down_rate == 0 {
            return None;
        }
        let remaining = self.size_bytes - self.completed_bytes;
        let seconds = remaining / self.down_rate;
        Some(format_duration(seconds))
    }
    
    pub fn status_text(&self) -> &'static str {
        match self.state {
            TorrentState::Downloading => "Downloading",
            TorrentState::Seeding => "Seeding",
            TorrentState::Paused => "Paused",
            TorrentState::Hashing => "Hashing",
            TorrentState::Error => "Error",
        }
    }
    
    pub fn progress_bar_class(&self) -> &'static str {
        match self.state {
            TorrentState::Downloading => "bg-blue-500",
            TorrentState::Seeding => "bg-green-500",
            TorrentState::Paused => "bg-orange-500",
            TorrentState::Hashing => "bg-yellow-500",
            TorrentState::Error => "bg-red-500",
        }
    }
}

fn format_bytes(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = KB * 1024;
    const GB: i64 = MB * 1024;
    const TB: i64 = GB * 1024;
    
    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn format_duration(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    
    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GlobalStats {
    pub down_rate: i64,
    pub up_rate: i64,
    pub free_disk_space: i64,
    pub active_peers: i64,
}

impl GlobalStats {
    pub fn down_rate_formatted(&self) -> String {
        format!("{}/s", format_bytes(self.down_rate))
    }
    
    pub fn up_rate_formatted(&self) -> String {
        format!("{}/s", format_bytes(self.up_rate))
    }
    
    pub fn free_disk_formatted(&self) -> String {
        format_bytes(self.free_disk_space)
    }
}

impl RtorrentClient {
    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }
    
    /// Test connection to rtorrent by attempting to connect to the socket
    pub async fn test_connection(&self) -> bool {
        self.connect().await.is_ok()
    }
    
    async fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket_path)
            .await
            .map_err(|e| AppError::RtorrentConnection(format!(
                "Failed to connect to {}: {}", self.socket_path, e
            )))
    }
    
    async fn send_request(&self, xml_body: &str) -> Result<String> {
        let mut stream = self.connect().await?;
        
        // Build SCGI request
        let content_length = xml_body.len();
        let headers = format!(
            "CONTENT_LENGTH\0{}\0SCGI\01\0REQUEST_METHOD\0POST\0REQUEST_URI\0/RPC2\0",
            content_length
        );
        
        // Netstring format: length:content,
        let mut request = BytesMut::new();
        request.put_slice(format!("{}:", headers.len()).as_bytes());
        request.put_slice(headers.as_bytes());
        request.put_u8(b',');
        request.put_slice(xml_body.as_bytes());
        
        // Send request
        stream.write_all(&request).await
            .map_err(|e| AppError::ScgiError(format!("Write error: {}", e)))?;
        
        // Read response
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await
            .map_err(|e| AppError::ScgiError(format!("Read error: {}", e)))?;
        
        // Parse HTTP response - skip headers
        let response_str = String::from_utf8_lossy(&response);
        let body_start = response_str.find("\r\n\r\n")
            .or_else(|| response_str.find("\n\n"))
            .map(|i| if response_str[i..].starts_with("\r\n") { i + 4 } else { i + 2 })
            .unwrap_or(0);
        
        Ok(response_str[body_start..].to_string())
    }
    
    fn build_multicall_xml(method: &str, params: &[&str]) -> String {
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        
        // Start methodCall
        writer.write_event(Event::Start(BytesStart::new("methodCall"))).unwrap();
        
        // methodName
        writer.write_event(Event::Start(BytesStart::new("methodName"))).unwrap();
        writer.write_event(Event::Text(BytesText::new(method))).unwrap();
        writer.write_event(Event::End(BytesEnd::new("methodName"))).unwrap();
        
        // params
        writer.write_event(Event::Start(BytesStart::new("params"))).unwrap();
        
        // First param (empty string for d.multicall2)
        writer.write_event(Event::Start(BytesStart::new("param"))).unwrap();
        writer.write_event(Event::Start(BytesStart::new("value"))).unwrap();
        writer.write_event(Event::Start(BytesStart::new("string"))).unwrap();
        writer.write_event(Event::End(BytesEnd::new("string"))).unwrap();
        writer.write_event(Event::End(BytesEnd::new("value"))).unwrap();
        writer.write_event(Event::End(BytesEnd::new("param"))).unwrap();
        
        // Second param (view name)
        writer.write_event(Event::Start(BytesStart::new("param"))).unwrap();
        writer.write_event(Event::Start(BytesStart::new("value"))).unwrap();
        writer.write_event(Event::Start(BytesStart::new("string"))).unwrap();
        writer.write_event(Event::Text(BytesText::new("main"))).unwrap();
        writer.write_event(Event::End(BytesEnd::new("string"))).unwrap();
        writer.write_event(Event::End(BytesEnd::new("value"))).unwrap();
        writer.write_event(Event::End(BytesEnd::new("param"))).unwrap();
        
        // Additional method params
        for param in params {
            writer.write_event(Event::Start(BytesStart::new("param"))).unwrap();
            writer.write_event(Event::Start(BytesStart::new("value"))).unwrap();
            writer.write_event(Event::Start(BytesStart::new("string"))).unwrap();
            writer.write_event(Event::Text(BytesText::new(param))).unwrap();
            writer.write_event(Event::End(BytesEnd::new("string"))).unwrap();
            writer.write_event(Event::End(BytesEnd::new("value"))).unwrap();
            writer.write_event(Event::End(BytesEnd::new("param"))).unwrap();
        }
        
        writer.write_event(Event::End(BytesEnd::new("params"))).unwrap();
        writer.write_event(Event::End(BytesEnd::new("methodCall"))).unwrap();
        
        let result = writer.into_inner().into_inner();
        format!("<?xml version=\"1.0\"?>\n{}", String::from_utf8(result).unwrap())
    }
    
    fn build_simple_xml(method: &str) -> String {
        format!(
            r#"<?xml version="1.0"?>
<methodCall>
<methodName>{}</methodName>
<params/>
</methodCall>"#,
            method
        )
    }
    
    fn build_single_param_xml(method: &str, param: &str) -> String {
        format!(
            r#"<?xml version="1.0"?>
<methodCall>
<methodName>{}</methodName>
<params>
<param><value><string>{}</string></value></param>
</params>
</methodCall>"#,
            method, param
        )
    }
    
    pub async fn get_torrents(&self) -> Result<Vec<Torrent>> {
        let xml = Self::build_multicall_xml(
            "d.multicall2",
            &[
                "d.hash=",
                "d.name=",
                "d.size_bytes=",
                "d.completed_bytes=",
                "d.down.rate=",
                "d.up.rate=",
                "d.is_active=",
                "d.is_open=",
                "d.is_hash_checking=",
                "d.complete=",
                "d.message=",
                "d.ratio=",
            ],
        );
        
        tracing::debug!("get_torrents request XML: {}", xml);
        let response = self.send_request(&xml).await?;
        tracing::debug!("get_torrents response: {}", response);
        self.parse_torrents_response(&response)
    }
    
    fn parse_torrents_response(&self, xml: &str) -> Result<Vec<Torrent>> {
        let mut torrents = Vec::new();
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        
        let mut current_values: Vec<String> = Vec::new();
        let mut in_value_tag = false;
        let mut value_collected = false;
        let mut in_array = false;
        let mut array_depth = 0;
        let mut buf = Vec::new();
        
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    match e.name().as_ref() {
                        b"array" => {
                            array_depth += 1;
                            if array_depth == 2 {
                                in_array = true;
                                current_values.clear();
                            }
                        }
                        b"i4" | b"i8" | b"int" | b"string" | b"double" => {
                            if in_array {
                                in_value_tag = true;
                                value_collected = false;
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(e)) => {
                    match e.name().as_ref() {
                        b"array" => {
                            if array_depth == 2 && current_values.len() >= 12 {
                                // Parse torrent from values
                                let is_active = current_values[6].parse::<i64>().unwrap_or(0) == 1;
                                let is_open = current_values[7].parse::<i64>().unwrap_or(0) == 1;
                                let is_hashing = current_values[8].parse::<i64>().unwrap_or(0) == 1;
                                let complete = current_values[9].parse::<i64>().unwrap_or(0) == 1;
                                
                                let state = if is_hashing {
                                    TorrentState::Hashing
                                } else if !current_values[10].is_empty() && current_values[10] != "0" {
                                    TorrentState::Error
                                } else if !is_active && !is_open {
                                    TorrentState::Paused
                                } else if !is_active {
                                    TorrentState::Paused
                                } else if complete {
                                    TorrentState::Seeding
                                } else {
                                    TorrentState::Downloading
                                };
                                
                                torrents.push(Torrent {
                                    hash: current_values[0].clone(),
                                    name: current_values[1].clone(),
                                    size_bytes: current_values[2].parse().unwrap_or(0),
                                    completed_bytes: current_values[3].parse().unwrap_or(0),
                                    down_rate: current_values[4].parse().unwrap_or(0),
                                    up_rate: current_values[5].parse().unwrap_or(0),
                                    is_active,
                                    is_open,
                                    is_hashing,
                                    complete,
                                    message: current_values[10].clone(),
                                    ratio: current_values[11].parse::<f64>().unwrap_or(0.0) / 1000.0,
                                    state,
                                });
                            }
                            array_depth -= 1;
                            if array_depth < 2 {
                                in_array = false;
                            }
                        }
                        b"i4" | b"i8" | b"int" | b"string" | b"double" => {
                            // If we're closing a value tag and no value was collected, add empty string
                            if in_value_tag && !value_collected && in_array {
                                current_values.push(String::new());
                            }
                            in_value_tag = false;
                            value_collected = false;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(e)) => {
                    if in_value_tag && in_array {
                        current_values.push(e.unescape().unwrap_or_default().to_string());
                        value_collected = true;
                    }
                }
                Ok(Event::Empty(e)) => {
                    // Handle empty tags like <string/>
                    if in_array {
                        match e.name().as_ref() {
                            b"string" | b"i4" | b"i8" | b"int" | b"double" => {
                                current_values.push(String::new());
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(AppError::XmlRpcError(format!("XML parse error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }
        
        tracing::debug!("Parsed {} torrents", torrents.len());
        for t in &torrents {
            tracing::debug!("Torrent: {} - {}", t.hash, t.name);
        }
        
        Ok(torrents)
    }
    
    pub async fn get_global_stats(&self) -> Result<GlobalStats> {
        // Get download rate
        let down_xml = Self::build_simple_xml("throttle.global_down.rate");
        let down_response = self.send_request(&down_xml).await?;
        let down_rate = self.parse_int_response(&down_response).unwrap_or(0);
        
        // Get upload rate
        let up_xml = Self::build_simple_xml("throttle.global_up.rate");
        let up_response = self.send_request(&up_xml).await?;
        let up_rate = self.parse_int_response(&up_response).unwrap_or(0);
        
        // Get free disk space
        let _disk_xml = Self::build_simple_xml("system.files.status_failures");
        let free_disk_space = 2_000_000_000_000i64; // 2TB placeholder - would need actual path
        
        // Count active peers (simplified)
        let active_peers = 0i64;
        
        Ok(GlobalStats {
            down_rate,
            up_rate,
            free_disk_space,
            active_peers,
        })
    }
    
    fn parse_int_response(&self, xml: &str) -> Option<i64> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        let mut in_value = false;
        
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    match e.name().as_ref() {
                        b"i4" | b"i8" | b"int" => in_value = true,
                        _ => {}
                    }
                }
                Ok(Event::Text(e)) if in_value => {
                    return e.unescape().ok()?.parse().ok();
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
        None
    }
    
    pub async fn get_client_version(&self) -> Result<String> {
        let xml = Self::build_simple_xml("system.client_version");
        let response = self.send_request(&xml).await?;
        self.parse_string_response(&response)
            .ok_or_else(|| AppError::XmlRpcError("Failed to parse version".to_string()))
    }

    fn parse_string_response(&self, xml: &str) -> Option<String> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        let mut in_string = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    if e.name().as_ref() == b"string" {
                        in_string = true;
                    }
                }
                Ok(Event::Text(e)) if in_string => {
                    return e.unescape().ok().map(|s| s.to_string());
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
        None
    }

    pub async fn pause_torrent(&self, hash: &str) -> Result<()> {
        let xml = Self::build_single_param_xml("d.stop", hash);
        self.send_request(&xml).await?;
        let xml = Self::build_single_param_xml("d.close", hash);
        self.send_request(&xml).await?;
        Ok(())
    }
    
    pub async fn resume_torrent(&self, hash: &str) -> Result<()> {
        let xml = Self::build_single_param_xml("d.open", hash);
        self.send_request(&xml).await?;
        let xml = Self::build_single_param_xml("d.start", hash);
        self.send_request(&xml).await?;
        Ok(())
    }
    
    pub async fn remove_torrent(&self, hash: &str) -> Result<()> {
        let xml = Self::build_single_param_xml("d.erase", hash);
        self.send_request(&xml).await?;
        Ok(())
    }
    
    pub async fn add_torrent_url(&self, url: &str) -> Result<()> {
        tracing::info!("Adding torrent from URL: {}", url);
        // Escape XML special characters in the URL
        let escaped_url = url
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;");
        // load.start needs empty string as first param (for view), then the URL
        let xml = format!(
            r#"<?xml version="1.0"?>
<methodCall>
<methodName>load.start</methodName>
<params>
<param><value><string></string></value></param>
<param><value><string>{}</string></value></param>
</params>
</methodCall>"#,
            escaped_url
        );
        let response = self.send_request(&xml).await?;
        tracing::debug!("add_torrent_url response: {}", response);
        Ok(())
    }
    
    pub async fn add_torrent_file(&self, data: &[u8]) -> Result<()> {
        tracing::info!("Adding torrent from file, size: {} bytes", data.len());
        // For file uploads, we use load.raw_start with base64 encoded data
        let encoder = base64_encode(data);
        let xml = format!(
            r#"<?xml version="1.0"?>
<methodCall>
<methodName>load.raw_start</methodName>
<params>
<param><value><string></string></value></param>
<param><value><base64>{}</base64></value></param>
</params>
</methodCall>"#,
            encoder
        );
        let response = self.send_request(&xml).await?;
        tracing::debug!("add_torrent_file response: {}", response);
        Ok(())
    }
}

fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    
    for chunk in data.chunks(3) {
        let mut n: u32 = 0;
        for (i, &byte) in chunk.iter().enumerate() {
            n |= (byte as u32) << (16 - i * 8);
        }
        
        result.push(ALPHABET[(n >> 18 & 0x3F) as usize] as char);
        result.push(ALPHABET[(n >> 12 & 0x3F) as usize] as char);
        
        if chunk.len() > 1 {
            result.push(ALPHABET[(n >> 6 & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        
        if chunk.len() > 2 {
            result.push(ALPHABET[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    
    result
}
