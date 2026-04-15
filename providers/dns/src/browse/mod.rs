use std::fmt::Write;

use crate::doh;
use crate::omnifs::provider::types::*;
use crate::path::RecordType;
use crate::{Continuation, DnsRecord, with_state};

mod routing;

pub use routing::{list_children, lookup_child, read_file};

pub(crate) fn err(msg: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::Err(msg.to_string()))
}

pub(crate) fn dispatch(id: u64, cont: Continuation, effect: SingleEffect) -> ProviderResponse {
    with_state(|s| s.pending.insert(id, cont))
        .map_or_else(|e| err(&e), |_| ProviderResponse::Effect(effect))
}

pub(crate) fn dispatch_batch(
    id: u64,
    cont: Continuation,
    effects: Vec<SingleEffect>,
) -> ProviderResponse {
    with_state(|s| s.pending.insert(id, cont))
        .map_or_else(|e| err(&e), |_| ProviderResponse::Batch(effects))
}

pub(crate) fn dir_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name: name.to_string(),
        kind: EntryKind::Directory,
        size: None,
        projected_files: None,
    })))
}

pub(crate) fn file_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(DirEntry {
        name: name.to_string(),
        kind: EntryKind::File,
        size: Some(4096),
        projected_files: None,
    })))
}

/// Format `_resolvers` content from the active config.
pub(crate) fn resolvers_content() -> ProviderResponse {
    let content = with_state(|s| s.resolvers.format_resolvers_file()).unwrap_or_default();
    ProviderResponse::Done(ActionResult::FileContent(content.into_bytes()))
}

/// Resolver directory names from config (e.g. `["@cloudflare", "@google"]`).
pub(crate) fn resolver_dir_names() -> Vec<String> {
    with_state(|s| s.resolvers.resolver_dir_names()).unwrap_or_default()
}

#[allow(clippy::needless_pass_by_value)]
pub fn resume(id: u64, effect_outcome: EffectResult) -> ProviderResponse {
    let continuation = match with_state(|s| s.pending.remove(&id)) {
        Ok(Some(c)) => c,
        Ok(None) => return err("no pending continuation"),
        Err(e) => return err(&e),
    };

    match continuation {
        Continuation::Single {
            resolver,
            domain,
            rtype,
        } => resume_single(resolver.as_deref(), &domain, rtype, &effect_outcome),
        Continuation::All {
            resolver,
            domain,
            results,
            pending_types,
        } => resume_all(
            resolver.as_deref(),
            &domain,
            results,
            &pending_types,
            &effect_outcome,
        ),
        Continuation::Raw { resolver, domain } => {
            resume_raw(resolver.as_deref(), &domain, &effect_outcome)
        }
    }
}

fn resume_single(
    _resolver: Option<&str>,
    _domain: &str,
    _rtype: RecordType,
    outcome: &EffectResult,
) -> ProviderResponse {
    let body = match extract_http_body(outcome) {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    match doh::parse_response(body) {
        Ok((records, _ttl)) => {
            let content = format_records(&records);
            ProviderResponse::Done(ActionResult::FileContent(content.into_bytes()))
        }
        Err(e) => {
            ProviderResponse::Done(ActionResult::FileContent(format!("{e}\n").into_bytes()))
        }
    }
}

fn resume_all(
    _resolver: Option<&str>,
    _domain: &str,
    mut accumulated: Vec<DnsRecord>,
    _pending_types: &[RecordType],
    outcome: &EffectResult,
) -> ProviderResponse {
    let results: &[SingleEffectResult] = match outcome {
        EffectResult::Batch(results) => results,
        EffectResult::Single(r) => std::slice::from_ref(r),
    };

    for result in results {
        let body = match result {
            SingleEffectResult::HttpResponse(resp) if resp.status < 400 => &resp.body,
            _ => continue,
        };
        if let Ok((records, _ttl)) = doh::parse_response(body) {
            accumulated.extend(records);
        }
    }

    let content = format_all_records(&accumulated);
    ProviderResponse::Done(ActionResult::FileContent(content.into_bytes()))
}

fn resume_raw(
    _resolver: Option<&str>,
    domain: &str,
    outcome: &EffectResult,
) -> ProviderResponse {
    let body = match extract_http_body(outcome) {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    match doh::parse_response(body) {
        Ok((records, _ttl)) => {
            let mut output = String::new();
            let _ = writeln!(output, ";; QUESTION SECTION:\n;{domain}.\t\tIN\tA");
            output.push_str("\n;; ANSWER SECTION:\n");
            for record in &records {
                let _ = writeln!(
                    output,
                    "{domain}.\t\tIN\t{}\t{}",
                    record.rtype.as_str(),
                    record.value,
                );
            }
            let _ = write!(output, "\n;; RECORDS: {}\n", records.len());
            ProviderResponse::Done(ActionResult::FileContent(output.into_bytes()))
        }
        Err(e) => ProviderResponse::Done(ActionResult::FileContent(
            format!(";; ERROR: {e}\n").into_bytes(),
        )),
    }
}

fn extract_http_body(outcome: &EffectResult) -> Result<&[u8], ProviderResponse> {
    let result = match outcome {
        EffectResult::Single(r) => r,
        EffectResult::Batch(v) if !v.is_empty() => &v[0],
        EffectResult::Batch(_) => return Err(err("empty batch result")),
    };

    match result {
        SingleEffectResult::HttpResponse(resp) => {
            if resp.status >= 400 {
                Err(err(&format!("HTTP {}", resp.status)))
            } else {
                Ok(&resp.body)
            }
        }
        SingleEffectResult::EffectError(e) => Err(err(&format!("effect error: {}", e.message))),
        _ => Err(err("unexpected effect result type")),
    }
}

fn format_records(records: &[DnsRecord]) -> String {
    let mut out = String::new();
    for r in records {
        out.push_str(&r.value);
        out.push('\n');
    }
    if out.is_empty() {
        out.push('\n');
    }
    out
}

fn format_all_records(records: &[DnsRecord]) -> String {
    let mut out = String::new();
    for r in records {
        out.push_str(r.rtype.as_str());
        out.push('\t');
        out.push_str(&r.value);
        out.push('\n');
    }
    if out.is_empty() {
        out.push_str(";; no records\n");
    }
    out
}
