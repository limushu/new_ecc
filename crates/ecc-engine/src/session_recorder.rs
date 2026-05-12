use std::sync::Arc;

use crate::context::RequestContext;
use crate::middleware::{Middleware, Next, PipelineResult};
use crate::port::SessionPort;
use ecc_domain::repository::SessionRecord;

/// If the last activity of a matching session was more than this, treat as a new session.
const SESSION_GAP_SECS: i64 = 30 * 60; // 30 minutes

pub struct SessionRecorder {
    session_port: Arc<dyn SessionPort>,
}

impl SessionRecorder {
    pub fn new(session_port: Arc<dyn SessionPort>) -> Self {
        Self { session_port }
    }
}

impl Middleware for SessionRecorder {
    fn handle<'a>(
        &'a self,
        ctx: &'a mut RequestContext,
        next: Next<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PipelineResult> + Send + 'a>> {
        Box::pin(async move {
            let base_hash = match extract_base_hash(&ctx.body) {
                Some(h) => h,
                None => return next.run(ctx).await,
            };

            let session_id = self.resolve_session(&base_hash);

            let result = next.run(ctx).await;

            if let (Some(target), Some(provider)) = (&ctx.resolved_target, &ctx.provider_config) {
                let request_body = String::from_utf8_lossy(&ctx.body).to_string();

                let response_body = if !ctx.stream_chunks.is_empty() {
                    assemble_stream_response(&ctx.stream_chunks)
                } else {
                    ctx.response_body
                        .as_ref()
                        .map(|b| String::from_utf8_lossy(b).to_string())
                        .unwrap_or_default()
                };

                let (assistant_text, thinking_text) = parse_response(&response_body);

                let usage = ctx.usage.as_ref();
                let record = SessionRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id,
                    timestamp: chrono::Utc::now(),
                    provider_name: provider.name.clone(),
                    target_model: target.provider_model.clone(),
                    requested_model: ctx.requested_model.clone().unwrap_or_default(),
                    request_body,
                    response_body,
                    assistant_text,
                    thinking_text,
                    input_tokens: usage.map(|u| u.input_tokens).unwrap_or(0),
                    output_tokens: usage.map(|u| u.output_tokens).unwrap_or(0),
                    latency_ms: ctx.timestamp.elapsed().as_millis() as u64,
                    status: ctx.response_status.unwrap_or(0),
                };

                if let Err(e) = self.session_port.record(record) {
                    tracing::warn!("failed to record session: {e}");
                }
            }

            result
        })
    }
}

impl SessionRecorder {
    fn resolve_session(&self, base_hash: &str) -> String {
        match self.session_port.find_latest_by_prefix(base_hash) {
            Ok(Some((existing_id, last_ts))) => {
                let gap = chrono::Utc::now()
                    .signed_duration_since(last_ts)
                    .num_seconds();
                if gap.abs() <= SESSION_GAP_SECS {
                    return existing_id;
                }
                self.next_session_id(base_hash, &existing_id)
            }
            _ => base_hash.to_string(),
        }
    }

    fn next_session_id(&self, base: &str, existing: &str) -> String {
        let counter = if let Some(rest) = existing.strip_prefix(base) {
            if rest.is_empty() {
                2
            } else {
                rest.trim_start_matches('_').parse::<u32>().unwrap_or(1) + 1
            }
        } else {
            2
        };
        format!("{base}_{counter}")
    }
}

fn extract_base_hash(body: &[u8]) -> Option<String> {
    let obj = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let messages = obj.get("messages")?.as_array()?;
    let first_user = messages.iter().find(|m| {
        m.get("role").and_then(|r| r.as_str()) == Some("user")
    })?;
    let content = first_user.get("content")?;

    let content_str = match content {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };

    if content_str.is_empty() {
        return None;
    }

    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    if let Some(system) = obj.get("system") {
        system.to_string().hash(&mut hasher);
    }

    content_str.hash(&mut hasher);
    Some(format!("ses_{:016x}", hasher.finish()))
}

fn assemble_stream_response(chunks: &[bytes::Bytes]) -> String {
    chunks
        .iter()
        .map(|c| String::from_utf8_lossy(c).to_string())
        .collect()
}

/// Parse response body to extract assistant text and thinking text.
/// Handles both streaming SSE and non-streaming JSON formats.
fn parse_response(body: &str) -> (String, String) {
    // Try non-streaming JSON first
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(body) {
        let assistant = extract_text_from_json(&obj);
        let thinking = extract_thinking_from_json(&obj);
        return (assistant, thinking);
    }

    // Streaming SSE format
    let mut assistant = String::new();
    let mut thinking = String::new();

    for line in body.lines() {
        let data = match line.strip_prefix("data: ") {
            Some(d) if d != "[DONE]" => d,
            _ => continue,
        };

        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(data) {
            // Anthropic content_block_delta: text
            if let Some(text) = obj
                .get("delta")
                .and_then(|d| d.get("text"))
                .and_then(|t| t.as_str())
            {
                assistant.push_str(text);
            }
            // Anthropic content_block_delta: thinking
            if let Some(text) = obj
                .get("delta")
                .and_then(|d| d.get("thinking"))
                .and_then(|t| t.as_str())
            {
                thinking.push_str(text);
            }
            // OpenAI choices[0].delta.content
            if let Some(text) = obj
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(|t| t.as_str())
            {
                assistant.push_str(text);
            }
        }
    }

    (assistant, thinking)
}

fn extract_text_from_json(obj: &serde_json::Value) -> String {
    // Anthropic: content array
    if let Some(content) = obj.get("content").and_then(|c| c.as_array()) {
        return content
            .iter()
            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect();
    }
    // OpenAI: choices[0].message.content
    if let Some(text) = obj
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|t| t.as_str())
    {
        return text.to_string();
    }
    String::new()
}

fn extract_thinking_from_json(obj: &serde_json::Value) -> String {
    if let Some(content) = obj.get("content").and_then(|c| c.as_array()) {
        return content
            .iter()
            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("thinking"))
            .filter_map(|b| b.get("thinking").and_then(|t| t.as_str()))
            .collect();
    }
    String::new()
}
