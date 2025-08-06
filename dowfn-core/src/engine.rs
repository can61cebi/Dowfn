use std::sync::Arc;
use tokio::sync::mpsc;
use dowfn_shared::*;
use crate::download::DownloadManager;
use crate::error::*;
use tracing::{info, error, debug};

pub struct DownloadEngine {
    manager: Arc<DownloadManager>,
    command_rx: mpsc::UnboundedReceiver<Command>,
    event_tx: mpsc::UnboundedSender<Response>,
}

impl DownloadEngine {
    pub async fn new(
        config: AppConfig,
    ) -> Result<(Self, mpsc::UnboundedSender<Command>, mpsc::UnboundedReceiver<Response>)> {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        
        let manager = Arc::new(DownloadManager::new(config, event_tx.clone()).await?);
        
        let engine = Self {
            manager,
            command_rx,
            event_tx,
        };
        
        Ok((engine, command_tx, event_rx))
    }
    
    pub async fn run(mut self) {
        info!("ENGINE: Starting download engine");
        while let Some(command) = self.command_rx.recv().await {
            debug!("ENGINE: Received command: {:?}", std::mem::discriminant(&command));
            let result = self.handle_command(command).await;
            
            if let Err(e) = result {
                error!("ENGINE: Command handling failed: {}", e);
                let _ = self.event_tx.send(Response::Error(ErrorResponse {
                    code: e.to_error_code(),
                    message: e.to_string(),
                    details: None,
                }));
            }
        }
        info!("ENGINE: Download engine stopped");
    }
    
    async fn handle_command(&self, command: Command) -> Result<()> {
        match command {
            Command::AddDownload(request) => {
                info!("ENGINE: Adding download for URL: {}", request.url);
                let id = self.manager.add_download(request).await?;
                info!("ENGINE: Download added successfully with ID: {}", id);
                self.event_tx.send(Response::Success(SuccessResponse {
                    message: "Download added successfully".to_string(),
                    download_id: Some(id),
                })).map_err(|_| DownloadError::Internal("Failed to send response".to_string()))?;
            }
            
            Command::PauseDownload(id) => {
                self.manager.pause_download(id).await?;
                self.event_tx.send(Response::Success(SuccessResponse {
                    message: "Download paused".to_string(),
                    download_id: Some(id),
                })).map_err(|_| DownloadError::Internal("Failed to send response".to_string()))?;
            }
            
            Command::ResumeDownload(id) => {
                self.manager.resume_download(id).await?;
                self.event_tx.send(Response::Success(SuccessResponse {
                    message: "Download resumed".to_string(),
                    download_id: Some(id),
                })).map_err(|_| DownloadError::Internal("Failed to send response".to_string()))?;
            }
            
            Command::CancelDownload(id) => {
                self.manager.cancel_download(id).await?;
                self.event_tx.send(Response::Success(SuccessResponse {
                    message: "Download cancelled".to_string(),
                    download_id: Some(id),
                })).map_err(|_| DownloadError::Internal("Failed to send response".to_string()))?;
            }
            
            Command::RemoveDownload(id) => {
                self.manager.remove_download(id).await?;
                self.event_tx.send(Response::Success(SuccessResponse {
                    message: "Download removed".to_string(),
                    download_id: Some(id),
                })).map_err(|_| DownloadError::Internal("Failed to send response".to_string()))?;
            }
            
            Command::GetDownloadInfo(id) => {
                let task = self.manager.get_download(id).await?;
                self.event_tx.send(Response::DownloadInfo(task))
                    .map_err(|_| DownloadError::Internal("Failed to send response".to_string()))?;
            }
            
            Command::GetAllDownloads => {
                let tasks = self.manager.get_all_downloads().await;
                self.event_tx.send(Response::DownloadList(tasks))
                    .map_err(|_| DownloadError::Internal("Failed to send response".to_string()))?;
            }
            
            Command::GetStatistics => {
                let stats = self.manager.get_statistics().await;
                self.event_tx.send(Response::Statistics(stats))
                    .map_err(|_| DownloadError::Internal("Failed to send response".to_string()))?;
            }
            
            Command::ClearCompleted => {
                self.manager.clear_completed().await?;
                self.event_tx.send(Response::Success(SuccessResponse {
                    message: "Completed downloads cleared".to_string(),
                    download_id: None,
                })).map_err(|_| DownloadError::Internal("Failed to send response".to_string()))?;
            }
            
            _ => {
                return Err(DownloadError::NotSupported("Command not implemented".to_string()));
            }
        }
        
        Ok(())
    }
}