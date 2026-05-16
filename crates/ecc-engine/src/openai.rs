use bytes::Bytes;

use crate::context::RequestContext;
use crate::converter::{ConvertedRequest, ProtocolConverter};

/// OpenAI converter — transforms Anthropic format to/from OpenAI Chat Completions.
pub struct OpenAiConverter;

impl ProtocolConverter for OpenAiConverter {
    fn convert_request(&self, ctx: &RequestContext) -> std::result::Result<ConvertedRequest, String> {
        let config = ctx.provider_config.as_ref().ok_or("no provider config")?;
        let target = ctx.resolved_target.as_ref().ok_or("no resolved target")?;

        let url = format!(
            "{}/chat/completions",
            config.base_url.trim_end_matches('/')
        );
        let mut headers = Vec::new();
        match config.auth_type {
            ecc_domain::provider::AuthType::Bearer => {
                headers.push(("Authorization".to_string(), format!("Bearer {}", config.auth_token)));
            }
            ecc_domain::provider::AuthType::ApiKey => {
                headers.push(("Authorization".to_string(), format!("Bearer {}", config.auth_token)));
            }
        }
        headers.push(("Content-Type".to_string(), "application/json".to_string()));

        let body = anthropic_to_openai(&ctx.body, &target.provider_model)?;

        Ok(ConvertedRequest { url, headers, body })
    }

    fn convert_response(&self, body: Bytes) -> std::result::Result<Bytes, String> {
        openai_to_anthropic(&body)
    }

    fn convert_stream_chunk(&self, chunk: Bytes) -> std::result::Result<Vec<String>, String> {
        let s = String::from_utf8_lossy(&chunk);
        let mut out = Vec::new();
        for line in s.lines() {
            if !line.starts_with("data: ") {
                continue;
            }
            let data = &line[6..];
            if data == "[DONE]" {
                out.push("data: [DONE]\n\n".to_string());
                continue;
            }
            let chunk: serde_json::Value =
                serde_json::from_str(data).map_err(|e| format!("invalid stream json: {e}"))?;

            let delta = chunk
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("delta"));

            // reasoning_content → thinking_delta (separate event)
            if let Some(reasoning) = delta.as_ref().and_then(|d| d.get("reasoning_content")).and_then(|r| r.as_str()) {
                if !reasoning.is_empty() {
                    out.push(format!("data: {}\n\n", serde_json::json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": { "type": "thinking_delta", "thinking": reasoning }
                    })));
                }
            }

            // content → text_delta
            let text = delta
                .and_then(|d| d.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !text.is_empty() {
                out.push(format!("data: {}\n\n", serde_json::json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": { "type": "text_delta", "text": text }
                })));
            }
        }
        Ok(out)
    }
}

fn anthropic_to_openai(body: &Bytes, model: &str) -> std::result::Result<Bytes, String> {
    let src: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("invalid json: {e}"))?;

    let mut dst = serde_json::Map::new();
    dst.insert("model".into(), serde_json::Value::String(model.to_string()));

    let stream = src.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    dst.insert("stream".into(), serde_json::Value::Bool(stream));

    // Convert messages
    if let Some(msgs) = src.get("messages").and_then(|v| v.as_array()) {
        let mut out = Vec::new();
        for msg in msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            if let Some(content) = msg.get("content") {
                out.push(serde_json::json!({
                    "role": role,
                    "content": content_to_text(content),
                }));
            }
        }
        dst.insert("messages".into(), serde_json::Value::Array(out));
    }

    // Copy common params
    if let Some(v) = src.get("max_tokens") {
        dst.insert("max_tokens".into(), v.clone());
    }
    if let Some(v) = src.get("temperature") {
        dst.insert("temperature".into(), v.clone());
    }
    if let Some(v) = src.get("top_p") {
        dst.insert("top_p".into(), v.clone());
    }
    if let Some(v) = src.get("stop_sequences") {
        dst.insert("stop".into(), v.clone());
    }

    // Pass through thinking parameter for providers like GLM
    if let Some(v) = src.get("thinking") {
        dst.insert("thinking".into(), v.clone());
    }

    serde_json::to_vec(&dst)
        .map(Bytes::from)
        .map_err(|e| format!("serialize failed: {e}"))
}

fn openai_to_anthropic(body: &Bytes) -> std::result::Result<Bytes, String> {
    let src: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("invalid json: {e}"))?;

    let mut dst = serde_json::Map::new();

    // id
    if let Some(id) = src.get("id") {
        dst.insert("id".into(), id.clone());
    } else {
        dst.insert("id".into(), serde_json::Value::String(format!("msg_{}", uuid::Uuid::new_v4().simple())));
    }

    dst.insert("type".into(), serde_json::Value::String("message".into()));
    dst.insert("role".into(), serde_json::Value::String("assistant".into()));

    // Build content array from choices[0].message
    let mut content = Vec::new();
    let msg = src
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"));

    // reasoning_content → thinking block
    if let Some(reasoning) = msg.and_then(|m| m.get("reasoning_content")).and_then(|r| r.as_str()) {
        if !reasoning.is_empty() {
            content.push(serde_json::json!({
                "type": "thinking",
                "thinking": reasoning,
            }));
        }
    }

    // content → text block
    let text = msg
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    if !text.is_empty() {
        content.push(serde_json::json!({
            "type": "text",
            "text": text,
        }));
    }

    if content.is_empty() {
        content.push(serde_json::json!({ "type": "text", "text": "" }));
    }
    dst.insert("content".into(), serde_json::Value::Array(content));

    // model
    if let Some(m) = src.get("model") {
        dst.insert("model".into(), m.clone());
    }

    // usage
    let usage = src.get("usage").cloned().unwrap_or(serde_json::json!({}));
    let mut anthropic_usage = serde_json::Map::new();
    anthropic_usage.insert(
        "input_tokens".into(),
        usage.get("prompt_tokens").cloned().unwrap_or(serde_json::json!(0)),
    );
    anthropic_usage.insert(
        "output_tokens".into(),
        usage.get("completion_tokens").cloned().unwrap_or(serde_json::json!(0)),
    );
    dst.insert("usage".into(), serde_json::Value::Object(anthropic_usage));

    // stop_reason
    let stop_reason = src
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .map(|r| match r {
            "stop" => "end_turn",
            "length" => "max_tokens",
            _ => "end_turn",
        })
        .unwrap_or("end_turn");
    dst.insert("stop_reason".into(), serde_json::Value::String(stop_reason.to_string()));

    serde_json::to_vec(&dst)
        .map(Bytes::from)
        .map_err(|e| format!("serialize failed: {e}"))
}

fn content_to_text(content: &serde_json::Value) -> serde_json::Value {
    match content {
        serde_json::Value::String(s) => serde_json::Value::String(s.clone()),
        serde_json::Value::Array(parts) => {
            let texts: Vec<&str> = parts
                .iter()
                .filter_map(|p| {
                    if p.get("type").and_then(|v| v.as_str()) == Some("text") {
                        p.get("text").and_then(|v| v.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            serde_json::Value::String(texts.join("\n"))
        }
        _ => content.clone(),
    }
}
