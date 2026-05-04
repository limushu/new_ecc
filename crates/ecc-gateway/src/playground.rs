//! Playground — test provider connectivity by sending a sample request.

use bytes::Bytes;
use http::StatusCode;
use hyper::Response;

use ecc_core::{ecc_error, ecc_info};
use ecc_core::logging::{PG_REQUEST, PG_ERROR};

/// Send a test message to a provider and return the response.
/// Used by the Dashboard playground feature.
pub async fn test_provider(
    client: &reqwest::Client,
    base_url: &str,
    auth_token: &str,
    auth_type: &str,
    protocol: &str,
    model: &str,
    message: &str,
) -> Response<Bytes> {
    let base = base_url.trim_end_matches('/');

    let (url, body) = if protocol == "anthropic" {
        (
            format!("{base}/v1/messages"),
            serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": message}],
                "max_tokens": 1024,
            }),
        )
    } else {
        (
            format!("{base}/v1/chat/completions"),
            serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": message}],
                "max_tokens": 1024,
            }),
        )
    };

    let auth_value = if auth_type == "api_key" {
        auth_token.to_string()
    } else {
        format!("Bearer {auth_token}")
    };

    ecc_info!(
        PG_REQUEST,
        url = %url,
        model = %model,
        protocol = %protocol,
        "playground request"
    );

    match client
        .post(&url)
        .header("content-type", "application/json")
        .header("authorization", &auth_value)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let resp_body = resp
                .bytes()
                .await
                .unwrap_or_else(|_| Bytes::from("{\"error\":\"failed to read response\"}"));

            if status >= 400 {
                ecc_error!(
                    PG_ERROR,
                    url = %url,
                    status = %status,
                    response = %String::from_utf8_lossy(&resp_body),
                    "playground upstream error"
                );
            }

            Response::builder()
                .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
                .header("content-type", "application/json")
                .body(resp_body)
                .unwrap()
        }
        Err(e) => {
            ecc_error!(PG_ERROR, url = %url, error = %e, "playground connection failed");
            Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .header("content-type", "application/json")
                .body(Bytes::from(format!("{{\"error\":\"{e}\"}}")))
                .unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn t59_playground_unreachable() {
        let client = reqwest::Client::new();
        let resp = test_provider(
            &client,
            "http://127.0.0.1:1",
            "sk-test",
            "bearer",
            "openai",
            "test-model",
            "Hello",
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }
}
