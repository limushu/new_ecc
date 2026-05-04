//! Proxy server — receives requests from Claude Code and drives the middleware pipeline.

use std::sync::Arc;

use http::{HeaderMap, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response};

use ecc_core::context::RequestContext;
use ecc_core::logging::{REQ_BODY_ERROR, REQ_COMPLETED, REQ_FAILED, REQ_RECEIVED};
use ecc_core::middleware::Pipeline;
use ecc_core::{ecc_error, ecc_info};

type BoxBody = Full<bytes::Bytes>;

#[derive(Clone)]
pub struct ProxyServer {
    pipeline: Arc<Pipeline>,
}

impl ProxyServer {
    pub fn new(pipeline: Arc<Pipeline>) -> Self {
        Self { pipeline }
    }

    pub async fn handle(&self, req: Request<Incoming>) -> Response<BoxBody> {
        let (parts, body) = req.into_parts();

        let method = parts.method.clone();
        let path = parts.uri.path().to_string();

        let body_bytes = match body.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                ecc_error!(REQ_BODY_ERROR, %method, %path, "Failed to read request body: {e}");
                return error_response(
                    StatusCode::BAD_REQUEST,
                    format!("{{\"error\":\"Failed to read body: {e}\"}}"),
                );
            }
        };

        let mut headers = HeaderMap::new();
        for (name, value) in parts.headers {
            if let Some(name) = name {
                headers.insert(name, value);
            }
        }

        let model = serde_json::from_slice::<serde_json::Value>(&body_bytes)
            .ok()
            .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(String::from))
            .unwrap_or_else(|| "?".to_string());

        let mut ctx = RequestContext::new(method.clone(), path.clone(), headers, body_bytes);

        ecc_info!(REQ_RECEIVED, request_id = %ctx.id, %method, %path, %model, "request received");

        match self.pipeline.execute(&mut ctx).await {
            Ok(()) => {
                let status = ctx.response_status.unwrap_or(200);
                let provider = ctx.resolved_target.as_ref()
                    .map(|t| t.provider.as_str()).unwrap_or("-");
                let target = ctx.resolved_target.as_ref()
                    .map(|t| t.model.as_str()).unwrap_or("-");
                let in_tok = ctx.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0);
                let out_tok = ctx.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0);

                ecc_info!(REQ_COMPLETED,
                    request_id = %ctx.id,
                    status,
                    provider = %provider,
                    target_model = %target,
                    input_tokens = in_tok,
                    output_tokens = out_tok,
                    retries = ctx.retry_count,
                    "request completed"
                );

                Response::builder()
                    .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
                    .body(Full::new(bytes::Bytes::new()))
                    .unwrap()
            }
            Err(e) => {
                let model = ctx.requested_model.as_deref().unwrap_or("?");
                let provider = ctx.resolved_target.as_ref()
                    .map(|t| t.provider.as_str()).unwrap_or("-");
                ecc_error!(REQ_FAILED,
                    request_id = %ctx.id,
                    model = %model,
                    provider = %provider,
                    retries = ctx.retry_count,
                    "pipeline failed: {e}"
                );
                error_response(
                    StatusCode::BAD_GATEWAY,
                    format!("{{\"error\":\"{e}\"}}"),
                )
            }
        }
    }
}

fn error_response(status: StatusCode, body: String) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(bytes::Bytes::from(body)))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t48_proxy_server_constructs() {
        let pipeline = Arc::new(Pipeline::new());
        let _server = ProxyServer::new(pipeline);
    }
}
