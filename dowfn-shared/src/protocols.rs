use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::models::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Command {
    AddDownload(AddDownloadRequest),
    PauseDownload(Uuid),
    ResumeDownload(Uuid),
    CancelDownload(Uuid),
    RemoveDownload(Uuid),
    RetryDownload(Uuid),
    SetSpeedLimit(SetSpeedLimitRequest),
    UpdateConfig(AppConfig),
    GetDownloadInfo(Uuid),
    GetAllDownloads,
    GetStatistics,
    ClearCompleted,
    PauseAll,
    ResumeAll,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddDownloadRequest {
    pub url: String,
    pub save_path: Option<String>,
    pub file_name: Option<String>,
    pub threads: Option<u8>,
    pub metadata: Option<DownloadMetadata>,
    pub auto_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSpeedLimitRequest {
    pub download_id: Option<Uuid>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Response {
    Success(SuccessResponse),
    Error(ErrorResponse),
    DownloadInfo(DownloadTask),
    DownloadList(Vec<DownloadTask>),
    Statistics(GlobalStatistics),
    Progress(ProgressUpdate),
    StatusChange(StatusUpdate),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessResponse {
    pub message: String,
    pub download_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorCode {
    InvalidUrl,
    NetworkError,
    FileSystemError,
    PermissionDenied,
    InsufficientSpace,
    DownloadNotFound,
    AlreadyExists,
    InvalidConfiguration,
    InternalError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub download_id: Uuid,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: u64,
    pub time_remaining: Option<u64>,
    pub active_chunks: Vec<ChunkProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkProgress {
    pub index: usize,
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusUpdate {
    pub download_id: Uuid,
    pub old_status: DownloadStatus,
    pub new_status: DownloadStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum ExtensionMessage {
    Handshake(HandshakeRequest),
    Command(Command),
    Subscribe(SubscribeRequest),
    Unsubscribe(Uuid),
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeRequest {
    pub version: String,
    pub client_id: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    pub download_id: Option<Uuid>,
    pub events: Vec<EventType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    Progress,
    StatusChange,
    Completion,
    Error,
    Statistics,
}