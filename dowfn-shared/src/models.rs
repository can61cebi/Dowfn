use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
    Pending,
    Connecting,
    Downloading,
    Paused,
    Completed,
    Failed(String),
    Cancelled,
    Merging,
    Verifying,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: Uuid,
    pub url: String,
    pub file_name: String,
    pub save_path: PathBuf,
    pub total_size: Option<u64>,
    pub downloaded_size: u64,
    pub status: DownloadStatus,
    pub threads: u8,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub resume_support: bool,
    pub chunk_info: Vec<ChunkInfo>,
    pub headers: Option<HttpHeaders>,
    pub hash: Option<FileHash>,
    pub retry_count: u32,
    pub max_retries: u32,
    pub metadata: DownloadMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    pub index: usize,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
    pub status: ChunkStatus,
    pub worker_id: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChunkStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
    Retrying,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpHeaders {
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub accept_ranges: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHash {
    pub algorithm: HashAlgorithm,
    pub value: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HashAlgorithm {
    SHA256,
    SHA512,
    BLAKE3,
    MD5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadMetadata {
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
    pub cookies: Option<String>,
    pub authentication: Option<AuthInfo>,
    pub proxy: Option<ProxyConfig>,
    pub speed_limit: Option<u64>,
    pub timeout: u64,
    pub auto_rename: bool,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthInfo {
    pub auth_type: AuthType,
    pub credentials: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthType {
    Basic,
    Bearer,
    ApiKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub proxy_type: ProxyType,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProxyType {
    HTTP,
    HTTPS,
    SOCKS5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadStatistics {
    pub download_speed: u64,
    pub upload_speed: u64,
    pub average_speed: u64,
    pub peak_speed: u64,
    pub time_remaining: Option<u64>,
    pub active_connections: u32,
    pub total_connections: u32,
    pub bytes_wasted: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalStatistics {
    pub total_downloads: u64,
    pub active_downloads: u32,
    pub completed_downloads: u64,
    pub failed_downloads: u64,
    pub total_bytes_downloaded: u64,
    pub session_bytes_downloaded: u64,
    pub current_download_speed: u64,
    pub current_upload_speed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub max_concurrent_downloads: u32,
    pub max_connections_per_download: u32,
    pub default_threads: u8,
    pub chunk_size: u64,
    pub buffer_size: usize,
    pub retry_delay: u64,
    pub max_retries: u32,
    pub timeout: u64,
    pub auto_start: bool,
    pub default_save_path: PathBuf,
    pub temp_path: PathBuf,
    pub enable_speed_limit: bool,
    pub global_speed_limit: Option<u64>,
    pub enable_scheduling: bool,
    pub schedule_config: Option<ScheduleConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    pub start_time: String,
    pub end_time: String,
    pub days: Vec<Weekday>,
    pub speed_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Default for DownloadMetadata {
    fn default() -> Self {
        Self {
            referrer: None,
            user_agent: Some("Dowfn/0.1.0".to_string()),
            cookies: None,
            authentication: None,
            proxy: None,
            speed_limit: None,
            timeout: 30000,
            auto_rename: true,
            overwrite: false,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 3,
            max_connections_per_download: 8,
            default_threads: 4,
            chunk_size: 1024 * 1024 * 2,
            buffer_size: 8192,
            retry_delay: 5000,
            max_retries: 3,
            timeout: 30000,
            auto_start: true,
            default_save_path: dirs::download_dir().unwrap_or_else(|| PathBuf::from(".")),
            temp_path: std::env::temp_dir().join("dowfn"),
            enable_speed_limit: false,
            global_speed_limit: None,
            enable_scheduling: false,
            schedule_config: None,
        }
    }
}