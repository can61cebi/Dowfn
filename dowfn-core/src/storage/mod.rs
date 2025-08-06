use std::path::{Path, PathBuf};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncSeekExt};
use uuid::Uuid;
use bytes::Bytes;
use blake3::Hasher;
use dowfn_shared::{DownloadTask, HashAlgorithm};
use crate::error::*;

pub struct Storage {
    temp_dir: PathBuf,
    download_dir: PathBuf,
    metadata_dir: PathBuf,
}

impl Storage {
    pub async fn new(temp_dir: &Path, download_dir: &Path) -> Result<Self> {
        fs::create_dir_all(temp_dir).await?;
        fs::create_dir_all(download_dir).await?;
        
        let metadata_dir = temp_dir.join("metadata");
        fs::create_dir_all(&metadata_dir).await?;
        
        Ok(Self {
            temp_dir: temp_dir.to_path_buf(),
            download_dir: download_dir.to_path_buf(),
            metadata_dir,
        })
    }
    
    pub async fn cleanup_temp_files(&self, task_id: Uuid) -> Result<()> {
        let task_dir = self.temp_dir.join(task_id.to_string());
        
        if task_dir.exists() {
            fs::remove_dir_all(task_dir).await?;
        }
        
        Ok(())
    }
    
    pub async fn save_task(&self, task: &DownloadTask) -> Result<()> {
        let metadata_file = self.metadata_dir.join(format!("{}.json", task.id));
        let json = serde_json::to_string_pretty(task)
            .map_err(|e| DownloadError::Internal(e.to_string()))?;
        
        fs::write(metadata_file, json).await?;
        
        Ok(())
    }
    
    pub async fn load_task(&self, task_id: Uuid) -> Result<DownloadTask> {
        let metadata_file = self.metadata_dir.join(format!("{}.json", task_id));
        
        let json = fs::read_to_string(metadata_file).await?;
        let task = serde_json::from_str(&json)
            .map_err(|e| DownloadError::Internal(e.to_string()))?;
        
        Ok(task)
    }
    
    pub async fn remove_task(&self, task_id: Uuid) -> Result<()> {
        let metadata_file = self.metadata_dir.join(format!("{}.json", task_id));
        
        if metadata_file.exists() {
            fs::remove_file(metadata_file).await?;
        }
        
        Ok(())
    }
    
    pub async fn load_all_tasks(&self) -> Result<Vec<DownloadTask>> {
        let mut tasks = Vec::new();
        
        let mut entries = fs::read_dir(&self.metadata_dir).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
                let json = fs::read_to_string(entry.path()).await?;
                if let Ok(task) = serde_json::from_str::<DownloadTask>(&json) {
                    tasks.push(task);
                }
            }
        }
        
        Ok(tasks)
    }
    
    pub async fn calculate_hash(&self, file_path: &Path, _algorithm: &HashAlgorithm) -> Result<String> {
        let mut file = File::open(file_path).await?;
        let mut hasher = Hasher::new();
        
        let mut buffer = vec![0u8; 8192];
        
        loop {
            let bytes_read = file.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        
        let hash = hasher.finalize();
        Ok(hash.to_hex().to_string())
    }
    
    pub async fn get_available_space(&self, path: &Path) -> Result<u64> {
        #[cfg(windows)]
        {
            use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
            use windows::core::PCWSTR;
            use std::ffi::OsStr;
            use std::os::windows::ffi::OsStrExt;
            
            let path_str: Vec<u16> = OsStr::new(path.to_str().unwrap())
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            
            let mut free_bytes = 0u64;
            
            unsafe {
                GetDiskFreeSpaceExW(
                    PCWSTR(path_str.as_ptr()),
                    Some(&mut free_bytes),
                    None,
                    None,
                ).map_err(|e| DownloadError::Internal(e.to_string()))?;
            }
            
            Ok(free_bytes)
        }
        
        #[cfg(not(windows))]
        {
            Ok(u64::MAX)
        }
    }
    
    pub async fn ensure_space(&self, required: u64) -> Result<()> {
        let available = self.get_available_space(&self.download_dir).await?;
        
        if available < required {
            return Err(DownloadError::InsufficientSpace {
                needed: required,
                available,
            });
        }
        
        Ok(())
    }
    
    pub async fn create_temp_file(&self, task_id: Uuid, chunk_index: usize) -> Result<PathBuf> {
        let task_dir = self.temp_dir.join(task_id.to_string());
        fs::create_dir_all(&task_dir).await?;
        
        let temp_file = task_dir.join(format!("chunk_{:04}.tmp", chunk_index));
        File::create(&temp_file).await?;
        
        Ok(temp_file)
    }
    
    pub async fn write_chunk(&self, file_path: &Path, data: &Bytes) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(file_path)
            .await?;
        
        file.write_all(data).await?;
        file.flush().await?;
        
        Ok(())
    }
    
    pub async fn seek_temp_file(&self, file_path: &Path, position: u64) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(file_path)
            .await?;
        
        file.seek(std::io::SeekFrom::Start(position)).await?;
        
        Ok(())
    }
    
    pub async fn merge_chunks(
        &self,
        task_id: Uuid,
        final_path: &Path,
        chunk_count: usize,
    ) -> Result<()> {
        let task_dir = self.temp_dir.join(task_id.to_string());
        
        let mut final_file = File::create(final_path).await?;
        
        for i in 0..chunk_count {
            let chunk_path = task_dir.join(format!("chunk_{:04}.tmp", i));
            
            if !chunk_path.exists() {
                return Err(DownloadError::Internal(
                    format!("Missing chunk file: {}", chunk_path.display())
                ));
            }
            
            let mut chunk_file = File::open(&chunk_path).await?;
            let mut buffer = Vec::new();
            let bytes_read = chunk_file.read_to_end(&mut buffer).await?;
            
            if bytes_read == 0 {
                return Err(DownloadError::Internal(
                    format!("Empty chunk file: {}", chunk_path.display())
                ));
            }
            
            final_file.write_all(&buffer).await?;
            
            tracing::info!("STORAGE: Merged chunk {} ({} bytes)", i, bytes_read);
        }
        
        final_file.flush().await?;
        
        self.cleanup_temp_files(task_id).await?;
        
        Ok(())
    }
    
    pub async fn finalize_download(&self, task_id: Uuid, final_path: &Path) -> Result<()> {
        let task_dir = self.temp_dir.join(task_id.to_string());
        let temp_file = task_dir.join("chunk_0000.tmp");
        
        if temp_file.exists() {
            fs::rename(&temp_file, final_path).await?;
            self.cleanup_temp_files(task_id).await?;
        }
        
        Ok(())
    }
}
