use std::fmt::Write;

use omnifs_sdk::prelude::*;

use crate::doh;
use crate::{Continuation, DnsRecord, with_state};

// --- Resume dispatch ---

#[allow(clippy::needless_pass_by_value)]
pub fn resume(_id: u64, cont: Continuation, outcome: EffectResult) -> ProviderResponse {
    match cont {
        Continuation::Single => resume_single(&outcome),
        Continuation::All { results } => resume_all(results, &outcome),
        Continuation::Raw { domain } => resume_raw(&domain, &outcome),
    }
}

fn resume_single(outcome: &EffectResult) -> ProviderResponse {
    let body = match extract_effect_body(outcome) {
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
    let body = match extract_effect_body(outcome) {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    match doh::parse_response(body) {
        Ok((records, _)) => {
            let mut out = String::new();
            let _ = writeln!(out, ";; QUESTION SECTION:\n;{domain}.\t\tIN\tA");
            out.push_str("\n;; ANSWER SECTION:\n");
            for r in &records {
                let _ = writeln!(out, "{domain}.\t\tIN\t{}\t{}", r.rtype.as_ref(), r.value);
            }
            let _ = write!(out, "\n;; RECORDS: {}\n", records.len());
            file_content(out)
        }
        Err(e) => file_content(format!(";; ERROR: {e}\n")),
    }
}

// --- Helpers ---

pub(crate) fn file_content(s: String) -> ProviderResponse {
    ProviderResponse::Done(ActionResult::FileContent(s.into_bytes()))
}

pub(crate) fn resolvers_content() -> ProviderResponse {
    let content = with_state(|s| s.resolvers.format_resolvers_file()).unwrap_or_default();
    ProviderResponse::Done(ActionResult::FileContent(content.into_bytes()))
}

pub(crate) fn resolver_dir_names() -> Vec<String> {
    with_state(|s| s.resolvers.resolver_dir_names()).unwrap_or_default()
}

pub(crate) fn format_records(records: &[DnsRecord]) -> String {
    if records.is_empty() {
        return "\n".to_string();
    }
    records
        .iter()
        .map(|r| &*r.value)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

pub(crate) fn format_all_records(records: &[DnsRecord]) -> String {
    if records.is_empty() {
        return ";; no records\n".to_string();
    }
    records
        .iter()
        .map(|r| format!("{}\t{}", r.rtype.as_ref(), r.value))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}
