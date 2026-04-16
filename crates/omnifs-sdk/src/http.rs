//! HTTP effect result extraction.

use crate::omnifs::provider::types::{ProviderResponse, SingleEffectResult};

pub fn extract_http_body(result: &SingleEffectResult) -> Result<&[u8], ProviderResponse> {
    match result {
        SingleEffectResult::HttpResponse(resp) if resp.status < 400 => Ok(&resp.body),
        SingleEffectResult::HttpResponse(resp) => {
            Err(crate::helpers::err(&format!("HTTP {}", resp.status)))
        }
        SingleEffectResult::EffectError(e) => {
            Err(crate::helpers::err(&format!("effect error: {}", e.message)))
        }
        _ => Err(crate::helpers::err("unexpected effect result type")),
    }
}
