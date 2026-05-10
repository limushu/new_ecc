use bytes::Bytes;
use futures_util::StreamExt;

use crate::port::forward::{ForwardError, ForwardPort, ForwardResult, ForwardStream};

pub struct ReqwestForwarder {
    client: reqwest::Client,
}

impl ReqwestForwarder {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl ForwardPort for ReqwestForwarder {
    fn send(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        body: Bytes,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ForwardResult, ForwardError>> + Send + '_>> {
        let url = url.to_string();
        let client = self.client.clone();
        Box::pin(async move {
            let mut req = client.post(&url);
            for (k, v) in &headers {
                req = req.header(k.as_str(), v.as_str());
            }
            let resp = req.body(body).send().await.map_err(|e| ForwardError::Connection(e.to_string()))?;
            let status = resp.status().as_u16();
            if !resp.status().is_success() {
                let body_text = resp.text().await.unwrap_or_default();
                return Err(ForwardError::Upstream { status, body: body_text });
            }
            let body_bytes = resp.bytes().await.map_err(|e| ForwardError::Connection(e.to_string()))?;
            Ok(ForwardResult { status, body: body_bytes })
        })
    }

    fn send_streaming(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        body: Bytes,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ForwardStream, ForwardError>> + Send + '_>> {
        let url = url.to_string();
        let client = self.client.clone();
        Box::pin(async move {
            let mut req = client.post(&url);
            for (k, v) in &headers {
                req = req.header(k.as_str(), v.as_str());
            }
            let resp = req.body(body).send().await.map_err(|e| ForwardError::Connection(e.to_string()))?;
            let status = resp.status().as_u16();
            if !resp.status().is_success() {
                let body_text = resp.text().await.unwrap_or_default();
                return Err(ForwardError::Upstream { status, body: body_text });
            }
            let mut chunks = Vec::new();
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(b) => chunks.push(b),
                    Err(e) => {
                        tracing::warn!("stream chunk error: {e}");
                        break;
                    }
                }
            }
            Ok(ForwardStream { status, chunks })
        })
    }
}
