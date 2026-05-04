//! ecc-gateway — HTTP servers for proxy and admin.
//!
//! Contains the proxy server (receives Claude Code requests on :4000) and the
//! admin server (serves Dashboard and REST API on :4001).

pub mod admin_server;
pub mod api;
pub mod playground;
pub mod proxy_server;

pub use admin_server::AdminServer;
pub use proxy_server::ProxyServer;
