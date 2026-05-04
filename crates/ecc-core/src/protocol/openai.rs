//! OpenAI Chat Completions protocol converter.
//!
//! Handles bidirectional conversion between Anthropic Messages API and OpenAI Chat Completions API.
//!
//! # Request conversion (Anthropic → OpenAI)
//!
//! | Anthropic | OpenAI | Notes |
//! |-----------|--------|-------|
//! | `system` (top-level) | `messages[0].role="system"` | String or array → plain text |
//! | `messages[].content[].type="thinking"` | `reasoning_content` | DeepSeek extension |
//! | `messages[].content[].type="tool_use"` | `tool_calls[].function` | Unwrap input to JSON string |
//! | `messages[].content[].type="tool_result"` | `role="tool"` message | Flatten to separate message |
//! | `messages[].content[].type="image"` | `image_url` with data URI | base64 → `data:image/...;base64,...` |
//! | `tools[].input_schema` | `tools[].function.parameters` | Wrap in function type |
//! | `stop_sequences` | `stop` | Rename |
//!
//! # Response conversion (OpenAI → Anthropic)
//!
//! | OpenAI | Anthropic | Notes |
//! |--------|-----------|-------|
//! | `choices[0].message.content` | `content[{type:"text"}]` | String → block array |
//! | `choices[0].message.tool_calls` | `content[{type:"tool_use"}]` | Unwrap function wrapper |
//! | `choices[0].message.reasoning_content` | `content[{type:"thinking"}]` | Prepended before text |
//! | `finish_reason="stop"` | `stop_reason="end_turn"` | Value mapping |
//! | `usage.prompt_tokens` | `usage.input_tokens` | Rename |
//! | `usage.prompt_tokens_details.cached_tokens` | `usage.cache_read_input_tokens` | Rename |
//!
//! # Streaming conversion
//!
//! OpenAI uses `data: {json}\n` lines terminated by `data: [DONE]\n`.
//! Anthropic uses named SSE events: `event: message_start\ndata: {json}\n\n`.
//!
//! Each OpenAI chunk is mapped to one or more Anthropic SSE events based on content type:
//! - `delta.role` → `message_start`
//! - `delta.content` → `content_block_start` + `content_block_delta(text_delta)` + `content_block_stop`
//! - `delta.reasoning_content` → thinking block events
//! - `delta.tool_calls[i]` → `content_block_start(tool_use)` + `input_json_delta` events
//! - `finish_reason` → `message_delta(stop_reason)` + `message_stop`

use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use serde_json::Value;

use crate::context::RequestContext;
use crate::middleware::MiddlewareError;
use crate::protocol::{ConvertedRequest, ProtocolConverter};

/// Stateful converter for the OpenAI Chat Completions protocol.
///
/// Maintains atomic counters for content block indices and tool call tracking
/// across streaming chunks. Create a new instance per request.
pub struct OpenAiConverter {
    block_index: AtomicU64,
    tool_call_started: AtomicU64,
}

impl OpenAiConverter {
    /// Create a new converter with fresh state.
    pub fn new() -> Self {
        Self {
            block_index: AtomicU64::new(0),
            tool_call_started: AtomicU64::new(0),
        }
    }
}

impl Default for OpenAiConverter {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a single Anthropic message to OpenAI format.
///
/// Dispatches based on role: `assistant` → [`convert_assistant_message`],
/// `user` → [`convert_user_message`].
fn convert_message(msg: &Value) -> Value {
    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
    let content = msg.get("content");

    match role {
        "assistant" => convert_assistant_message(content),
        "user" => convert_user_message(content),
        _ => msg.clone(),
    }
}

/// Convert an assistant message: extract text, tool_use, and thinking blocks into OpenAI format.
///
/// - `text` blocks → joined into `content` string
/// - `tool_use` blocks → `tool_calls[]` with `function.name` and `function.arguments` (JSON string)
/// - `thinking` blocks → `reasoning_content` string
fn convert_assistant_message(content: Option<&Value>) -> Value {
    let mut result = serde_json::Map::new();
    result.insert("role".into(), Value::String("assistant".into()));

    let blocks = content.and_then(|c| c.as_array());
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut reasoning = String::new();

    if let Some(blocks) = blocks {
        for block in blocks.iter() {
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                        text_parts.push(t.to_string());
                    }
                }
                "tool_use" => {
                    tool_calls.push(serde_json::json!({
                        "id": block["id"],
                        "type": "function",
                        "function": {
                            "name": block["name"],
                            "arguments": serde_json::to_string(&block["input"]).unwrap_or_default(),
                        }
                    }));
                }
                "thinking" => {
                    if let Some(t) = block.get("thinking").and_then(|t| t.as_str()) {
                        reasoning = t.to_string();
                    }
                }
                _ => {}
            }
        }
    } else if let Some(text) = content.and_then(|c| c.as_str()) {
        text_parts.push(text.to_string());
    }

    if !tool_calls.is_empty() {
        result.insert("content".into(), Value::Null);
        result.insert("tool_calls".into(), Value::Array(tool_calls));
    } else {
        let text = text_parts.join("");
        result.insert("content".into(), Value::String(text));
    }

    if !reasoning.is_empty() {
        result.insert("reasoning_content".into(), Value::String(reasoning));
    }

    Value::Object(result)
}

/// Convert a user message: extract text, tool_result, and image blocks into OpenAI format.
///
/// - Plain string → direct `content` string
/// - `tool_result` block → `role="tool"` message with `tool_call_id`
/// - `image` block with base64 source → `image_url` with data URI
/// - `text` blocks → content parts array
fn convert_user_message(content: Option<&Value>) -> Value {
    if let Some(text) = content.and_then(|c| c.as_str()) {
        return serde_json::json!({"role": "user", "content": text});
    }

    let blocks = content.and_then(|c| c.as_array());
    let Some(blocks) = blocks else {
        return serde_json::json!({"role": "user", "content": ""});
    };

    if let Some(tr) = blocks.iter().find(|b| b.get("type").map_or(false, |t| t == "tool_result")) {
        let tool_use_id = tr.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
        let tr_content = tr.get("content").and_then(|c| {
            c.as_str().map(|s| Value::String(s.to_string()))
                .or_else(|| c.as_array().and_then(|arr| {
                    let texts: Vec<&str> = arr.iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect();
                    Some(Value::String(texts.join("")))
                }))
        }).unwrap_or(Value::String(String::new()));

        return serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_use_id,
            "content": tr_content,
        });
    }

    let parts: Vec<Value> = blocks.iter().map(|block| {
        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match block_type {
            "text" => serde_json::json!({"type": "text", "text": block["text"]}),
            "image" => {
                let source = block.get("source");
                let url = source.and_then(|s| {
                    let media_type = s.get("media_type").and_then(|m| m.as_str()).unwrap_or("image/png");
                    let data = s.get("data").and_then(|d| d.as_str()).unwrap_or("");
                    Some(format!("data:{};base64,{}", media_type, data))
                }).unwrap_or_default();
                serde_json::json!({"type": "image_url", "image_url": {"url": url}})
            }
            _ => block.clone(),
        }
    }).collect();

    serde_json::json!({"role": "user", "content": parts})
}

/// Format an Anthropic SSE event: `event: {name}\ndata: {json}\n\n`.
fn format_sse(event: &str, data: &Value) -> String {
    format!("event: {}\ndata: {}\n\n", event, serde_json::to_string(data).unwrap_or_default())
}

impl ProtocolConverter for OpenAiConverter {
    fn convert_request(&self, ctx: &RequestContext) -> Result<ConvertedRequest, MiddlewareError> {
        let input: Value = serde_json::from_slice(&ctx.body)
            .map_err(|e| MiddlewareError::Custom(format!("Invalid JSON body: {e}")))?;

        let mut output = serde_json::Map::new();

        // Copy simple fields
        if let Some(v) = input.get("model") { output.insert("model".into(), v.clone()); }
        if let Some(v) = input.get("max_tokens") { output.insert("max_tokens".into(), v.clone()); }
        if let Some(v) = input.get("temperature") { output.insert("temperature".into(), v.clone()); }
        if let Some(v) = input.get("top_p") { output.insert("top_p".into(), v.clone()); }
        if let Some(v) = input.get("stream") { output.insert("stream".into(), v.clone()); }

        // stop_sequences → stop
        if let Some(v) = input.get("stop_sequences") {
            output.insert("stop".into(), v.clone());
        }

        // system → first system message
        let mut messages = Vec::new();
        if let Some(sys) = input.get("system") {
            let system_content = match sys {
                Value::String(s) => s.clone(),
                Value::Array(parts) => {
                    parts.iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
                _ => String::new(),
            };
            messages.push(serde_json::json!({"role": "system", "content": system_content}));
        }

        // Convert messages
        if let Some(msgs) = input.get("messages").and_then(|m| m.as_array()) {
            for msg in msgs {
                messages.push(convert_message(msg));
            }
        }
        output.insert("messages".into(), Value::Array(messages));

        // tools: Anthropic format → OpenAI function format
        if let Some(tools) = input.get("tools").and_then(|t| t.as_array()) {
            let openai_tools: Vec<Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t["name"],
                        "description": t.get("description").unwrap_or(&Value::Null),
                        "parameters": t.get("input_schema").unwrap_or(&Value::Null),
                    }
                })
            }).collect();
            output.insert("tools".into(), Value::Array(openai_tools));
        }

        let body = serde_json::to_vec(&Value::Object(output))
            .map_err(|e| MiddlewareError::Custom(format!("Serialize error: {e}")))?;

        Ok(ConvertedRequest {
            url: String::new(),
            headers: Vec::new(),
            body: Bytes::from(body),
        })
    }

    fn convert_response(&self, body: Bytes) -> Result<Bytes, MiddlewareError> {
        let input: Value = serde_json::from_slice(&body)
            .map_err(|e| MiddlewareError::Custom(format!("Invalid response JSON: {e}")))?;

        let mut content_blocks = Vec::new();

        if let Some(reasoning) = input.get("reasoning_content").and_then(|r| r.as_str()) {
            if !reasoning.is_empty() {
                content_blocks.push(serde_json::json!({
                    "type": "thinking",
                    "thinking": reasoning,
                }));
            }
        }

        if let Some(choices) = input.get("choices").and_then(|c| c.as_array()) {
            if let Some(choice) = choices.first() {
                let message = choice.get("message").unwrap_or(&Value::Null);

                // reasoning_content at message level
                if let Some(reasoning) = message.get("reasoning_content").and_then(|r| r.as_str()) {
                    if !reasoning.is_empty() {
                        content_blocks.push(serde_json::json!({
                            "type": "thinking",
                            "thinking": reasoning,
                        }));
                    }
                }

                // tool_calls → tool_use blocks
                if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tool_calls.iter() {
                        let args_str = tc.get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|a| a.as_str())
                            .unwrap_or("{}");
                        let input_val: Value = serde_json::from_str(args_str).unwrap_or(Value::Null);
                        content_blocks.push(serde_json::json!({
                            "type": "tool_use",
                            "id": tc["id"],
                            "name": tc["function"]["name"],
                            "input": input_val,
                        }));
                    }
                }

                // text content
                if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
                    if !text.is_empty() {
                        content_blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                    }
                }
            }
        }

        let stop_reason = input.get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|c| c.get("finish_reason"))
            .and_then(|f| f.as_str())
            .map(|r| match r {
                "stop" => "end_turn",
                "length" => "max_tokens",
                "tool_calls" => "tool_use",
                "content_filter" => "end_turn",
                other => other,
            })
            .unwrap_or("end_turn");

        let usage = input.get("usage");
        let anthropic_usage = serde_json::json!({
            "input_tokens": usage.and_then(|u| u.get("prompt_tokens")).unwrap_or(&Value::Null),
            "output_tokens": usage.and_then(|u| u.get("completion_tokens")).unwrap_or(&Value::Null),
            "cache_read_input_tokens": usage
                .and_then(|u| u.get("prompt_tokens_details"))
                .and_then(|d| d.get("cached_tokens"))
                .unwrap_or(&Value::Null),
        });

        let response = serde_json::json!({
            "id": input.get("id").unwrap_or(&Value::Null),
            "type": "message",
            "role": "assistant",
            "content": content_blocks,
            "model": input.get("model").unwrap_or(&Value::Null),
            "stop_reason": stop_reason,
            "stop_sequence": Value::Null,
            "usage": anthropic_usage,
        });

        let out = serde_json::to_vec(&response)
            .map_err(|e| MiddlewareError::Custom(format!("Serialize error: {e}")))?;
        Ok(Bytes::from(out))
    }

    fn convert_stream_chunk(&self, chunk: Bytes) -> Result<Vec<String>, MiddlewareError> {
        let raw = String::from_utf8_lossy(&chunk);
        let mut events = Vec::new();

        for line in raw.lines() {
            let line = line.trim();
            if !line.starts_with("data: ") { continue; }
            let data = &line[6..];
            if data == "[DONE]" { break; }

            let parsed: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let choices = parsed.get("choices").and_then(|c| c.as_array());
            let Some(choices) = choices else { continue };
            if choices.is_empty() { continue; }

            let choice = &choices[0];
            let delta = choice.get("delta").unwrap_or(&Value::Null);
            let finish_reason = choice.get("finish_reason").and_then(|f| f.as_str());

            // role assignment → message_start
            if delta.get("role").is_some() {
                let _idx = self.block_index.fetch_add(0, Ordering::SeqCst);
                let model = parsed.get("model").unwrap_or(&Value::Null);
                events.push(format_sse("message_start", &serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": parsed.get("id").unwrap_or(&Value::Null),
                        "type": "message",
                        "role": "assistant",
                        "content": [],
                        "model": model,
                        "stop_reason": Value::Null,
                        "stop_sequence": Value::Null,
                        "usage": {"input_tokens": 0, "output_tokens": 0},
                    }
                })));
            }

            // reasoning_content → thinking content block
            if let Some(reasoning) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
                let idx = self.block_index.fetch_add(1, Ordering::SeqCst);
                events.push(format_sse("content_block_start", &serde_json::json!({
                    "type": "content_block_start",
                    "index": idx,
                    "content_block": {"type": "thinking", "thinking": ""}
                })));
                events.push(format_sse("content_block_delta", &serde_json::json!({
                    "type": "content_block_delta",
                    "index": idx,
                    "delta": {"type": "thinking_delta", "thinking": reasoning}
                })));
                events.push(format_sse("content_block_stop", &serde_json::json!({
                    "type": "content_block_stop",
                    "index": idx
                })));
            }

            // content → text content block
            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                let idx = self.block_index.fetch_add(1, Ordering::SeqCst);
                events.push(format_sse("content_block_start", &serde_json::json!({
                    "type": "content_block_start",
                    "index": idx,
                    "content_block": {"type": "text", "text": ""}
                })));
                events.push(format_sse("content_block_delta", &serde_json::json!({
                    "type": "content_block_delta",
                    "index": idx,
                    "delta": {"type": "text_delta", "text": content}
                })));
                events.push(format_sse("content_block_stop", &serde_json::json!({
                    "type": "content_block_stop",
                    "index": idx
                })));
            }

            // tool_calls
            if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tool_calls.iter() {
                    let tc_idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                    let started = self.tool_call_started.load(Ordering::SeqCst);
                    let bit = 1u64 << tc_idx;
                    let is_new = (started & bit) == 0;

                    if is_new {
                        self.tool_call_started.fetch_or(bit, Ordering::SeqCst);
                        let block_idx = self.block_index.fetch_add(1, Ordering::SeqCst);
                        events.push(format_sse("content_block_start", &serde_json::json!({
                            "type": "content_block_start",
                            "index": block_idx,
                            "content_block": {
                                "type": "tool_use",
                                "id": tc.get("id").unwrap_or(&Value::Null),
                                "name": tc.get("function").and_then(|f| f.get("name")).unwrap_or(&Value::Null),
                                "input": {}
                            }
                        })));
                    }

                    if let Some(args) = tc.get("function").and_then(|f| f.get("arguments")).and_then(|a| a.as_str()) {
                        let block_idx = self.block_index.load(Ordering::SeqCst) - 1;
                        events.push(format_sse("content_block_delta", &serde_json::json!({
                            "type": "content_block_delta",
                            "index": block_idx,
                            "delta": {"type": "input_json_delta", "partial_json": args}
                        })));
                    }
                }
            }

            // finish_reason → message_delta + message_stop
            if let Some(reason) = finish_reason {
                let stop_reason = match reason {
                    "stop" => "end_turn",
                    "length" => "max_tokens",
                    "tool_calls" => "tool_use",
                    "content_filter" => "end_turn",
                    other => other,
                };
                // close any open tool call blocks
                let started = self.tool_call_started.load(Ordering::SeqCst);
                if started > 0 {
                    let block_idx = self.block_index.load(Ordering::SeqCst) - 1;
                    events.push(format_sse("content_block_stop", &serde_json::json!({
                        "type": "content_block_stop",
                        "index": block_idx
                    })));
                }
                events.push(format_sse("message_delta", &serde_json::json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": stop_reason, "stop_sequence": Value::Null},
                    "usage": {"output_tokens": 0}
                })));
                events.push(format_sse("message_stop", &serde_json::json!({
                    "type": "message_stop"
                })));
            }
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::{HeaderMap, Method};

    fn make_ctx_with_body(body: &str) -> RequestContext {
        RequestContext::new(
            Method::POST,
            "/v1/messages".to_string(),
            HeaderMap::new(),
            Bytes::from(body.to_string()),
        )
    }

    fn parse_output(converted: &ConvertedRequest) -> Value {
        serde_json::from_slice(&converted.body).unwrap()
    }

    #[tokio::test]
    async fn t23_anthropic_request_to_openai() {
        let converter = OpenAiConverter::new();
        let ctx = make_ctx_with_body(
            r#"{"model":"claude-sonnet-4-6","max_tokens":1024,"system":"You are helpful","messages":[{"role":"user","content":"Hello"}],"tools":[{"name":"get_weather","description":"Get weather","input_schema":{"type":"object","properties":{"city":{"type":"string"}}}}]}"#,
        );

        let converted = converter.convert_request(&ctx).unwrap();
        let out = parse_output(&converted);

        // system should become first message with role "system"
        let messages = out.get("messages").unwrap().as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are helpful");

        // user message preserved
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Hello");

        // max_tokens preserved
        assert_eq!(out["max_tokens"], 1024);

        // tools wrapped in function type
        let tools = out.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
        assert_eq!(tools[0]["function"]["description"], "Get weather");
        assert_eq!(tools[0]["function"]["parameters"]["type"], "object");

        // model preserved
        assert_eq!(out["model"], "claude-sonnet-4-6");
    }

    #[tokio::test]
    async fn t24_openai_response_to_anthropic() {
        let converter = OpenAiConverter::new();
        let openai_response = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150,
                "prompt_tokens_details": {"cached_tokens": 80}
            }
        });

        let result = converter.convert_response(
            Bytes::from(serde_json::to_string(&openai_response).unwrap())
        ).unwrap();
        let out: Value = serde_json::from_slice(&result).unwrap();

        assert_eq!(out["type"], "message");
        assert_eq!(out["role"], "assistant");
        assert_eq!(out["id"], "chatcmpl-123");

        let content = out["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Hello! How can I help?");

        assert_eq!(out["stop_reason"], "end_turn");

        let usage = &out["usage"];
        assert_eq!(usage["input_tokens"], 100);
        assert_eq!(usage["output_tokens"], 50);
        assert_eq!(usage["cache_read_input_tokens"], 80);
    }

    #[tokio::test]
    async fn t26_tool_calls_conversion() {
        let converter = OpenAiConverter::new();

        // --- Request: Anthropic tool_use + tool_result → OpenAI tool_calls + tool message ---
        let ctx = make_ctx_with_body(
            r#"{"model":"m","max_tokens":1024,"messages":[
                {"role":"user","content":"What's the weather?"},
                {"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"get_weather","input":{"city":"Beijing"}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"Sunny, 25°C"}]}
            ]}"#,
        );
        let converted = converter.convert_request(&ctx).unwrap();
        let out: Value = serde_json::from_slice(&converted.body).unwrap();
        let msgs = out["messages"].as_array().unwrap();

        // assistant message should have tool_calls
        assert_eq!(msgs[1]["role"], "assistant");
        let tool_calls = msgs[1]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls[0]["type"], "function");
        assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
        let args: Value = serde_json::from_str(tool_calls[0]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["city"], "Beijing");

        // tool_result → tool role message
        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "toolu_1");
        assert_eq!(msgs[2]["content"], "Sunny, 25°C");

        // --- Response: OpenAI tool_calls → Anthropic tool_use blocks ---
        let openai_resp = serde_json::json!({
            "id": "chatcmpl-456",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{"id":"call_1","type":"function","function":{"name":"search","arguments":"{\"q\":\"rust\"}"}}]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });
        let result = converter.convert_response(
            Bytes::from(serde_json::to_string(&openai_resp).unwrap())
        ).unwrap();
        let resp: Value = serde_json::from_slice(&result).unwrap();

        assert_eq!(resp["stop_reason"], "tool_use");
        let blocks = resp["content"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "tool_use");
        assert_eq!(blocks[0]["id"], "call_1");
        assert_eq!(blocks[0]["name"], "search");
        assert_eq!(blocks[0]["input"]["q"], "rust");
    }

    #[tokio::test]
    async fn t27_thinking_block_conversion() {
        let converter = OpenAiConverter::new();

        // --- Request: Anthropic thinking → OpenAI reasoning_content ---
        let ctx = make_ctx_with_body(
            r#"{"model":"m","max_tokens":1024,"messages":[
                {"role":"user","content":"Think about this"},
                {"role":"assistant","content":[
                    {"type":"thinking","thinking":"I need to consider...","signature":"sig123"},
                    {"type":"text","text":"Here is my answer"}
                ]}
            ]}"#,
        );
        let converted = converter.convert_request(&ctx).unwrap();
        let out: Value = serde_json::from_slice(&converted.body).unwrap();
        let msgs = out["messages"].as_array().unwrap();

        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["reasoning_content"], "I need to consider...");
        assert_eq!(msgs[1]["content"], "Here is my answer");

        // --- Response: OpenAI reasoning_content → Anthropic thinking block ---
        let openai_resp = serde_json::json!({
            "id": "chatcmpl-789",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_content": "Let me reason step by step...",
                    "content": "The answer is 42"
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 20, "completion_tokens": 30}
        });
        let result = converter.convert_response(
            Bytes::from(serde_json::to_string(&openai_resp).unwrap())
        ).unwrap();
        let resp: Value = serde_json::from_slice(&result).unwrap();

        let blocks = resp["content"].as_array().unwrap();
        // thinking block should come first
        assert_eq!(blocks[0]["type"], "thinking");
        assert_eq!(blocks[0]["thinking"], "Let me reason step by step...");
        // then text block
        assert_eq!(blocks[1]["type"], "text");
        assert_eq!(blocks[1]["text"], "The answer is 42");
    }

    #[tokio::test]
    async fn t28_image_base64_conversion() {
        let converter = OpenAiConverter::new();

        let ctx = make_ctx_with_body(
            r#"{"model":"m","max_tokens":1024,"messages":[
                {"role":"user","content":[
                    {"type":"text","text":"What is in this image?"},
                    {"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo="}}
                ]}
            ]}"#,
        );
        let converted = converter.convert_request(&ctx).unwrap();
        let out: Value = serde_json::from_slice(&converted.body).unwrap();
        let msgs = out["messages"].as_array().unwrap();

        assert_eq!(msgs[0]["role"], "user");
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "What is in this image?");

        // Anthropic image → OpenAI image_url
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/png;base64,iVBORw0KGgo="
        );
    }

    #[tokio::test]
    async fn t25_openai_stream_to_anthropic() {
        let converter = OpenAiConverter::new();

        // Simulate an OpenAI streaming response with 3 chunks
        let chunk1 = "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":null},\"finish_reason\":null}]}\n\n";
        let chunk2 = "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n";
        let chunk3 = "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\n";
        let chunk4 = "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
        let chunk5 = "data: [DONE]\n\n";

        let all_events: Vec<String> = vec![chunk1, chunk2, chunk3, chunk4, chunk5]
            .iter()
            .flat_map(|c| {
                converter.convert_stream_chunk(Bytes::from(c.to_string())).unwrap()
            })
            .collect();

        // Should contain: message_start, content_block_start, content_block_delta("Hello"),
        // content_block_stop, content_block_start, content_block_delta(" world"),
        // content_block_stop, message_delta, message_stop
        let event_types: Vec<&str> = all_events.iter()
            .filter(|e| e.starts_with("event: "))
            .map(|e| e.strip_prefix("event: ").unwrap().split('\n').next().unwrap())
            .collect();

        assert!(event_types.contains(&"message_start"), "should have message_start");
        assert!(event_types.contains(&"message_delta"), "should have message_delta");
        assert!(event_types.contains(&"message_stop"), "should have message_stop");

        // Verify text content in deltas
        let text_deltas: Vec<&str> = all_events.iter()
            .filter(|e| e.contains("text_delta"))
            .filter_map(|e| {
                let start = e.find("\"text\":\"")? + 8;
                let end = e[start..].find('"')? + start;
                Some(&e[start..end])
            })
            .collect();
        assert_eq!(text_deltas, vec!["Hello", " world"]);

        // Verify stop_reason mapping
        let msg_delta = all_events.iter().find(|e| e.contains("message_delta")).unwrap();
        assert!(msg_delta.contains("end_turn"), "stop should map to end_turn");
    }
}
