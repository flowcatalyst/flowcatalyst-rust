//! Reading a delivery response without trusting its size
//! (`docs/parity/java-2026-09-25-triage.md` S13; Java b1ce6e55 S3.4).
//!
//! A subscriber, or a scheduled job's target, chooses how much it sends back.
//! Reading the whole body lets a hostile or chatty endpoint balloon memory
//! and the stored attempt row, so a body is read chunk by chunk only as far
//! as a cap, as Go reads it through `io.LimitReader`. The rest is never
//! read; dropping the response closes the connection. The client's own
//! request timeout still bounds how long the read may take.

/// How much of a subscriber's response a dispatch attempt keeps (Go
/// `maxResponseBody`, dispatchjob/processing/processing.go:76: 64 KiB).
pub const DELIVERY_RESPONSE_CAP: usize = 64 << 10;

/// How much of a scheduled job target's non-2xx response goes into the
/// failure message (Go scheduledjob/scheduler/dispatcher.go:242: 500 bytes).
pub const SCHEDULED_JOB_ERROR_SNIPPET_CAP: usize = 500;

/// A body read up to a cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CappedBody {
    pub bytes: Vec<u8>,
    /// Whether the body went on past the cap.
    pub truncated: bool,
}

impl CappedBody {
    /// The bytes as text; a character the cap cut in two, or any other
    /// invalid UTF-8, reads as U+FFFD.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

/// Read `response`'s body until it ends or `cap` bytes are in hand. A read
/// error ends the body where it stands, as Go ignores it (`raw, _ :=
/// io.ReadAll(...)`).
pub async fn read_capped(mut response: reqwest::Response, cap: usize) -> CappedBody {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if append_capped(&mut bytes, &chunk, cap) {
                    return CappedBody {
                        bytes,
                        truncated: true,
                    };
                }
            }
            Ok(None) | Err(_) => {
                return CappedBody {
                    bytes,
                    truncated: false,
                }
            }
        }
    }
}

/// Append as much of `chunk` as fits under `cap`; `true` when some of it
/// did not fit (or the cap was already reached with more to come).
fn append_capped(bytes: &mut Vec<u8>, chunk: &[u8], cap: usize) -> bool {
    let room = cap.saturating_sub(bytes.len());
    if chunk.len() > room {
        bytes.extend_from_slice(&chunk[..room]);
        true
    } else {
        bytes.extend_from_slice(chunk);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn chunks_are_kept_up_to_the_cap() {
        let mut bytes = Vec::new();
        assert!(!append_capped(&mut bytes, b"abc", 5));
        assert!(!append_capped(&mut bytes, b"de", 5));
        assert!(append_capped(&mut bytes, b"f", 5));
        assert_eq!(bytes, b"abcde");
        let mut bytes = Vec::new();
        assert!(append_capped(&mut bytes, b"abcdefgh", 3));
        assert_eq!(bytes, b"abc");
    }

    #[test]
    fn a_cut_character_reads_as_a_replacement() {
        let body = CappedBody {
            bytes: "é".as_bytes()[..1].to_vec(),
            truncated: true,
        };
        assert_eq!(body.text(), "\u{FFFD}");
    }

    /// A local server streaming far more than the cap: only the cap is read,
    /// and the read ends without waiting for the rest.
    #[tokio::test]
    async fn a_huge_response_is_read_only_to_the_cap() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            let _ = socket
                .write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 104857600\r\n\r\n",
                )
                .await;
            let block = vec![b'x'; 64 * 1024];
            // 100 MiB promised; stop when the client goes away.
            for _ in 0..1600 {
                if socket.write_all(&block).await.is_err() {
                    break;
                }
            }
        });

        let response = reqwest::Client::new()
            .get(format!("http://{addr}/"))
            .send()
            .await
            .unwrap();
        let body = read_capped(response, SCHEDULED_JOB_ERROR_SNIPPET_CAP).await;
        assert!(body.truncated);
        assert_eq!(body.bytes.len(), SCHEDULED_JOB_ERROR_SNIPPET_CAP);
        assert!(body.bytes.iter().all(|b| *b == b'x'));
    }

    #[tokio::test]
    async fn a_short_response_is_read_whole() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\n\r\n{\"ack\":true")
                .await;
        });
        let response = reqwest::Client::new()
            .get(format!("http://{addr}/"))
            .send()
            .await
            .unwrap();
        let body = read_capped(response, DELIVERY_RESPONSE_CAP).await;
        assert!(!body.truncated);
        assert_eq!(body.text(), "{\"ack\":true");
    }
}
