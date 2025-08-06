use std::time::Duration;
use reqwest::{Client, Response, header};
use futures::Stream;
use bytes::Bytes;
use crate::error::*;

pub struct HttpClient {
    client: Client,
}

pub struct HeadResponse {
    pub content_length: Option<u64>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub accept_ranges: bool,
}

impl HttpClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))  // 5 minutes for streaming downloads
            .connect_timeout(Duration::from_secs(15))  // Slightly longer connect timeout
            .pool_idle_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(8)  // Reasonable connection pooling
            .http1_title_case_headers()
            .http2_adaptive_window(true)
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
            .danger_accept_invalid_certs(false)
            .tcp_nodelay(true)
            .tcp_keepalive(Duration::from_secs(60))  // Longer keepalive for sustained downloads
            .build()
            .map_err(|e| DownloadError::Internal(e.to_string()))?;
        
        Ok(Self { client })
    }
    
    pub async fn head(&self, url: &str) -> Result<HeadResponse> {
        let mut attempts = 0;
        let max_attempts = 3;
        
        loop {
            attempts += 1;
            
            match self.client
                .head(url)
                .send()
                .await 
            {
                Ok(response) => {
                    let headers = response.headers();
                    
                    return Ok(HeadResponse {
                        content_length: headers
                            .get(header::CONTENT_LENGTH)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse().ok()),
                        content_type: headers
                            .get(header::CONTENT_TYPE)
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.to_string()),
                        content_disposition: headers
                            .get(header::CONTENT_DISPOSITION)
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.to_string()),
                        etag: headers
                            .get(header::ETAG)
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.to_string()),
                        last_modified: headers
                            .get(header::LAST_MODIFIED)
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.to_string()),
                        accept_ranges: headers
                            .get(header::ACCEPT_RANGES)
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.contains("bytes"))
                            .unwrap_or(false),
                    });
                }
                Err(e) => {
                    if attempts >= max_attempts {
                        return Err(DownloadError::Network(e));
                    }
                    
                    tokio::time::sleep(Duration::from_millis(1000 * attempts as u64)).await;
                }
            }
        }
    }
    
    pub async fn get_stream(&self, url: &str) -> Result<impl Stream<Item = reqwest::Result<Bytes>>> {
        let response = self.client
            .get(url)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(DownloadError::Network(
                reqwest::Error::from(response.error_for_status().unwrap_err())
            ));
        }
        
        Ok(response.bytes_stream())
    }
    
    pub async fn get_range_stream(
        &self,
        url: &str,
        start: u64,
        end: u64,
    ) -> Result<impl Stream<Item = reqwest::Result<Bytes>>> {
        let range = format!("bytes={}-{}", start, end);
        
        let response = self.client
            .get(url)
            .header(header::RANGE, range)
            .send()
            .await?;
        
        if !response.status().is_success() && response.status() != 206 {
            return Err(DownloadError::Network(
                reqwest::Error::from(response.error_for_status().unwrap_err())
            ));
        }
        
        Ok(response.bytes_stream())
    }
    
    pub async fn get_with_headers(
        &self,
        url: &str,
        custom_headers: Vec<(String, String)>,
    ) -> Result<Response> {
        let mut request = self.client.get(url);
        
        for (key, value) in custom_headers {
            request = request.header(key, value);
        }
        
        let response = request.send().await?;
        
        if !response.status().is_success() {
            return Err(DownloadError::Network(
                reqwest::Error::from(response.error_for_status().unwrap_err())
            ));
        }
        
        Ok(response)
    }
}