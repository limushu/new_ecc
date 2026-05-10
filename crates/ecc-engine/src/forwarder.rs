use std::sync::Arc;

use bytes::Bytes;

use crate::context::RequestContext;
use crate::middleware::{Middleware, Next, PipelineError, PipelineResult};
use crate::port::forward::{ForwardError, ForwardPort};

pub struct Forwarder {
    forward_port: Arc<dyn ForwardPort>,
}

impl Forwarder {
    pub fn new(forward_port: Arc<dyn ForwardPort>) -> Self {
        Self { forward_port }
    }
}

impl Middleware for Forwarder {
    fn handle<'a>(
        &'a self,
        ctx: &'a mut RequestContext,
        next: Next<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PipelineResult> + Send + 'a>> {
        Box::pin(async move {
            let url = ctx.upstream_url.clone().ok_or_else(|| PipelineError::Aborted("no upstream url".into()))?;
            let headers = ctx.upstream_headers.clone().unwrap_or_default();
            let body = ctx.upstream_body.clone().unwrap_or_default();
            let streaming = ctx.is_streaming();

            if streaming {
                match self.forward_port.send_streaming(&url, headers, body).await {
                    Ok(result) => {
                        ctx.response_status = Some(result.status);
                        ctx.stream_chunks = result.chunks;
                        ctx.usage = extract_stream_usage(&ctx.stream_chunks);
                    }
                    Err(ForwardError::Upstream { status, body: b }) => {
                        ctx.response_status = Some(status);
                        ctx.response_body = Some(Bytes::from(b));
                        return Err(PipelineError::Aborted(format!("upstream {status}")));
                    }
                    Err(e) => return Err(PipelineError::Aborted(e.to_string())),
                }
            } else {
                match self.forward_port.send(&url, headers, body).await {
                    Ok(result) => {
                        ctx.response_status = Some(result.status);
                        ctx.response_body = Some(result.body.clone());
                        ctx.usage = extract_response_usage(&result.body);
                    }
                    Err(ForwardError::Upstream { status, body: b }) => {
                        ctx.response_status = Some(status);
                        ctx.response_body = Some(Bytes::from(b));
                        return Err(PipelineError::Aborted(format!("upstream {status}")));
                    }
                    Err(e) => return Err(PipelineError::Aborted(e.to_string())),
                }
            }

            next.run(ctx).await
        })
    }
}

fn extract_response_usage(body: &[u8]) -> Option<crate::context::TokenUsage> {
    let obj: serde_json::Value = serde_json::from_slice(body).ok()?;
    let usage = obj.get("usage")?;
    Some(crate::context::TokenUsage {
        input_tokens: usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        cache_read_tokens: usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        output_tokens: usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
    })
}

fn extract_stream_usage(chunks: &[Bytes]) -> Option<crate::context::TokenUsage> {
    for chunk in chunks.iter().rev() {
        let s = String::from_utf8_lossy(chunk);
        for line in s.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" { continue; }
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(usage) = obj.get("usage") {
                        return Some(crate::context::TokenUsage {
                            input_tokens: usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                            cache_read_tokens: usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                            output_tokens: usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        });
                    }
                }
            }
        }
    }
    None
}
