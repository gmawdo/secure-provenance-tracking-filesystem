
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};
use bytes::{BytesMut, Bytes};
use std::sync::Arc;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tracing::debug;
pub struct ActiveWrite {
    pub channel: Arc<ChannelBuffer>,
    pub last_activity: Instant,
}

impl ActiveWrite {
    pub fn new(channel: Arc<ChannelBuffer>) -> Self {
        ActiveWrite {
            channel,
            last_activity: Instant::now(),
        }
    }
}
pub struct ChannelBuffer {
    //buffer: Mutex<BytesMut>,
    buffer: Mutex<BTreeMap<u64, Bytes>>,
    total_size: AtomicU64,
    last_write: Mutex<Instant>,
    is_complete: AtomicBool,
}

impl ChannelBuffer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            buffer: Mutex::new(BTreeMap::new()),
            total_size: AtomicU64::new(0),
            last_write: Mutex::new(Instant::now()),
            is_complete: AtomicBool::new(false),
        })
    }

    pub async fn read_range(&self, offset: u64, count: u32) -> Vec<u8> {
        let buffer = self.buffer.lock().await;
        let mut result = Vec::with_capacity(count as usize);
        let end_offset = offset + count as u64;
        let total_size = self.total_size.load(Ordering::SeqCst);
        
        // If reading past total size, adjust end_offset
        let end_offset = std::cmp::min(end_offset, total_size);
        
        // Find all chunks that overlap with the requested range
        let mut current_offset = offset;
        while current_offset < end_offset {
            if let Some(bytes) = buffer.get(&current_offset) {
                let available_bytes = bytes.len() as u64;
                let bytes_to_copy = std::cmp::min(
                    available_bytes,
                    end_offset - current_offset
                ) as usize;
                
                result.extend_from_slice(&bytes[..bytes_to_copy]);
                current_offset += bytes_to_copy as u64;
            } else {
                break;
            }
        }
        
        result
    }

pub async fn write(&self, offset: u64, data: &[u8]) {
    debug!("write: {:?}", offset);
    let mut buffer = self.buffer.lock().await;
    
    buffer.insert(offset, Bytes::copy_from_slice(data));
    
    // Update total size if this write extends it
    let end_offset = offset + data.len() as u64;
    let current_size = self.total_size.load(Ordering::SeqCst);
    if end_offset > current_size {
        self.total_size.store(end_offset, Ordering::SeqCst);
    }
    
    // Update last write time
    *self.last_write.lock().await = Instant::now();
}

    pub async fn read_all(&self) -> Bytes {
        debug!("read_all");
        let buffer = self.buffer.lock().await;
        let mut result = BytesMut::with_capacity(self.total_size.load(Ordering::SeqCst) as usize);
        
        let mut expected_offset = 0;
        for (&offset, chunk) in buffer.iter() {
            if offset != expected_offset {
                result.resize(offset as usize, 0);
            }
            result.extend_from_slice(chunk);
            expected_offset = offset + chunk.len() as u64;
        }

        result.freeze()
    }

    pub fn total_size(&self) -> u64 {
        self.total_size.load(Ordering::SeqCst)
    }

    pub fn is_write_complete(&self) -> bool {
        self.is_complete.load(Ordering::SeqCst)
    }

    pub fn set_complete(&self) {
        self.is_complete.store(true, Ordering::SeqCst);
    }

    pub async fn time_since_last_write(&self) -> Duration {
        Instant::now().duration_since(*self.last_write.lock().await)
    }

    pub async fn clear(&self) {
        let mut buffer = self.buffer.lock().await;
        buffer.clear();
    }

    pub async fn is_empty(&self) -> bool {
        let buffer = self.buffer.lock().await;
        buffer.is_empty()
    }
}