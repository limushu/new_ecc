//! Thinking block rectifier — ensures assistant messages have thinking blocks when expected.
//!
//! Some upstream providers (e.g. DeepSeek) only emit `reasoning_content` on the first response
//! in a multi-turn conversation. Claude Code, however, expects every assistant message that was
//! generated with thinking enabled to include a thinking block. This middleware patches the
//! request body before forwarding: if the latest assistant message is missing a thinking block
//! but the conversation context suggests thinking was used, an empty one is inserted.

use crate::context::RequestContext;
use crate::middleware::{BoxFuture, Middleware, MiddlewareError, Next};
use serde_json::Value;

/// Middleware that patches missing thinking blocks in the request body.
///
/// When Claude Code sends a multi-turn conversation where earlier assistant messages
/// have thinking blocks but the latest one doesn't, this middleware inserts an empty
/// thinking block to maintain consistency.
pub struct ThinkingRectifier;

impl Middleware for ThinkingRectifier {
    fn handle<'a>(
        &'a self,
        ctx: &'a mut RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<(), MiddlewareError>> {
        if let Ok(mut body) = serde_json::from_slice::<Value>(&ctx.body) {
            if rectify_thinking(&mut body) {
                if let Ok(new_body) = serde_json::to_vec(&body) {
                    ctx.body = new_body.into();
                }
            }
        }
        Box::pin(async move { next.run(ctx).await })
    }
}

/// Check if any assistant message has a thinking block. If so, ensure all assistant
/// messages have one — inserting empty thinking blocks where missing.
///
/// Returns `true` if any modifications were made.
fn rectify_thinking(body: &mut Value) -> bool {
    let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return false;
    };

    let has_thinking = messages.iter().any(|msg| {
        msg.get("role") == Some(&Value::String("assistant".into()))
            && msg.get("content").and_then(|c| c.as_array()).map_or(false, |blocks| {
                blocks.iter().any(|b| b.get("type") == Some(&Value::String("thinking".into())))
            })
    });

    if !has_thinking {
        return false;
    }

    let mut modified = false;
    for msg in messages.iter_mut() {
        if msg.get("role") != Some(&Value::String("assistant".into())) {
            continue;
        }

        let Some(content) = msg.get_mut("content") else { continue };
        let Some(blocks) = content.as_array_mut() else { continue };

        let already_has = blocks.iter().any(|b| b.get("type") == Some(&Value::String("thinking".into())));
        if !already_has {
            blocks.insert(0, serde_json::json!({
                "type": "thinking",
                "thinking": "",
            }));
            modified = true;
        }
    }

    modified
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::{HeaderMap, Method};
    use std::sync::Arc;

    fn make_ctx_with_body(body: &str) -> RequestContext {
        RequestContext::new(
            Method::POST,
            "/v1/messages".to_string(),
            HeaderMap::new(),
            Bytes::from(body.to_string()),
        )
    }

    #[test]
    fn t29_empty_thinking_block_inserted() {
        let mut body = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "Let me think..."},
                    {"type": "text", "text": "Hi!"}
                ]},
                {"role": "user", "content": "More"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Sure!"}
                ]}
            ]
        });

        let changed = rectify_thinking(&mut body);
        assert!(changed);

        let msgs = body["messages"].as_array().unwrap();
        let last_assistant = &msgs[3];
        let blocks = last_assistant["content"].as_array().unwrap();
        // Empty thinking block should be inserted at index 0
        assert_eq!(blocks[0]["type"], "thinking");
        assert_eq!(blocks[0]["thinking"], "");
        // Original text block shifted to index 1
        assert_eq!(blocks[1]["text"], "Sure!");
    }

    #[test]
    fn t30_existing_thinking_unchanged() {
        let mut body = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "I should help."},
                    {"type": "text", "text": "Hi!"}
                ]}
            ]
        });

        let changed = rectify_thinking(&mut body);
        assert!(!changed);

        let msgs = body["messages"].as_array().unwrap();
        let blocks = msgs[1]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["thinking"], "I should help.");
    }

    #[tokio::test]
    async fn t29_no_thinking_at_all_skipped() {
        let mut ctx = make_ctx_with_body(
            r#"{"model":"m","messages":[
                {"role":"user","content":"Hi"},
                {"role":"assistant","content":[{"type":"text","text":"Hello"}]}
            ]}"#,
        );

        let pipeline = crate::middleware::Pipeline::new()
            .add(Arc::new(ThinkingRectifier));

        pipeline.execute(&mut ctx).await.unwrap();

        let body: Value = serde_json::from_slice(&ctx.body).unwrap();
        let blocks = body["messages"][1]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "text");
    }
}
