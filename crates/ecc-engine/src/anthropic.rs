use bytes::Bytes;

use crate::context::RequestContext;
use crate::converter::{ConvertedRequest, ProtocolConverter};

/// Anthropic passthrough — request/response/stream go through unchanged.
pub struct AnthropicConverter;

impl ProtocolConverter for AnthropicConverter {
    fn convert_request(&self, ctx: &RequestContext) -> std::result::Result<ConvertedRequest, String> {
        let config = ctx.provider_config.as_ref().ok_or("no provider config")?;
        let target = ctx.resolved_target.as_ref().ok_or("no resolved target")?;

        let url = format!("{}{}", config.base_url.trim_end_matches('/'), ctx.path);
        let mut headers = Vec::new();
        headers.push(("x-api-key".to_string(), config.auth_token.clone()));
        headers.push(("content-type".to_string(), "application/json".to_string()));
        headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));

        // Replace model in body
        let body = replace_model(&ctx.body, &target.provider_model)?;

        Ok(ConvertedRequest { url, headers, body })
    }

    fn convert_response(&self, body: Bytes) -> std::result::Result<Bytes, String> {
        Ok(body) // passthrough
    }

    fn convert_stream_chunk(&self, chunk: Bytes) -> std::result::Result<Vec<String>, String> {
        let s = String::from_utf8_lossy(&chunk);
        Ok(s.lines()
            .filter(|l| l.starts_with("data: "))
            .map(|l| l.to_string())
            .collect())
    }
}

fn replace_model(body: &Bytes, model: &str) -> std::result::Result<Bytes, String> {
    let mut obj: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("invalid json: {e}"))?;
    if let Some(m) = obj.get_mut("model") {
        *m = serde_json::Value::String(model.to_string());
    }
    serde_json::to_vec(&obj)
        .map(Bytes::from)
        .map_err(|e| format!("serialize failed: {e}"))
}
