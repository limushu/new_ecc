use std::sync::Arc;

use ecc_api::{AdminServer, ProxyServer};
use ecc_app::provider_service::ProviderService;
use ecc_app::preset_service::PresetService;
use ecc_app::quota_service::QuotaService;
use ecc_app::usage_service::UsageService;
use ecc_app::PlaygroundService;
use ecc_engine::circuit_breaker::CircuitBreaker;
use ecc_engine::forwarder::Forwarder;
use ecc_engine::middleware::Pipeline;
use ecc_engine::rectifier::ThinkingRectifier;
use ecc_engine::reqwest_forwarder::ReqwestForwarder;
use ecc_engine::router::Router;
use ecc_engine::usage_tracker::UsageTracker;
use ecc_domain::repository::RouteRepository;
use ecc_engine::circuit_breaker::CircuitBreakerConfig;
use ecc_infra::{ConfigRepo, PresetRepo, ProviderRepo, RouteRepo, SqliteRepo, UsageRepo};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("info".parse().unwrap()),
        )
        .init();

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async_main());
}

async fn async_main() {
    let db_path = std::env::var("ECC_DB_PATH").unwrap_or_else(|_| "ecc.db".into());
    let proxy_port: u16 = std::env::var("ECC_PROXY_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9090);
    let admin_port: u16 = std::env::var("ECC_ADMIN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let crypto_seed =
        std::env::var("ECC_CRYPTO_SEED").unwrap_or_else(|_| "ecc-default-seed".into());

    let store = Arc::new(
        SqliteRepo::open(db_path.as_ref(), &crypto_seed).expect("failed to open database"),
    );

    // Build repos
    let provider_repo = Arc::new(ProviderRepo::new(store.clone()).expect("failed to init provider repo"));
    let route_repo = Arc::new(RouteRepo::new(provider_repo.clone()).expect("failed to init route repo"));
    let config_repo = Arc::new(ConfigRepo::new(store.clone()));
    let preset_repo = Arc::new(PresetRepo::new(store.clone()));
    let usage_repo = Arc::new(UsageRepo::new(store.clone()));

    // DB seed
    let seed_count =
        ecc_infra::seed::seed_if_empty(&*preset_repo).expect("failed to seed presets");
    if seed_count > 0 {
        tracing::info!("seeded {seed_count} presets");
    }

    // Build route table
    route_repo.rebuild().expect("failed to build routes");
    tracing::info!("route table built");

    // Services
    let provider_service = Arc::new(ProviderService::new(
        provider_repo.clone(),
        config_repo,
        route_repo.clone(),
    ));
    let preset_service = Arc::new(PresetService::new(preset_repo));
    let usage_service = Arc::new(UsageService::new(usage_repo.clone()));
    let quota_service = Arc::new(QuotaService::new());
    let playground_service = Arc::new(PlaygroundService::new());

    // Pipeline
    let reqwest_client = reqwest::Client::new();
    let forward_port = Arc::new(ReqwestForwarder::new(reqwest_client.clone()));
    let pipeline = Arc::new(
        Pipeline::new()
            .with_max_retries(2)
            .add(Arc::new(Router::new(route_repo.clone())))
            .add(Arc::new(ThinkingRectifier::new()))
            .add(Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
                failure_threshold: 5,
                cooldown: std::time::Duration::from_secs(30),
            })))
            .add(Arc::new(Forwarder::new(forward_port)))
            .add(Arc::new(UsageTracker::new(usage_repo))),
    );

    let proxy_server = ProxyServer::new(pipeline);
    let admin_server = Arc::new(AdminServer::new(
        provider_service,
        preset_service,
        usage_service,
        quota_service,
        playground_service,
        reqwest_client,
    ));

    tracing::info!("proxy listening on :{proxy_port}");
    tracing::info!("admin listening on :{admin_port}");

    let proxy_handle = tokio::spawn(run_proxy(proxy_port, proxy_server));
    let admin_handle = tokio::spawn(run_admin(admin_port, admin_server));

    tokio::select! {
        r = proxy_handle => tracing::info!("proxy exited: {:?}", r),
        r = admin_handle => tracing::info!("admin exited: {:?}", r),
    }
}

async fn run_proxy(port: u16, server: ProxyServer) {
    let listener = TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind proxy port");

    loop {
        let (stream, _) = listener.accept().await.expect("failed to accept");
        let io = TokioIo::new(stream);
        let server = server.clone();

        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let server = server.clone();
                async move { Ok::<_, std::convert::Infallible>(server.handle(req).await) }
            });

            if let Err(e) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await
            {
                tracing::debug!("proxy connection error: {e}");
            }
        });
    }
}

async fn run_admin<P, C, R, U, PR>(port: u16, server: Arc<AdminServer<P, C, R, U, PR>>)
where
    P: ecc_domain::repository::ProviderRepository + 'static,
    C: ecc_domain::repository::ConfigRepository + 'static,
    R: ecc_domain::repository::RouteRepository + 'static,
    U: ecc_domain::repository::UsageRepository + 'static,
    PR: ecc_domain::repository::PresetRepository + 'static,
{
    let listener = TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind admin port");

    loop {
        let (stream, _) = listener.accept().await.expect("failed to accept");
        let io = TokioIo::new(stream);
        let server = server.clone();

        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let server = server.clone();
                async move { Ok::<_, std::convert::Infallible>(server.handle(req).await) }
            });

            if let Err(e) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await
            {
                tracing::debug!("admin connection error: {e}");
            }
        });
    }
}
