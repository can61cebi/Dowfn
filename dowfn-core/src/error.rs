use thiserror::Error;
use dowfn_shared::ErrorCode;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    
    #[error("File already exists: {0}")]
    FileExists(String),
    
    #[error("Insufficient disk space: need {needed} bytes, available {available} bytes")]
    InsufficientSpace { needed: u64, available: u64 },
    
    #[error("Download not found: {0}")]
    NotFound(uuid::Uuid),
    
    #[error("Operation not supported: {0}")]
    NotSupported(String),
    
    #[error("Download cancelled")]
    Cancelled,
    
    #[error("Maximum retries exceeded")]
    MaxRetriesExceeded,
    
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    
    #[error("Authentication failed")]
    AuthenticationFailed,
    
    #[error("Checksum verification failed")]
    ChecksumMismatch,
    
    #[error("Internal error: {0}")]
    Internal(String),
}

impl DownloadError {
    pub fn to_error_code(&self) -> ErrorCode {
        match self {
            Self::Network(_) => ErrorCode::NetworkError,
            Self::Io(_) => ErrorCode::FileSystemError,
            Self::InvalidUrl(_) => ErrorCode::InvalidUrl,
            Self::FileExists(_) => ErrorCode::AlreadyExists,
            Self::InsufficientSpace { .. } => ErrorCode::InsufficientSpace,
            Self::NotFound(_) => ErrorCode::DownloadNotFound,
            Self::AuthenticationFailed => ErrorCode::PermissionDenied,
            _ => ErrorCode::InternalError,
        }
    }
}

pub type Result<T> = std::result::Result<T, DownloadError>;