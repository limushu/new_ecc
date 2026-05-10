use bytes::Bytes;

use crate::context::RequestContext;
use crate::middleware::{Middleware, Next, PipelineError, PipelineResult};

/// Protocol converter trait — implementations handle Anthropic↔OpenAI conversion.
pub trait ProtocolConverter: Send + Sync {
    fn convert_request(&self, ctx: &RequestContext) -> std::result::Result<ConvertedRequest, String>;
    fn convert_response(&self, body: Bytes) -> std::result::Result<Bytes, String>;
    fn convert_stream_chunk(&self, chunk: Bytes) -> std::result::Result<Vec<String>, String>;
}

pub struct ConvertedRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

pub fn get_converter(protocol: &ecc_domain::provider::Protocol) -> Box<dyn ProtocolConverter> {
    match protocol {
        ecc_domain::provider::Protocol::Anthropic => Box::new(crate::anthropic::AnthropicConverter),
        ecc_domain::provider::Protocol::OpenAI => Box::new(crate::openai::OpenAiConverter),
    }
}

pub struct ConverterMiddleware;

impl ConverterMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Middleware for ConverterMiddleware {
    fn handle<'a>(
        &'a self,
        ctx: &'a mut RequestContext,
        next: Next<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PipelineResult> + Send + 'a>> {
        Box::pin(async move {
            let provider_config = ctx
                .provider_config
                .as_ref()
                .ok_or_else(|| PipelineError::Aborted("no provider config".into()))?;

            let converter = get_converter(&provider_config.protocol);
            let converted = converter
                .convert_request(ctx)
                .map_err(|e| PipelineError::Internal(format!("conversion failed: {e}")))?;

            ctx.upstream_url = Some(converted.url);
            ctx.upstream_headers = Some(converted.headers);
            ctx.upstream_body = Some(converted.body);

            let result = next.run(ctx).await;

            // Convert response back if non-streaming
            if result.is_ok() && !ctx.is_streaming() {
                if let Some(body) = ctx.response_body.take() {
                    ctx.response_body = Some(
                        converter.convert_response(body).map_err(|e| PipelineError::Internal(e))?,
                    );
                }
            }

            result
        })
    }
}
