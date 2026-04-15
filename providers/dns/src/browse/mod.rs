use std::fmt::Write;

use crate::doh;
use crate::omnifs::provider::types::*;
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

pub(crate) fn mk_dir(name: impl Into<String>) -> DirEntry {
    DirEntry { name: name.into(), kind: EntryKind::Directory, size: None, projected_files: None }
}

pub(crate) fn mk_file(name: impl Into<String>) -> DirEntry {
    DirEntry { name: name.into(), kind: EntryKind::File, size: Some(4096), projected_files: None }
}

pub(crate) fn dir_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(mk_dir(name))))
}

pub(crate) fn file_entry(name: &str) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::DirEntryOption(Some(mk_file(name))))
}

pub(crate) fn resolvers_content() -> ProviderResponse {
    let content = with_state(|s| s.resolvers.format_resolvers_file()).unwrap_or_default();
    ProviderResponse::Done(ActionResult::FileContent(content.into_bytes()))
}

pub(crate) fn resolver_dir_names() -> Vec<String> {
    with_state(|s| s.resolvers.resolver_dir_names()).unwrap_or_default()
}

// --- Resume dispatch ---

#[allow(clippy::needless_pass_by_value)]
pub fn resume(id: u64, effect_outcome: EffectResult) -> ProviderResponse {
    let continuation = match with_state(|s| s.pending.remove(&id)) {
        Ok(Some(c)) => c,
        Ok(None) => return err("no pending continuation"),
        Err(e) => return err(&e),
    };

    match continuation {
        Continuation::Single { .. } => resume_single(&effect_outcome),
        Continuation::All { results, .. } => resume_all(results, &effect_outcome),
        Continuation::Raw { ctx, .. } => resume_raw(&ctx.domain, &effect_outcome),
    }
}

fn resume_single(outcome: &EffectResult) -> ProviderResponse {
    let body = match extract_http_body(outcome) {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    match doh::parse_response(body) {
        Ok((records, _)) => file_content(format_records(&records)),
        Err(e) => file_content(format!("{e}\n")),
    }
}

fn resume_all(mut accumulated: Vec<DnsRecord>, outcome: &EffectResult) -> ProviderResponse {
    let results: &[SingleEffectResult] = match outcome {
        EffectResult::Batch(v) => v,
        EffectResult::Single(r) => std::slice::from_ref(r),
    };

    for result in results {
        if let SingleEffectResult::HttpResponse(resp) = result
            && resp.status < 400
            && let Ok((records, _)) = doh::parse_response(&resp.body)
        {
            accumulated.extend(records);
        }
    }

    file_content(format_all_records(&accumulated))
}

fn resume_raw(domain: &str, outcome: &EffectResult) -> ProviderResponse {
    let body = match extract_http_body(outcome) {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    match doh::parse_response(body) {
        Ok((records, _)) => {
            let mut out = String::new();
            let _ = writeln!(out, ";; QUESTION SECTION:\n;{domain}.\t\tIN\tA");
            out.push_str("\n;; ANSWER SECTION:\n");
            for r in &records {
                let _ = writeln!(out, "{domain}.\t\tIN\t{}\t{}", r.rtype.as_str(), r.value);
            }
            let _ = write!(out, "\n;; RECORDS: {}\n", records.len());
            file_content(out)
        }
        Err(e) => file_content(format!(";; ERROR: {e}\n")),
    }
}

// --- Helpers ---

fn file_content(s: String) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::FileContent(s.into_bytes()))
}

fn extract_http_body(outcome: &EffectResult) -> Result<&[u8], ProviderResponse> {
    let result = match outcome {
        EffectResult::Single(r) => r,
        EffectResult::Batch(v) if !v.is_empty() => &v[0],
        EffectResult::Batch(_) => return Err(err("empty batch result")),
    };
    match result {
        SingleEffectResult::HttpResponse(resp) if resp.status < 400 => Ok(&resp.body),
        SingleEffectResult::HttpResponse(resp) => Err(err(&format!("HTTP {}", resp.status))),
        SingleEffectResult::EffectError(e) => Err(err(&format!("effect error: {}", e.message))),
        _ => Err(err("unexpected effect result type")),
    }
}

fn format_records(records: &[DnsRecord]) -> String {
    if records.is_empty() {
        return "\n".to_string();
    }
    let mut out = String::new();
    for r in records {
        out.push_str(&r.value);
        out.push('\n');
    }
    out
}

fn format_all_records(records: &[DnsRecord]) -> String {
    if records.is_empty() {
        return ";; no records\n".to_string();
    }
    let mut out = String::new();
    for r in records {
        out.push_str(r.rtype.as_str());
        out.push('\t');
        out.push_str(&r.value);
        out.push('\n');
    }
    out
}
