use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::task::JoinSet;
use futures::StreamExt;
use uuid::Uuid;
use dowfn_shared::*;
use crate::error::*;
use crate::storage::Storage;
use crate::network::HttpClient;
use tracing::{info, error, debug, warn};

pub struct Downloader {
    task: Arc<RwLock<DownloadTask>>,
    http_client: Arc<HttpClient>,
    storage: Arc<Storage>,
    event_tx: mpsc::UnboundedSender<Response>,
    is_paused: Arc<AtomicBool>,
    is_cancelled: Arc<AtomicBool>,
    current_speed: Arc<AtomicU64>,
    worker_semaphore: Arc<Semaphore>,
}

impl Downloader {
    pub async fn new(
        task: Arc<RwLock<DownloadTask>>,
        http_client: Arc<HttpClient>,
        storage: Arc<Storage>,
        event_tx: mpsc::UnboundedSender<Response>,
    ) -> Self {
        let threads = task.read().await.threads;
        
        Self {
            task,
            http_client,
            storage,
            event_tx,
            is_paused: Arc::new(AtomicBool::new(false)),
            is_cancelled: Arc::new(AtomicBool::new(false)),
            current_speed: Arc::new(AtomicU64::new(0)),
            worker_semaphore: Arc::new(Semaphore::new(threads as usize)),
        }
    }
    
    pub async fn start(&self) -> Result<()> {
        let mut task = self.task.write().await;
        task.status = DownloadStatus::Connecting;
        let task_id = task.id;
        let url = task.url.clone();
        drop(task);
        
        info!("DOWNLOADER: Starting download for task {}, URL: {}", task_id, url);
        self.send_status_update(task_id, DownloadStatus::Connecting).await;
        
        info!("DOWNLOADER: Sending HEAD request to {}", url);
        let head_response = match self.http_client.head(&url).await {
            Ok(response) => {
                info!("DOWNLOADER: HEAD request successful for {}", url);
                response
            }
            Err(e) => {
                error!("DOWNLOADER: HEAD request failed for {}: {}", url, e);
                error!("DOWNLOADER: Falling back to GET request to determine file info");
                
                let get_response = match self.http_client.get_with_headers(&url, vec![]).await {
                    Ok(response) => response,
                    Err(get_e) => {
                        error!("DOWNLOADER: GET fallback also failed: {}", get_e);
                        return Err(get_e);
                    }
                };
                
                let headers = get_response.headers();
                crate::network::HeadResponse {
                    content_length: headers
                        .get(reqwest::header::CONTENT_LENGTH)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse().ok()),
                    content_type: headers
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string()),
                    content_disposition: headers
                        .get(reqwest::header::CONTENT_DISPOSITION)
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string()),
                    etag: headers
                        .get(reqwest::header::ETAG)
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string()),
                    last_modified: headers
                        .get(reqwest::header::LAST_MODIFIED)
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string()),
                    accept_ranges: headers
                        .get(reqwest::header::ACCEPT_RANGES)
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.contains("bytes"))
                        .unwrap_or(false),
                }
            }
        };
        let content_disposition = head_response.content_disposition.clone();
        let headers = HttpHeaders {
            content_type: head_response.content_type,
            content_disposition: head_response.content_disposition,
            etag: head_response.etag,
            last_modified: head_response.last_modified,
            accept_ranges: head_response.accept_ranges,
        };
        
        let total_size = head_response.content_length;
        let resume_support = head_response.accept_ranges;
        
        let mut task = self.task.write().await;
        task.headers = Some(headers);
        task.total_size = total_size;
        task.resume_support = resume_support;
        
        if let Some(content_disposition) = &content_disposition {
            if let Some(filename) = Self::extract_filename_from_header(content_disposition) {
                task.file_name = filename;
                task.save_path = task.save_path.parent().unwrap().join(&task.file_name);
            }
        }
        
        let can_multithread = resume_support && total_size.is_some();
        let threads = if can_multithread { task.threads } else { 1 };
        
        info!("DOWNLOADER: File info - Size: {:?}, Resume support: {}, Threads: {}", total_size, resume_support, threads);
        
        if can_multithread && total_size.is_some() {
            let size = total_size.unwrap();
            task.chunk_info = self.create_chunks(size, threads as usize);
            info!("DOWNLOADER: Using multithread download with {} chunks", task.chunk_info.len());
        } else {
            task.chunk_info = vec![ChunkInfo {
                index: 0,
                start: 0,
                end: total_size.unwrap_or(0),
                downloaded: 0,
                status: ChunkStatus::Pending,
                worker_id: None,
            }];
            info!("DOWNLOADER: Using single thread download");
        }
        
        task.status = DownloadStatus::Downloading;
        drop(task);
        
        self.send_status_update(task_id, DownloadStatus::Downloading).await;
        
        let result = if can_multithread {
            info!("DOWNLOADER: Starting multithread download");
            self.download_multithread().await
        } else {
            info!("DOWNLOADER: Starting single thread download");
            self.download_single_thread().await
        };
        
        match &result {
            Ok(_) => info!("DOWNLOADER: Download completed successfully"),
            Err(e) => error!("DOWNLOADER: Download failed: {}", e),
        }
        
        result
    }
    
    async fn download_multithread(&self) -> Result<()> {
        let mut join_set = JoinSet::new();
        let task = self.task.read().await;
        let chunks = task.chunk_info.clone();
        let url = task.url.clone();
        let task_id = task.id;
        drop(task);
        
        let speed_tracker = Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::<usize, u64>::new()));
        let speed_tracker_clone = speed_tracker.clone();
        let current_speed_clone = self.current_speed.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                interval.tick().await;
                let speeds = speed_tracker_clone.read().await;
                let total_speed: u64 = speeds.values().sum();
                current_speed_clone.store(total_speed, Ordering::Relaxed);
            }
        });
        
        for (index, chunk) in chunks.iter().enumerate() {
            if chunk.status == ChunkStatus::Completed {
                continue;
            }
            
            let http_client = self.http_client.clone();
            let storage = self.storage.clone();
            let task = self.task.clone();
            let event_tx = self.event_tx.clone();
            let is_paused = self.is_paused.clone();
            let is_cancelled = self.is_cancelled.clone();
            let current_speed = self.current_speed.clone();
            let semaphore = self.worker_semaphore.clone();
            let speed_tracker = speed_tracker.clone();
            let url = url.clone();
            let chunk = chunk.clone();
            
            join_set.spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                Self::download_chunk_with_speed_tracking(
                    index,
                    chunk,
                    url,
                    task_id,
                    http_client,
                    storage,
                    task,
                    event_tx,
                    is_paused,
                    is_cancelled,
                    current_speed,
                    speed_tracker,
                ).await
            });
        }
        
        while let Some(result) = join_set.join_next().await {
            if self.is_cancelled.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            
            match result {
                Ok(Ok(_)) => {
                    info!("DOWNLOADER: Chunk completed successfully");
                },
                Ok(Err(e)) => {
                    error!("DOWNLOADER: Chunk failed with error: {}", e);
                    join_set.shutdown().await;
                    
                    let mut task = self.task.write().await;
                    task.status = DownloadStatus::Failed(e.to_string());
                    drop(task);
                    
                    self.send_status_update(task_id, DownloadStatus::Failed(e.to_string())).await;
                    return Err(e);
                }
                Err(e) => {
                    error!("DOWNLOADER: Chunk join error: {}", e);
                    join_set.shutdown().await;
                    
                    let mut task = self.task.write().await;
                    let error_msg = e.to_string();
                    task.status = DownloadStatus::Failed(error_msg.clone());
                    drop(task);
                    
                    self.send_status_update(task_id, DownloadStatus::Failed(error_msg)).await;
                    return Err(DownloadError::Internal(e.to_string()));
                }
            }
        }
        
        // Verify all chunks completed before merging
        let task = self.task.read().await;
        let all_completed = task.chunk_info.iter().all(|chunk| chunk.status == ChunkStatus::Completed);
        let total_downloaded: u64 = task.chunk_info.iter().map(|chunk| chunk.downloaded).sum();
        let expected_total = task.total_size.unwrap_or(0);
        drop(task);
        
        if !all_completed {
            let task = self.task.read().await;
            let incomplete_chunks: Vec<usize> = task.chunk_info.iter()
                .filter(|chunk| chunk.status != ChunkStatus::Completed)
                .map(|chunk| chunk.index)
                .collect();
            drop(task);
            error!("DOWNLOADER: Not all chunks completed. Incomplete chunks: {:?}", incomplete_chunks);
            return Err(DownloadError::Internal(format!("Incomplete chunks: {:?}", incomplete_chunks)));
        }
        
        if total_downloaded != expected_total {
            error!("DOWNLOADER: Total downloaded mismatch: got {} bytes, expected {} bytes", 
                   total_downloaded, expected_total);
            return Err(DownloadError::Internal(format!(
                "Downloaded size mismatch: got {} bytes, expected {} bytes", 
                total_downloaded, expected_total
            )));
        }
        
        info!("DOWNLOADER: All chunks verified, starting merge");
        self.merge_chunks().await?;
        self.verify_download().await?;
        
        let mut task = self.task.write().await;
        task.status = DownloadStatus::Completed;
        drop(task);
        
        self.send_status_update(task_id, DownloadStatus::Completed).await;
        
        Ok(())
    }
    
    async fn download_single_thread(&self) -> Result<()> {
        let task = self.task.read().await;
        let url = task.url.clone();
        let task_id = task.id;
        let save_path = task.save_path.clone();
        drop(task);
        
        info!("DOWNLOADER: Getting stream for single thread download: {}", url);
        let mut response = match self.http_client.get_stream(&url).await {
            Ok(response) => {
                info!("DOWNLOADER: Stream obtained successfully");
                response
            }
            Err(e) => {
                error!("DOWNLOADER: Failed to get stream: {}", e);
                return Err(e);
            }
        };
        let temp_file = self.storage.create_temp_file(task_id, 0).await?;
        info!("DOWNLOADER: Temp file created: {:?}", temp_file);
        
        let mut downloaded = 0u64;
        let mut last_progress_update = std::time::Instant::now();
        let mut speed_calculator = SpeedCalculator::new();
        
        while let Some(chunk_result) = response.next().await {
            if self.is_cancelled.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            
            while self.is_paused.load(Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                if self.is_cancelled.load(Ordering::Relaxed) {
                    return Err(DownloadError::Cancelled);
                }
            }
            
            let chunk = chunk_result.map_err(DownloadError::Network)?;
            let chunk_size = chunk.len() as u64;
            
            self.storage.write_chunk(&temp_file, &chunk).await?;
            downloaded += chunk_size;
            
            let speed = speed_calculator.update(chunk_size);
            self.current_speed.store(speed, Ordering::Relaxed);
            
            if last_progress_update.elapsed() >= std::time::Duration::from_millis(100) {
                self.send_progress_update(task_id, downloaded, None, speed).await;
                last_progress_update = std::time::Instant::now();
            }
            
            let mut task = self.task.write().await;
            task.downloaded_size = downloaded;
            if !task.chunk_info.is_empty() {
                task.chunk_info[0].downloaded = downloaded;
            }
            drop(task);
        }
        
        self.storage.finalize_download(task_id, &save_path).await?;
        
        let mut task = self.task.write().await;
        task.status = DownloadStatus::Completed;
        drop(task);
        
        self.send_status_update(task_id, DownloadStatus::Completed).await;
        
        Ok(())
    }
    
    async fn download_chunk_with_speed_tracking(
        index: usize,
        mut chunk: ChunkInfo,
        url: String,
        task_id: Uuid,
        http_client: Arc<HttpClient>,
        storage: Arc<Storage>,
        task: Arc<RwLock<DownloadTask>>,
        event_tx: mpsc::UnboundedSender<Response>,
        is_paused: Arc<AtomicBool>,
        is_cancelled: Arc<AtomicBool>,
        _current_speed: Arc<AtomicU64>,
        speed_tracker: Arc<tokio::sync::RwLock<std::collections::HashMap<usize, u64>>>,
    ) -> Result<()> {
        let max_retries = 3;
        let mut retry_count = 0;
        
        while retry_count <= max_retries {
            if is_cancelled.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            
            match Self::try_download_chunk_with_speed_tracking(
                index,
                &mut chunk,
                &url,
                task_id,
                &http_client,
                &storage,
                &task,
                &event_tx,
                &is_paused,
                &is_cancelled,
                &speed_tracker,
            ).await {
                Ok(_) => {
                    if retry_count > 0 {
                        info!("DOWNLOADER: Chunk {} succeeded on retry {} after previous failures", index, retry_count + 1);
                    }
                    return Ok(());
                },
                Err(e) => {
                    warn!("DOWNLOADER: Chunk {} attempt {} failed: {}", index, retry_count + 1, e);
                    retry_count += 1;
                    
                    if retry_count > max_retries {
                        error!("DOWNLOADER: Chunk {} failed after {} attempts. Final error: {}", index, max_retries + 1, e);
                        return Err(e);
                    }
                    
                    // Exponential backoff: 1s, 2s, 4s
                    let delay = std::time::Duration::from_secs(1 << (retry_count - 1));
                    info!("DOWNLOADER: Retrying chunk {} in {} seconds...", index, delay.as_secs());
                    tokio::time::sleep(delay).await;
                }
            }
        }
        
        Err(DownloadError::MaxRetriesExceeded)
    }
    
    async fn try_download_chunk_with_speed_tracking(
        index: usize,
        chunk: &mut ChunkInfo,
        url: &str,
        task_id: Uuid,
        http_client: &HttpClient,
        storage: &Storage,
        task: &Arc<RwLock<DownloadTask>>,
        event_tx: &mpsc::UnboundedSender<Response>,
        is_paused: &Arc<AtomicBool>,
        is_cancelled: &Arc<AtomicBool>,
        speed_tracker: &Arc<tokio::sync::RwLock<std::collections::HashMap<usize, u64>>>,
    ) -> Result<()> {
        chunk.status = ChunkStatus::Downloading;
        chunk.worker_id = Some(index);
        
        let range_start = chunk.start + chunk.downloaded;
        let range_end = chunk.end;
        
        let mut response = match http_client.get_range_stream(url, range_start, range_end).await {
            Ok(r) => r,
            Err(e) => {
                error!("DOWNLOADER: Failed to get range stream for chunk {}: {}", index, e);
                return Err(e);
            }
        };
        let temp_file = storage.create_temp_file(task_id, index).await?;
        
        if chunk.downloaded > 0 {
            storage.seek_temp_file(&temp_file, chunk.downloaded).await?;
        }
        
        let mut speed_calculator = SpeedCalculator::new();
        let mut last_progress_update = std::time::Instant::now();
        
        while let Some(data_result) = response.next().await {
            if is_cancelled.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            
            while is_paused.load(Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                if is_cancelled.load(Ordering::Relaxed) {
                    return Err(DownloadError::Cancelled);
                }
            }
            
            let data = match data_result {
                Ok(d) => d,
                Err(e) => {
                    error!("DOWNLOADER: Network error in chunk {}: {}", index, e);
                    return Err(DownloadError::Network(e));
                }
            };
            let data_size = data.len() as u64;
            
            if let Err(e) = storage.write_chunk(&temp_file, &data).await {
                error!("DOWNLOADER: Failed to write chunk {} data: {}", index, e);
                return Err(e);
            }
            chunk.downloaded += data_size;
            
            let chunk_speed = speed_calculator.update(data_size);
            {
                let mut speeds = speed_tracker.write().await;
                speeds.insert(index, chunk_speed);
            }
            
            let mut task_guard = task.write().await;
            task_guard.chunk_info[index] = chunk.clone();
            let total_downloaded = task_guard.chunk_info.iter()
                .map(|c| c.downloaded)
                .sum();
            task_guard.downloaded_size = total_downloaded;
            let total_size = task_guard.total_size;
            drop(task_guard);
            
            if last_progress_update.elapsed() >= std::time::Duration::from_millis(500) {
                let speeds = speed_tracker.read().await;
                let total_speed: u64 = speeds.values().sum();
                let _ = event_tx.send(Response::Progress(ProgressUpdate {
                    download_id: task_id,
                    downloaded: total_downloaded,
                    total: total_size,
                    speed: total_speed,
                    time_remaining: total_size.map(|t| {
                        if total_speed > 0 && t > total_downloaded {
                            (t - total_downloaded) / total_speed
                        } else {
                            0
                        }
                    }),
                    active_chunks: Vec::new(),
                }));
                last_progress_update = std::time::Instant::now();
            }
        }
        
        // Verify chunk is fully downloaded
        let expected_size = chunk.end - chunk.start + 1;
        if chunk.downloaded != expected_size {
            error!("DOWNLOADER: Chunk {} incomplete: downloaded {} bytes, expected {} bytes", 
                   index, chunk.downloaded, expected_size);
            return Err(DownloadError::Internal(format!(
                "Chunk {} incomplete: downloaded {} bytes, expected {} bytes", 
                index, chunk.downloaded, expected_size
            )));
        }
        
        chunk.status = ChunkStatus::Completed;
        let mut task_guard = task.write().await;
        task_guard.chunk_info[index] = chunk.clone();
        drop(task_guard);
        
        {
            let mut speeds = speed_tracker.write().await;
            speeds.remove(&index);
        }
        
        info!("DOWNLOADER: Chunk {} completed successfully", index);
        Ok(())
    }
    
    async fn download_chunk(
        index: usize,
        mut chunk: ChunkInfo,
        url: String,
        task_id: Uuid,
        http_client: Arc<HttpClient>,
        storage: Arc<Storage>,
        task: Arc<RwLock<DownloadTask>>,
        event_tx: mpsc::UnboundedSender<Response>,
        is_paused: Arc<AtomicBool>,
        is_cancelled: Arc<AtomicBool>,
        current_speed: Arc<AtomicU64>,
    ) -> Result<()> {
        chunk.status = ChunkStatus::Downloading;
        chunk.worker_id = Some(index);
        
        let range_start = chunk.start + chunk.downloaded;
        let range_end = chunk.end;
        
        let mut response = http_client.get_range_stream(&url, range_start, range_end).await?;
        let temp_file = storage.create_temp_file(task_id, index).await?;
        
        if chunk.downloaded > 0 {
            storage.seek_temp_file(&temp_file, chunk.downloaded).await?;
        }
        
        let mut speed_calculator = SpeedCalculator::new();
        let mut last_progress_update = std::time::Instant::now();
        
        while let Some(data_result) = response.next().await {
            if is_cancelled.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            
            while is_paused.load(Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                if is_cancelled.load(Ordering::Relaxed) {
                    return Err(DownloadError::Cancelled);
                }
            }
            
            let data = data_result.map_err(DownloadError::Network)?;
            let data_size = data.len() as u64;
            
            storage.write_chunk(&temp_file, &data).await?;
            chunk.downloaded += data_size;
            
            let _speed = speed_calculator.update(data_size);
            
            let mut task_guard = task.write().await;
            task_guard.chunk_info[index] = chunk.clone();
            let total_downloaded = task_guard.chunk_info.iter()
                .map(|c| c.downloaded)
                .sum();
            task_guard.downloaded_size = total_downloaded;
            let total_size = task_guard.total_size;
            drop(task_guard);
            
            if last_progress_update.elapsed() >= std::time::Duration::from_millis(200) {
                let _ = event_tx.send(Response::Progress(ProgressUpdate {
                    download_id: task_id,
                    downloaded: total_downloaded,
                    total: total_size,
                    speed: current_speed.load(Ordering::Relaxed),
                    time_remaining: total_size.map(|t| {
                        let speed = current_speed.load(Ordering::Relaxed);
                        if speed > 0 && t > total_downloaded {
                            (t - total_downloaded) / speed
                        } else {
                            0
                        }
                    }),
                    active_chunks: Vec::new(),
                }));
                last_progress_update = std::time::Instant::now();
            }
        }
        
        chunk.status = ChunkStatus::Completed;
        let mut task_guard = task.write().await;
        task_guard.chunk_info[index] = chunk;
        drop(task_guard);
        
        Ok(())
    }
    
    async fn merge_chunks(&self) -> Result<()> {
        let task = self.task.read().await;
        let task_id = task.id;
        let save_path = task.save_path.clone();
        let chunks = task.chunk_info.clone();
        drop(task);
        
        let mut task = self.task.write().await;
        task.status = DownloadStatus::Merging;
        drop(task);
        
        self.send_status_update(task_id, DownloadStatus::Merging).await;
        
        self.storage.merge_chunks(task_id, &save_path, chunks.len()).await?;
        
        Ok(())
    }
    
    async fn verify_download(&self) -> Result<()> {
        let task = self.task.read().await;
        
        if let Some(hash) = &task.hash {
            let mut task_mut = self.task.write().await;
            task_mut.status = DownloadStatus::Verifying;
            drop(task_mut);
            
            self.send_status_update(task.id, DownloadStatus::Verifying).await;
            
            let calculated_hash = self.storage.calculate_hash(&task.save_path, &hash.algorithm).await?;
            
            if calculated_hash != hash.value {
                return Err(DownloadError::ChecksumMismatch);
            }
            
            let mut task_mut = self.task.write().await;
            if let Some(h) = &mut task_mut.hash {
                h.verified = true;
            }
        }
        
        Ok(())
    }
    
    fn create_chunks(&self, total_size: u64, threads: usize) -> Vec<ChunkInfo> {
        let chunk_size = total_size / threads as u64;
        let mut chunks = Vec::new();
        
        for i in 0..threads {
            let start = i as u64 * chunk_size;
            let end = if i == threads - 1 {
                total_size - 1
            } else {
                (i + 1) as u64 * chunk_size - 1
            };
            
            chunks.push(ChunkInfo {
                index: i,
                start,
                end,
                downloaded: 0,
                status: ChunkStatus::Pending,
                worker_id: None,
            });
        }
        
        chunks
    }
    
    fn extract_filename_from_header(header: &str) -> Option<String> {
        if let Some(filename_part) = header.split("filename=").nth(1) {
            let filename = filename_part.trim_matches('"').trim_matches('\'');
            return Some(filename.to_string());
        }
        None
    }
    
    async fn send_progress_update(&self, id: Uuid, downloaded: u64, total: Option<u64>, speed: u64) {
        let _ = self.event_tx.send(Response::Progress(ProgressUpdate {
            download_id: id,
            downloaded,
            total,
            speed,
            time_remaining: total.map(|t| {
                if speed > 0 && t > downloaded {
                    (t - downloaded) / speed
                } else {
                    0
                }
            }),
            active_chunks: Vec::new(),
        }));
    }
    
    async fn send_status_update(&self, id: Uuid, new_status: DownloadStatus) {
        let _ = self.event_tx.send(Response::StatusChange(StatusUpdate {
            download_id: id,
            old_status: DownloadStatus::Pending,
            new_status,
            message: None,
        }));
    }
    
    pub async fn pause(&self) -> Result<()> {
        self.is_paused.store(true, Ordering::Relaxed);
        Ok(())
    }
    
    pub async fn cancel(&self) -> Result<()> {
        self.is_cancelled.store(true, Ordering::Relaxed);
        Ok(())
    }
    
    pub async fn get_current_speed(&self) -> u64 {
        self.current_speed.load(Ordering::Relaxed)
    }
}

struct SpeedCalculator {
    last_time: std::time::Instant,
    bytes_since_last: u64,
}

impl SpeedCalculator {
    fn new() -> Self {
        Self {
            last_time: std::time::Instant::now(),
            bytes_since_last: 0,
        }
    }
    
    fn update(&mut self, bytes: u64) -> u64 {
        self.bytes_since_last += bytes;
        let elapsed = self.last_time.elapsed();
        
        if elapsed >= std::time::Duration::from_secs(1) {
            let speed = self.bytes_since_last; // bytes per second
            self.bytes_since_last = 0;
            self.last_time = std::time::Instant::now();
            speed
        } else if elapsed.as_millis() > 0 {
            // Estimate current speed based on elapsed time
            (self.bytes_since_last * 1000) / elapsed.as_millis() as u64
        } else {
            0
        }
    }
}