use std::pin::Pin;

use bytes::Bytes;

use futures_util::Future;

/// Port consumed by Forwarder middleware — sends HTTP requests upstream.
pub trait ForwardPort: Send + Sync {
    fn send(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        body: Bytes,
    ) -> Pin<Box<dyn Future<Output = Result<ForwardResult, ForwardError>> + Send + '_>>;

    fn send_streaming(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        body: Bytes,
    ) -> Pin<Box<dyn Future<Output = Result<ForwardStream, ForwardError>> + Send + '_>>;
}

#[derive(Debug)]
pub struct ForwardResult {
    pub status: u16,
    pub body: Bytes,
}

pub struct ForwardStream {
    pub status: u16,
    pub chunks: Vec<Bytes>,
}

#[derive(Debug, thiserror::Error)]
pub enum ForwardError {
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("timeout")]
    Timeout,
    #[error("upstream error {status}: {body}")]
    Upstream { status: u16, body: String },
}
