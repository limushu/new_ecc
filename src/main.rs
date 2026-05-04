use std::sync::Arc;
use tokio::sync::RwLock;

use ecc_config::provider::ProviderTable;
use ecc_config::route::RouteTable;
use ecc_core::logging::{SRV_CONN_ERROR, SRV_STARTUP};
use ecc_core::middleware::Pipeline;
use ecc_core::router::RouterMiddleware;
use ecc_core::forwarder::Forwarder;
use ecc_core::rectifier::ThinkingRectifier;
use ecc_core::{ecc_error, ecc_info, ecc_warn};

#[tokio::main]
async fn main() {
    ecc_core::logging::init(ecc_core::logging::LogConfig::from_env());

    // Default to 4010/4011 to avoid conflict with existing Python ecc on 4000/4001
    let proxy_port = port_from_env("ECC_PROXY_PORT", 4010);
    let admin_port = port_from_env("ECC_ADMIN_PORT", 4011);

    // Load config
    let config_dir = dirs().expect("Cannot determine config directory");
    std::fs::create_dir_all(&config_dir).expect("Cannot create config directory");

    let routes = load_routes(&config_dir);
    let providers = load_providers(&config_dir);

    let route_table = Arc::new(RwLock::new(routes));
    let provider_table = Arc::new(RwLock::new(providers));

    // Build middleware pipeline
    let pipeline = Pipeline::new()
        .add(Arc::new(RouterMiddleware::new(Arc::clone(&route_table))))
        .add(Arc::new(ThinkingRectifier))
        .add(Arc::new(Forwarder::new(Arc::clone(&provider_table))));

    let proxy = ecc_gateway::ProxyServer::new(Arc::new(pipeline));
    let admin = ecc_gateway::AdminServer::new(
        Arc::clone(&provider_table),
        Arc::clone(&route_table),
        config_dir.join("providers.toml"),
        config_dir.join("routes.toml"),
        config_dir.join("usage"),
    );

    // Start servers
    let proxy_addr = std::net::SocketAddr::from(([127, 0, 0, 1], proxy_port));
    let admin_addr = std::net::SocketAddr::from(([127, 0, 0, 1], admin_port));

    ecc_info!(SRV_STARTUP, "proxy listening on {}", proxy_addr);
    ecc_info!(SRV_STARTUP, "admin listening on {}", admin_addr);

    let proxy_server = serve_proxy(proxy_addr, proxy);
    let admin_server = serve_admin(admin_addr, admin);

    tokio::select! {
        r = proxy_server => { if let Err(e) = r { ecc_error!(SRV_CONN_ERROR, "proxy server error: {e}"); } }
        r = admin_server => { if let Err(e) = r { ecc_error!(SRV_CONN_ERROR, "admin server error: {e}"); } }
    }
}

fn port_from_env(name: &str, default: u16) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn dirs() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(std::path::PathBuf::from(home).join(".config").join("ecc"))
}

fn load_routes(config_dir: &std::path::Path) -> RouteTable {
    let path = config_dir.join("routes.toml");
    if path.exists() {
        match ecc_config::route::load_routes(&path) {
            Ok(t) => { ecc_info!(SRV_STARTUP, "loaded routes from {}", path.display()); return t; }
            Err(e) => ecc_warn!(SRV_STARTUP, "failed to load routes: {e}"),
        }
    }
    ecc_info!(SRV_STARTUP, "using empty route table");
    RouteTable::default()
}

fn load_providers(config_dir: &std::path::Path) -> ProviderTable {
    let path = config_dir.join("providers.toml");
    if path.exists() {
        match ecc_config::provider::load_providers(&path) {
            Ok(t) => { ecc_info!(SRV_STARTUP, "loaded providers from {}", path.display()); return t; }
            Err(e) => ecc_warn!(SRV_STARTUP, "failed to load providers: {e}"),
        }
    }
    ecc_info!(SRV_STARTUP, "using empty provider table");
    ProviderTable::default()
}

async fn serve_proxy(
    addr: std::net::SocketAddr,
    proxy: ecc_gateway::ProxyServer,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let io = hyper_util::rt::TokioIo::new(stream);
        let proxy = proxy.clone();
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let proxy = proxy.clone();
                async move {
                    Ok::<_, hyper::Error>(proxy.handle(req).await)
                }
            });
            if let Err(e) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await
            {
                ecc_error!(SRV_CONN_ERROR, "proxy connection error: {e}");
            }
        });
    }
}

async fn serve_admin(
    addr: std::net::SocketAddr,
    admin: ecc_gateway::AdminServer,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let io = hyper_util::rt::TokioIo::new(stream);
        let admin = admin.clone();
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let admin = admin.clone();
                async move {
                    Ok::<_, hyper::Error>(admin.handle(req).await)
                }
            });
            if let Err(e) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await
            {
                ecc_error!(SRV_CONN_ERROR, "admin connection error: {e}");
            }
        });
    }
}
