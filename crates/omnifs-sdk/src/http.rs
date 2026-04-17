//! HTTP effect result extraction.

use crate::error::ProviderError;
use crate::omnifs::provider::types::{EffectResult, ProviderResponse, SingleEffectResult};

pub fn extract_http_body(result: &SingleEffectResult) -> Result<&[u8], ProviderResponse> {
    match result {
        SingleEffectResult::HttpResponse(resp) if resp.status < 400 => Ok(&resp.body),
        SingleEffectResult::HttpResponse(resp) => Err(crate::helpers::err(
            ProviderError::from_http_status(resp.status),
        )),
        SingleEffectResult::EffectError(e) => {
            Err(crate::helpers::err(ProviderError::from_effect_error(e)))
        }
        _ => Err(crate::helpers::err(ProviderError::internal(
            "unexpected effect result type",
        ))),
    }
}

pub fn extract_effect_body(result: &EffectResult) -> Result<&[u8], ProviderResponse> {
    let result = match result {
        EffectResult::Single(r) => r,
        EffectResult::Batch(v) if !v.is_empty() => &v[0],
        EffectResult::Batch(_) => {
            return Err(crate::helpers::err(ProviderError::internal(
                "empty batch result",
            )));
        }
    };
    extract_http_body(result)
}
