use std::io::{self, Read, Write};
use serde::{Deserialize, Serialize};
use serde_json;
use tokio::sync::mpsc;
use dowfn_shared::*;

#[derive(Serialize, Deserialize)]
struct NativeMessage {
    #[serde(rename = "type")]
    msg_type: String,
    payload: serde_json::Value,
}

fn read_message() -> io::Result<String> {
    let mut length_bytes = [0u8; 4];
    io::stdin().read_exact(&mut length_bytes)?;
    
    let length = u32::from_ne_bytes(length_bytes) as usize;
    
    if length > 1024 * 1024 * 10 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Message too large"));
    }
    
    let mut buffer = vec![0u8; length];
    io::stdin().read_exact(&mut buffer)?;
    
    String::from_utf8(buffer)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn write_message(message: &str) -> io::Result<()> {
    let bytes = message.as_bytes();
    let length = bytes.len() as u32;
    
    io::stdout().write_all(&length.to_ne_bytes())?;
    io::stdout().write_all(bytes)?;
    io::stdout().flush()?;
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    
    std::thread::spawn(move || {
        loop {
            match read_message() {
                Ok(msg) => {
                    if let Ok(native_msg) = serde_json::from_str::<NativeMessage>(&msg) {
                        let _ = tx.send(native_msg);
                    }
                }
                Err(_) => break,
            }
        }
    });
    
    while let Some(msg) = rx.recv().await {
        match msg.msg_type.as_str() {
            "Handshake" => {
                let response = NativeMessage {
                    msg_type: "Success".to_string(),
                    payload: serde_json::json!({
                        "message": "Connected to Dowfn"
                    }),
                };
                
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = write_message(&json);
                }
            }
            "Command" => {
                if let Ok(cmd) = serde_json::from_value::<Command>(msg.payload) {
                    // Forward command to main Dowfn application via IPC
                    // This would connect to the main app's IPC server
                }
            }
            _ => {}
        }
    }
    
    Ok(())
}