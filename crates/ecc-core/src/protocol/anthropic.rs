//! Anthropic protocol pass-through converter.
//!
//! Used when the upstream provider speaks Anthropic Messages API natively (e.g. Kimi).
//! No conversion is needed — request and response bodies are forwarded as-is.

use bytes::Bytes;

use crate::context::RequestContext;
use crate::middleware::MiddlewareError;
use crate::protocol::{ConvertedRequest, ProtocolConverter};

/// Pass-through converter for Anthropic-native providers.
///
/// Forwards the request body unchanged and returns responses as-is.
pub struct AnthropicConverter;

impl ProtocolConverter for AnthropicConverter {
    fn convert_request(&self, _ctx: &RequestContext) -> Result<ConvertedRequest, MiddlewareError> {
        todo!()
    }

    fn convert_response(&self, _body: Bytes) -> Result<Bytes, MiddlewareError> {
        todo!()
    }

    fn convert_stream_chunk(&self, _chunk: Bytes) -> Result<Vec<String>, MiddlewareError> {
        todo!()
    }
}
