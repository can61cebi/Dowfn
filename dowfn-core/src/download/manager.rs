use std::sync::Arc;
use std::path::{Path, PathBuf};
use tokio::sync::{mpsc, RwLock, Semaphore};
use dashmap::DashMap;
use uuid::Uuid;
use chrono::Utc;
use dowfn_shared::*;
use crate::error::*;
use crate::download::downloader::Downloader;
use crate::storage::Storage;
use crate::network::HttpClient;
use tracing::{info, error};

pub struct DownloadManager {
    downloads: Arc<DashMap<Uuid, Arc<RwLock<DownloadTask>>>>,
    active_downloaders: Arc<DashMap<Uuid, Arc<Downloader>>>,
    storage: Arc<Storage>,
    http_client: Arc<HttpClient>,
    config: Arc<RwLock<AppConfig>>,
    semaphore: Arc<Semaphore>,
    event_tx: mpsc::UnboundedSender<Response>,
    #[allow(dead_code)]  // Geçici olarak uyarıyı sustur
    shutdown_tx: mpsc::Sender<()>,
}

impl DownloadManager {
    pub async fn new(
        config: AppConfig,
        event_tx: mpsc::UnboundedSender<Response>,
    ) -> Result<Self> {
        let storage = Arc::new(Storage::new(&config.temp_path, &config.default_save_path).await?);
        let http_client = Arc::new(HttpClient::new()?);
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_downloads as usize));
        let (shutdown_tx, _) = mpsc::channel(1);
        
        Ok(Self {
            downloads: Arc::new(DashMap::new()),
            active_downloaders: Arc::new(DashMap::new()),
            storage: storage.clone(),
            http_client,
            config: Arc::new(RwLock::new(config)),
            semaphore,
            event_tx,
            shutdown_tx,
        })
    }
    
    pub async fn add_download(&self, request: AddDownloadRequest) -> Result<Uuid> {
        info!("MANAGER: Processing add_download request for URL: {}", request.url);
        let url = url::Url::parse(&request.url)
            .map_err(|e| {
                error!("MANAGER: Invalid URL format: {} - Error: {}", request.url, e);
                DownloadError::InvalidUrl(e.to_string())
            })?;
        
        let config = self.config.read().await;
        let file_name = request.file_name.unwrap_or_else(|| {
            Self::extract_filename_from_url(&url)
        });
        
        let save_path = request.save_path
            .map(PathBuf::from)
            .unwrap_or_else(|| config.default_save_path.join(&file_name));
        
        if save_path.exists() && !request.metadata.as_ref().map_or(false, |m| m.overwrite) {
            if request.metadata.as_ref().map_or(true, |m| m.auto_rename) {
                let _save_path = self.generate_unique_filename(&save_path).await;
            } else {
                return Err(DownloadError::FileExists(save_path.display().to_string()));
            }
        }
        
        let task = DownloadTask {
            id: Uuid::new_v4(),
            url: request.url.clone(),
            file_name,
            save_path,
            total_size: None,
            downloaded_size: 0,
            status: DownloadStatus::Pending,
            threads: request.threads.unwrap_or(config.default_threads),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            resume_support: false,
            chunk_info: Vec::new(),
            headers: None,
            hash: None,
            retry_count: 0,
            max_retries: config.max_retries,
            metadata: request.metadata.unwrap_or_default(),
        };
        
        let task_id = task.id;
        self.downloads.insert(task_id, Arc::new(RwLock::new(task.clone())));
        
        self.storage.save_task(&task).await?;
        
        let _ = self.event_tx.send(Response::Success(SuccessResponse {
            message: format!("Download added: {}", task.file_name),
            download_id: Some(task_id),
        }));
        
        if request.auto_start {
            info!("MANAGER: Auto-starting download for task: {}", task_id);
            match self.start_download(task_id).await {
                Ok(_) => info!("MANAGER: Download started successfully for task: {}", task_id),
                Err(e) => error!("MANAGER: Failed to start download for task {}: {}", task_id, e),
            }
        }
        
        // Send updated download list to UI
        let all_downloads = self.get_all_downloads().await;
        let _ = self.event_tx.send(Response::DownloadList(all_downloads));
        
        info!("MANAGER: Add download completed successfully, task ID: {}", task_id);
        Ok(task_id)
    }
    
    pub async fn start_download(&self, id: Uuid) -> Result<()> {
        info!("MANAGER: Starting download for task: {}", id);
        let task = self.downloads.get(&id)
            .ok_or_else(|| {
                error!("MANAGER: Task not found: {}", id);
                DownloadError::NotFound(id)
            })?
            .clone();
        
        let mut task_guard = task.write().await;
        
        match task_guard.status {
            DownloadStatus::Downloading | DownloadStatus::Connecting => {
                return Ok(());
            }
            DownloadStatus::Completed => {
                return Err(DownloadError::NotSupported("Download already completed".to_string()));
            }
            _ => {}
        }
        
        task_guard.status = DownloadStatus::Connecting;
        task_guard.started_at = Some(Utc::now());
        drop(task_guard);
        
        let permit = self.semaphore.clone().acquire_owned().await.unwrap();
        
        let downloader = Arc::new(Downloader::new(
            task.clone(),
            self.http_client.clone(),
            self.storage.clone(),
            self.event_tx.clone(),
        ).await);
        
        self.active_downloaders.insert(id, downloader.clone());
        
        let active_downloaders = self.active_downloaders.clone();
        let downloads = self.downloads.clone();
        let event_tx = self.event_tx.clone();
        
        tokio::spawn(async move {
            let result = downloader.start().await;
            
            active_downloaders.remove(&id);
            drop(permit);
            
            if let Some(task) = downloads.get(&id) {
                let mut task_guard = task.write().await;
                match result {
                    Ok(_) => {
                        task_guard.status = DownloadStatus::Completed;
                        task_guard.completed_at = Some(Utc::now());
                    }
                    Err(e) => {
                        task_guard.status = DownloadStatus::Failed(e.to_string());
                    }
                }
                drop(task_guard);
                
                // Send updated download list to UI after status change
                let mut all_downloads = Vec::new();
                for entry in downloads.iter() {
                    all_downloads.push(entry.value().read().await.clone());
                }
                let _ = event_tx.send(Response::DownloadList(all_downloads));
            }
        });
        
        Ok(())
    }
    
    pub async fn pause_download(&self, id: Uuid) -> Result<()> {
        if let Some(downloader) = self.active_downloaders.get(&id) {
            downloader.pause().await?;
        }
        
        if let Some(task) = self.downloads.get(&id) {
            let mut task_guard = task.write().await;
            task_guard.status = DownloadStatus::Paused;
        }
        
        // Send updated download list to UI
        let all_downloads = self.get_all_downloads().await;
        let _ = self.event_tx.send(Response::DownloadList(all_downloads));
        
        Ok(())
    }
    
    pub async fn resume_download(&self, id: Uuid) -> Result<()> {
        if let Some(task) = self.downloads.get(&id) {
            let task_guard = task.read().await;
            if task_guard.status == DownloadStatus::Paused {
                drop(task_guard);
                self.start_download(id).await?;
            }
        }
        
        // Send updated download list to UI
        let all_downloads = self.get_all_downloads().await;
        let _ = self.event_tx.send(Response::DownloadList(all_downloads));
        
        Ok(())
    }
    
    pub async fn cancel_download(&self, id: Uuid) -> Result<()> {
        if let Some(downloader) = self.active_downloaders.get(&id) {
            downloader.cancel().await?;
            self.active_downloaders.remove(&id);
        }
        
        if let Some(task) = self.downloads.get(&id) {
            let mut task_guard = task.write().await;
            task_guard.status = DownloadStatus::Cancelled;
        }
        
        self.storage.cleanup_temp_files(id).await?;
        
        // Send updated download list to UI
        let all_downloads = self.get_all_downloads().await;
        let _ = self.event_tx.send(Response::DownloadList(all_downloads));
        
        Ok(())
    }
    
    pub async fn get_download(&self, id: Uuid) -> Result<DownloadTask> {
        let task = self.downloads.get(&id)
            .ok_or(DownloadError::NotFound(id))?;
        let task = task.read().await.clone();
        Ok(task)
    }
    
    pub async fn get_all_downloads(&self) -> Vec<DownloadTask> {
        let mut tasks = Vec::new();
        for entry in self.downloads.iter() {
            tasks.push(entry.value().read().await.clone());
        }
        tasks
    }
    
    pub async fn remove_download(&self, id: Uuid) -> Result<()> {
        self.cancel_download(id).await?;
        self.downloads.remove(&id);
        self.storage.remove_task(id).await?;
        
        // Send updated download list to UI
        let all_downloads = self.get_all_downloads().await;
        let _ = self.event_tx.send(Response::DownloadList(all_downloads));
        
        Ok(())
    }
    
    pub async fn clear_completed(&self) -> Result<()> {
        let mut to_remove = Vec::new();
        
        for entry in self.downloads.iter() {
            let task = entry.value().read().await;
            if task.status == DownloadStatus::Completed {
                to_remove.push(task.id);
            }
        }
        
        for id in to_remove {
            self.downloads.remove(&id);
            self.storage.remove_task(id).await?;
        }
        
        // Send updated download list to UI
        let all_downloads = self.get_all_downloads().await;
        let _ = self.event_tx.send(Response::DownloadList(all_downloads));
        
        Ok(())
    }
    
    pub async fn get_statistics(&self) -> GlobalStatistics {
        let mut stats = GlobalStatistics {
            total_downloads: self.downloads.len() as u64,
            active_downloads: 0,
            completed_downloads: 0,
            failed_downloads: 0,
            total_bytes_downloaded: 0,
            session_bytes_downloaded: 0,
            current_download_speed: 0,
            current_upload_speed: 0,
        };
        
        for entry in self.downloads.iter() {
            let task = entry.value().read().await;
            match task.status {
                DownloadStatus::Downloading => stats.active_downloads += 1,
                DownloadStatus::Completed => stats.completed_downloads += 1,
                DownloadStatus::Failed(_) => stats.failed_downloads += 1,
                _ => {}
            }
            stats.total_bytes_downloaded += task.downloaded_size;
        }
        
        for downloader in self.active_downloaders.iter() {
            stats.current_download_speed += downloader.get_current_speed().await;
        }
        
        stats
    }
    
    fn extract_filename_from_url(url: &url::Url) -> String {
        url.path_segments()
            .and_then(|segments| segments.last())
            .filter(|s| !s.is_empty())
            .unwrap_or("download")
            .to_string()
    }
    
    async fn generate_unique_filename(&self, path: &Path) -> PathBuf {
        let mut counter = 1;
        let stem = path.file_stem().unwrap_or_default();
        let extension = path.extension();
        let parent = path.parent().unwrap_or(Path::new("."));
        
        loop {
            let new_name = if let Some(ext) = extension {
                format!("{}_{}.{}", stem.to_string_lossy(), counter, ext.to_string_lossy())
            } else {
                format!("{}_{}", stem.to_string_lossy(), counter)
            };
            
            let new_path = parent.join(new_name);
            if !new_path.exists() {
                return new_path;
            }
            counter += 1;
        }
    }
}