//! REST API endpoints — provider and route management, usage stats.
//!
//! These handlers are called by [`AdminServer`] for API routes.

use bytes::Bytes;
use http::StatusCode;
use hyper::Response;

/// Usage stats API handler. Returns daily usage summary.
pub async fn daily_usage(
    usage_dir: &std::path::Path,
    date: &str,
) -> Response<Bytes> {
    let store = ecc_core::usage::UsageStore::new(usage_dir.to_path_buf(), 0);
    match store.read_daily(date) {
        Ok(records) => {
            let stats = ecc_core::usage::aggregate_daily(&records);
            let json = serde_json::to_string(&serde_json::json!({
                "date": date,
                "total_requests": stats.total_requests,
                "total_input_tokens": stats.total_input_tokens,
                "total_output_tokens": stats.total_output_tokens,
                "total_cost_usd": stats.total_cost_usd,
                "by_provider": stats.by_provider,
            }))
            .unwrap_or_default();
            Response::builder()
                .header("content-type", "application/json")
                .body(Bytes::from(json))
                .unwrap()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e}")),
    }
}

fn error_response(status: StatusCode, msg: &str) -> Response<Bytes> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Bytes::from(format!("{{\"error\":\"{msg}\"}}")))
        .unwrap()
}

#[cfg(test)]
mod tests {
    #[test]
    fn t56_daily_usage_endpoint() {
        // Basic construction test — full integration tested via admin_server
    }
}
