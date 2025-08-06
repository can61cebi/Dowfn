use std::sync::Arc;
use slint::{Model, ModelRc, VecModel, SharedString};  // Model trait'i eklendi
use dowfn_core::DownloadEngine;
use dowfn_shared::*;
use anyhow::Result;
use tracing::{info, error, debug};
use arboard::Clipboard;

slint::include_modules!();

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    hide_console_window();
    
    let config = AppConfig::default();
    let (engine, command_tx, mut event_rx) = DownloadEngine::new(config).await?;
    
    tokio::spawn(async move {
        engine.run().await;
    });
    
    let ui = MainWindow::new()?;
    let ui_handle = ui.as_weak();
    let command_tx = Arc::new(command_tx);
    
    let model_rc = ModelRc::new(VecModel::<DownloadItem>::default());
    ui.set_downloads(model_rc.clone());
    
    let cmd_tx = command_tx.clone();
    ui.on_add_download(move |url| {
        let url = url.to_string();
        info!("UI: Adding download for URL: {}", url);
        let cmd_tx = cmd_tx.clone();
        
        tokio::spawn(async move {
            let request = AddDownloadRequest {
                url: url.clone(),
                save_path: None,
                file_name: None,
                threads: None,
                metadata: None,
                auto_start: true,
            };
            
            info!("UI: Sending AddDownload command to engine for URL: {}", url);
            match cmd_tx.send(Command::AddDownload(request)) {
                Ok(_) => info!("UI: AddDownload command sent successfully"),
                Err(e) => error!("UI: Failed to send AddDownload command: {}", e),
            }
        });
    });
    
    let cmd_tx = command_tx.clone();
    ui.on_pause_download(move |id| {
        let id = uuid::Uuid::parse_str(&id.to_string()).unwrap();
        let _ = cmd_tx.send(Command::PauseDownload(id));
    });
    
    let cmd_tx = command_tx.clone();
    ui.on_resume_download(move |id| {
        let id = uuid::Uuid::parse_str(&id.to_string()).unwrap();
        let _ = cmd_tx.send(Command::ResumeDownload(id));
    });
    
    let cmd_tx = command_tx.clone();
    ui.on_cancel_download(move |id| {
        let id = uuid::Uuid::parse_str(&id.to_string()).unwrap();
        let _ = cmd_tx.send(Command::CancelDownload(id));
    });
    
    let cmd_tx = command_tx.clone();
    ui.on_remove_download(move |id| {
        let id = uuid::Uuid::parse_str(&id.to_string()).unwrap();
        let _ = cmd_tx.send(Command::RemoveDownload(id));
    });
    
    let cmd_tx = command_tx.clone();
    ui.on_clear_completed(move || {
        let _ = cmd_tx.send(Command::ClearCompleted);
    });
    
    let ui_weak = ui_handle.clone();
    ui.on_paste_from_clipboard(move || {
        if let Ok(mut clipboard) = Clipboard::new() {
            if let Ok(text) = clipboard.get_text() {
                let text = text.trim().to_string();
                if is_valid_url(&text) {
                    ui_weak.upgrade_in_event_loop(move |ui| {
                        ui.set_clipboard_content(SharedString::from(text));
                        ui.set_is_url_valid(true);
                    }).unwrap();
                }
            }
        }
    });
    
    ui.on_open_settings(|| {
        info!("Opening settings dialog");
    });
    
    let ui_weak = ui_handle.clone();
    tokio::spawn(async move {
        while let Some(response) = event_rx.recv().await {
            handle_response(response, &ui_weak).await;
        }
    });
    
    // Periyodik statistics güncellemesi
    let stats_cmd_tx = command_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
        loop {
            interval.tick().await;
            let _ = stats_cmd_tx.send(Command::GetStatistics);
        }
    });
    
    let _ = command_tx.send(Command::GetAllDownloads);
    
    ui.run()?;
    
    Ok(())
}

async fn handle_response(
    response: Response,
    ui_handle: &slint::Weak<MainWindow>,
) {
    debug!("UI: Received response: {:?}", std::mem::discriminant(&response));
    match response {
        Response::DownloadList(tasks) => {
            let items: Vec<DownloadItem> = tasks.into_iter().map(|task| {
                task_to_download_item(task)
            }).collect();
            
            ui_handle.upgrade_in_event_loop(move |ui| {
                let new_model = ModelRc::new(VecModel::from(items));
                ui.set_downloads(new_model);
            }).unwrap();
        }
        
        Response::Progress(update) => {
            ui_handle.upgrade_in_event_loop(move |ui| {
                let downloads = ui.get_downloads();
                for i in 0..downloads.row_count() {
                    if let Some(mut item) = downloads.row_data(i) {
                        if item.id == SharedString::from(update.download_id.to_string()) {
                            item.downloaded = SharedString::from(format_bytes(update.downloaded));
                            if let Some(total) = update.total {
                                item.progress = update.downloaded as f32 / total as f32;
                            }
                            item.speed = SharedString::from(format_speed(update.speed));
                            item.time_remaining = SharedString::from(
                                format_time_remaining(update.time_remaining)
                            );
                            downloads.set_row_data(i, item);
                            break;
                        }
                    }
                }
            }).unwrap();
        }
        
        Response::StatusChange(update) => {
            ui_handle.upgrade_in_event_loop(move |ui| {
                let downloads = ui.get_downloads();
                for i in 0..downloads.row_count() {
                    if let Some(mut item) = downloads.row_data(i) {
                        if item.id == SharedString::from(update.download_id.to_string()) {
                            item.status = SharedString::from(format_status(&update.new_status));
                            downloads.set_row_data(i, item);
                            break;
                        }
                    }
                }
            }).unwrap();
        }
        
        Response::Statistics(stats) => {
            ui_handle.upgrade_in_event_loop(move |ui| {
                ui.set_stats(Statistics {
                    total_downloads: stats.total_downloads as i32,
                    active_downloads: stats.active_downloads as i32,
                    completed_downloads: stats.completed_downloads as i32,
                    failed_downloads: stats.failed_downloads as i32,
                    current_speed: SharedString::from(format_speed(stats.current_download_speed)),
                    total_downloaded: SharedString::from(format_bytes(stats.total_bytes_downloaded)),
                });
            }).unwrap();
        }
        
        Response::Success(msg) => {
            info!("Success: {}", msg.message);
            if msg.download_id.is_some() {
                let _ = ui_handle.upgrade_in_event_loop(move |_ui| {
                    // UI güncellemesi
                }).unwrap();
            }
        }
        
        Response::Error(err) => {
            error!("Error: {}", err.message);
        }
        
        _ => {}
    }
}

fn task_to_download_item(task: DownloadTask) -> DownloadItem {
    DownloadItem {
        id: SharedString::from(task.id.to_string()),
        filename: SharedString::from(task.file_name),
        url: SharedString::from(task.url),
        size: SharedString::from(format_bytes(task.total_size.unwrap_or(0))),
        downloaded: SharedString::from(format_bytes(task.downloaded_size)),
        progress: if let Some(total) = task.total_size {
            if total > 0 {
                task.downloaded_size as f32 / total as f32
            } else {
                0.0
            }
        } else {
            0.0
        },
        speed: SharedString::from("0 B/s"),
        status: SharedString::from(format_status(&task.status)),
        time_remaining: SharedString::from("--:--"),
    }
}

fn format_status(status: &DownloadStatus) -> String {
    match status {
        DownloadStatus::Pending => "Pending",
        DownloadStatus::Connecting => "Connecting",
        DownloadStatus::Downloading => "Downloading",
        DownloadStatus::Paused => "Paused",
        DownloadStatus::Completed => "Completed",
        DownloadStatus::Failed(_) => "Failed",
        DownloadStatus::Cancelled => "Cancelled",
        DownloadStatus::Merging => "Merging",
        DownloadStatus::Verifying => "Verifying",
    }.to_string()
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    
    if bytes == 0 {
        return "0 B".to_string();
    }
    
    let mut size = bytes as f64;
    let mut unit_index = 0;
    
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    
    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}

fn format_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec))
}

fn format_time_remaining(seconds: Option<u64>) -> String {
    match seconds {
        Some(0) | None => "--:--".to_string(),
        Some(secs) => {
            let hours = secs / 3600;
            let minutes = (secs % 3600) / 60;
            let seconds = secs % 60;
            
            if hours > 0 {
                format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
            } else {
                format!("{:02}:{:02}", minutes, seconds)
            }
        }
    }
}

fn is_valid_url(text: &str) -> bool {
    url::Url::parse(text).is_ok()
}

fn init_logging() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env()
            .add_directive("dowfn=debug".parse().unwrap())
            .add_directive("info".parse().unwrap()))
        .init();
}

#[cfg(windows)]
fn hide_console_window() {
    use windows::Win32::System::Console::FreeConsole;
    unsafe { FreeConsole().ok(); }
}

#[cfg(not(windows))]
fn hide_console_window() {}