//! End-to-end integration tests.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use bytes::Bytes;
use http::{HeaderMap, Method};
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::Response;
use hyper_util::rt::TokioIo;

use ecc_config::provider::{AuthType, Provider, ProviderTable, Protocol};
use ecc_config::route::{RouteEntry, RouteTable, RouteTarget};
use ecc_core::context::RequestContext;
use ecc_core::forwarder::Forwarder;
use ecc_core::middleware::Pipeline;
use ecc_core::rectifier::ThinkingRectifier;
use ecc_core::router::RouterMiddleware;

/// Start a mock server that returns a valid OpenAI response.
async fn start_mock_ok_server() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let resp = serde_json::json!({
                    "id": "chatcmpl-mock",
                    "object": "chat.completion",
                    "model": "deepseek-chat",
                    "choices": [{"index":0,"message":{"role":"assistant","content":"Hello!"},"finish_reason":"stop"}],
                    "usage": {"prompt_tokens": 20, "completion_tokens": 10, "total_tokens": 30, "prompt_tokens_details": {"cached_tokens": 5}}
                });
                let service = service_fn(|_req| async {
                    Ok::<_, hyper::Error>(Response::builder()
                        .header("content-type", "application/json")
                        .body(Full::new(Bytes::from(serde_json::to_string(&resp).unwrap())))
                        .unwrap())
                });
                http1::Builder::new().serve_connection(io, service).await.unwrap();
            });
        }
    });

    port
}

fn make_pipeline(mock_port: u16) -> (Arc<Pipeline>, Arc<RwLock<ProviderTable>>) {
    let mut routes_map = HashMap::new();
    routes_map.insert(
        "claude-sonnet-4-6".to_string(),
        RouteEntry {
            targets: vec![RouteTarget {
                provider: "mock".to_string(),
                model: "deepseek-chat".to_string(),
                priority: 1,
            }],
        },
    );
    let route_table = Arc::new(RwLock::new(RouteTable { routes: routes_map }));

    let mut providers = HashMap::new();
    providers.insert("mock".to_string(), Provider {
        base_url: format!("http://127.0.0.1:{mock_port}"),
        auth_token: "sk-mock".to_string(),
        auth_type: AuthType::Bearer,
        protocol: Protocol::OpenAI,
    });
    let provider_table = Arc::new(RwLock::new(ProviderTable { providers }));

    let pipeline = Pipeline::new()
        .add(Arc::new(RouterMiddleware::new(route_table)))
        .add(Arc::new(ThinkingRectifier))
        .add(Arc::new(Forwarder::new(Arc::clone(&provider_table))));

    (Arc::new(pipeline), provider_table)
}

#[tokio::test]
async fn t63_full_request_lifecycle() {
    let port = start_mock_ok_server().await;
    let (pipeline, _) = make_pipeline(port);

    let mut ctx = RequestContext::new(
        Method::POST,
        "/v1/messages".to_string(),
        HeaderMap::new(),
        Bytes::from(r#"{"model":"claude-sonnet-4-6","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}"#),
    );

    let result = pipeline.execute(&mut ctx).await;
    assert!(result.is_ok(), "Pipeline should succeed");

    assert_eq!(ctx.requested_model, Some("claude-sonnet-4-6".to_string()));
    assert_eq!(ctx.resolved_target.as_ref().unwrap().provider, "mock");
    assert_eq!(ctx.response_status, Some(200));

    let usage = ctx.usage.as_ref().expect("Should have usage");
    assert_eq!(usage.input_tokens, 20);
    assert_eq!(usage.output_tokens, 10);
    assert_eq!(usage.cache_read_tokens, 5);
}

#[tokio::test]
async fn t64_failover_e2e() {
    // Start a server that always returns 500
    let fail_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fail_port = fail_listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (stream, _) = fail_listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let service = service_fn(|_req| async {
                    Ok::<_, hyper::Error>(Response::builder().status(500)
                        .body(Full::new(Bytes::from("{\"error\":\"fail\"}"))).unwrap())
                });
                http1::Builder::new().serve_connection(io, service).await.unwrap();
            });
        }
    });

    // Start a server that always returns 200
    let ok_port = start_mock_ok_server().await;

    let mut providers = HashMap::new();
    providers.insert("primary".to_string(), Provider {
        base_url: format!("http://127.0.0.1:{fail_port}"),
        auth_token: "sk".to_string(),
        auth_type: AuthType::Bearer,
        protocol: Protocol::OpenAI,
    });
    providers.insert("backup".to_string(), Provider {
        base_url: format!("http://127.0.0.1:{ok_port}"),
        auth_token: "sk".to_string(),
        auth_type: AuthType::Bearer,
        protocol: Protocol::OpenAI,
    });
    let provider_table = Arc::new(RwLock::new(ProviderTable { providers }));

    let pipeline = Pipeline::new()
        .with_max_retries(3)
        .add(Arc::new(Forwarder::new(provider_table)));

    let mut ctx = RequestContext::new(
        Method::POST,
        "/v1/messages".to_string(),
        HeaderMap::new(),
        Bytes::from(r#"{"model":"m","messages":[]}"#),
    );
    ctx.resolved_target = Some(RouteTarget { provider: "primary".to_string(), model: "m1".to_string(), priority: 1 });
    ctx.fallback_targets.push(RouteTarget { provider: "backup".to_string(), model: "m2".to_string(), priority: 2 });

    let result = pipeline.execute(&mut ctx).await;
    assert!(result.is_ok(), "Should succeed after failover");
    assert_eq!(ctx.retry_count, 1);
}

#[tokio::test]
async fn t65_usage_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let port = start_mock_ok_server().await;
    let (pipeline, _) = make_pipeline(port);

    let mut ctx = RequestContext::new(
        Method::POST,
        "/v1/messages".to_string(),
        HeaderMap::new(),
        Bytes::from(r#"{"model":"claude-sonnet-4-6","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}"#),
    );

    pipeline.execute(&mut ctx).await.unwrap();

    let usage = ctx.usage.as_ref().unwrap();
    let store = ecc_core::usage::UsageStore::new(dir.path().to_path_buf(), 100);
    let record = ecc_core::usage::UsageRecord {
        ts: "2026-05-03T10:00:00Z".to_string(),
        req_id: ctx.id.to_string(),
        model: "claude-sonnet-4-6".to_string(),
        provider: "mock".to_string(),
        target_model: "deepseek-chat".to_string(),
        input_tokens: usage.input_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        output_tokens: usage.output_tokens,
        latency_ms: 100,
        status: 200,
        cost_usd: 0.001,
    };
    store.record(record).unwrap();
    store.flush().unwrap();

    let records = store.read_daily("2026-05-03").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].input_tokens, 20);
    assert_eq!(records[0].output_tokens, 10);
}
