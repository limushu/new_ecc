use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::Request;
use hyper::Response;

use ecc_engine::middleware::{Pipeline, PipelineError};
use ecc_engine::context::RequestContext;

#[derive(Clone)]
pub struct ProxyServer {
    pipeline: Arc<Pipeline>,
}

impl ProxyServer {
    pub fn new(pipeline: Arc<Pipeline>) -> Self {
        Self { pipeline }
    }

    pub async fn handle(&self, req: Request<Incoming>) -> Response<Full<Bytes>> {
        let (parts, body) = req.into_parts();
        let raw_body = body
            .collect()
            .await
            .map(|b| b.to_bytes())
            .unwrap_or_default();

        let mut ctx = RequestContext::new(
            parts.method,
            parts.uri.path().to_string(),
            parts.headers,
            raw_body,
        );

        ctx.extract_model();

        match self.pipeline.execute(&mut ctx).await {
            Ok(()) => self.build_response(&ctx),
            Err(PipelineError::Aborted(msg)) => {
                tracing::warn!(id = %ctx.id, "pipeline aborted: {msg}");
                self.build_response(&ctx)
            }
            Err(PipelineError::Internal(msg)) => {
                tracing::error!(id = %ctx.id, "pipeline error: {msg}");
                json_error(500, &msg)
            }
        }
    }

    fn build_response(&self, ctx: &RequestContext) -> Response<Full<Bytes>> {
        if !ctx.stream_chunks.is_empty() {
            let mut body = Vec::new();
            for chunk in &ctx.stream_chunks {
                body.extend_from_slice(chunk);
            }
            let status = ctx.response_status.unwrap_or(200);
            return Response::builder()
                .status(status)
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .body(Full::new(Bytes::from(body)))
                .unwrap();
        }

        let status = ctx.response_status.unwrap_or(502);
        let body = ctx.response_body.clone().unwrap_or_default();
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Full::new(body))
            .unwrap()
    }
}

fn json_error(status: u16, msg: &str) -> Response<Full<Bytes>> {
    let body = serde_json::json!({ "error": msg });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}
