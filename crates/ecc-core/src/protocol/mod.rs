//! Protocol conversion module — transforms between Anthropic Messages API and upstream provider formats.
//!
//! ecc receives requests in Anthropic format from Claude Code and must convert them to whatever
//! protocol the upstream provider speaks. Currently supported:
//!
//! - **Anthropic** — pass-through, no conversion needed (e.g. Kimi)
//! - **OpenAI** — full bidirectional conversion (request, response, streaming)
//!
//! # Architecture
//!
//! The conversion is split into two layers:
//!
//! 1. **Protocol layer** — selected by [`ecc_config::provider::Protocol`] in the provider config.
//!    Each protocol implements [`ProtocolConverter`].
//! 2. **Provider adaptation layer** — future: per-provider field filtering via config extensions,
//!    not hardcoded vendor names.
//!
//! # Conversion principle
//!
//! **Lenient output, strict input**:
//! - When sending to upstream, strip fields the upstream won't understand.
//! - When receiving from upstream, parse leniently and default missing fields.
//!
//! # Usage
//!
//! ```ignore
//! use ecc_core::protocol::get_converter;
//!
//! let converter = get_converter(&provider.protocol);
//! let converted = converter.convert_request(&ctx)?;
//! // send converted.body to converted.url with converted.headers
//! let response_body = converter.convert_response(upstream_bytes)?;
//! // response_body is now in Anthropic format
//! ```

pub mod anthropic;
pub mod openai;

use bytes::Bytes;

use crate::context::RequestContext;
use crate::middleware::MiddlewareError;

/// A request ready to be sent to an upstream provider.
///
/// Produced by [`ProtocolConverter::convert_request`]. Contains the full HTTP request
/// that should be sent (URL, headers, body) in the target protocol format.
pub struct ConvertedRequest {
    /// The full upstream URL (base_url + path).
    pub url: String,
    /// HTTP headers for the upstream request (e.g. Authorization, Content-Type).
    pub headers: Vec<(String, String)>,
    /// The request body serialized in the target protocol format.
    pub body: Bytes,
}

/// Bidirectional converter between Anthropic format and a specific upstream protocol.
///
/// Each upstream protocol (Anthropic, OpenAI) implements this trait to handle:
/// - **Request conversion**: Anthropic → upstream format
/// - **Response conversion**: upstream format → Anthropic format
/// - **Stream conversion**: upstream SSE chunks → Anthropic SSE events
///
/// Implementations must be `Send + Sync` so they can be shared across async tasks.
pub trait ProtocolConverter: Send + Sync {
    /// Convert an incoming Anthropic-format request into the target protocol format.
    ///
    /// Reads the original request from `ctx.body` and returns a [`ConvertedRequest`]
    /// with the transformed URL, headers, and body.
    fn convert_request(&self, ctx: &RequestContext) -> Result<ConvertedRequest, MiddlewareError>;

    /// Convert an upstream non-streaming response body back to Anthropic format.
    ///
    /// Takes the raw response bytes from the provider and returns bytes that
    /// conform to the Anthropic Messages API response schema.
    fn convert_response(&self, body: Bytes) -> Result<Bytes, MiddlewareError>;

    /// Convert an upstream streaming chunk into Anthropic SSE event lines.
    ///
    /// Each call processes one chunk from the upstream SSE stream and returns
    /// zero or more Anthropic-format SSE event strings (each including
    /// `event:` and `data:` lines, terminated by `\n\n`).
    fn convert_stream_chunk(&self, chunk: Bytes) -> Result<Vec<String>, MiddlewareError>;
}

/// Select the appropriate converter based on the provider's declared protocol.
///
/// Returns a boxed converter ready for use. The converter is stateful for streaming
/// (tracks block indices and tool call state), so create a new one per request.
pub fn get_converter(protocol: &ecc_config::provider::Protocol) -> Box<dyn ProtocolConverter> {
    match protocol {
        ecc_config::provider::Protocol::OpenAI => Box::new(openai::OpenAiConverter::new()),
        ecc_config::provider::Protocol::Anthropic => Box::new(anthropic::AnthropicConverter),
    }
}
