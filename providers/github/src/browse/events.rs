//! Timer-driven refresh and webhook-style event polling.
//!
//! Handles periodic cache refresh, event polling via `ETags`, and cache
//! invalidation based on repository activity.

use super::{enter_cache_only, err, truncate_content, with_state};
use crate::Continuation;
use crate::api;
use crate::omnifs::provider::types::*;
use crate::path::FsPath;

pub fn timer_tick(id: u64) -> ProviderResponse {
    let pending_invalidations = with_state(|state| {
        std::mem::take(&mut state.pending_host_invalidations)
    }).unwrap_or_default();

    let repos = with_state(|state| {
        state.cache.advance_tick();
        // Prune stale active repos
        let tick = state.cache.current_tick();
        state
            .active_repos
            .retain(|_, &mut last_touch| tick.saturating_sub(last_touch) < super::ACTIVE_REPO_TTL);
        if state.cache_only {
            return Vec::new();
        }
        state.active_repos.keys().cloned().collect::<Vec<_>>()
    })
    .unwrap_or_default();

    // Event fetches first (one per repo), invalidations appended after.
    // This keeps the 1:1 alignment between repos[i] and results[i].
    let mut effects: Vec<SingleEffect> = repos
        .iter()
        .filter_map(|repo| {
            let (owner, name) = repo.split_once('/')?;
            Some(events_fetch(owner, name, event_etag(repo)))
        })
        .collect();

    let invalidation_count = pending_invalidations.len();
    effects.extend(pending_invalidations.into_iter().map(|prefix| {
        SingleEffect::CacheInvalidatePrefix(CacheInvalidateRequest { prefix })
    }));

    if effects.is_empty() {
        return ProviderResponse::Done(ActionResult::Ok);
    }

    match with_state(|state| {
        state
            .pending
            .insert(id, Continuation::FetchingEvents { repos, invalidation_count });
    }) {
        Ok(()) => ProviderResponse::Batch(effects),
        Err(e) => err(&e),
    }
}

pub fn event_etag(repo: &str) -> Option<String> {
    with_state(|state| state.event_etags.get(repo).cloned()).unwrap_or(None)
}

pub fn events_fetch(owner: &str, repo: &str, etag: Option<String>) -> SingleEffect {
    let mut headers = vec![
        Header {
            name: "Accept".to_string(),
            value: "application/vnd.github+json".to_string(),
        },
        Header {
            name: "X-GitHub-Api-Version".to_string(),
            value: "2022-11-28".to_string(),
        },
    ];
    if let Some(etag) = etag {
        headers.push(Header {
            name: "If-None-Match".to_string(),
            value: etag,
        });
    }
    SingleEffect::Fetch(HttpRequest {
        method: "GET".to_string(),
        url: format!("https://api.github.com/repos/{owner}/{repo}/events?per_page=30"),
        headers,
        body: None,
    })
}

pub fn resume_events(repos: &[String], invalidation_count: usize, effect_outcome: &EffectResult) -> ProviderResponse {
    let all_results = match effect_outcome {
        EffectResult::Batch(results) => results,
        EffectResult::Single(result) => std::slice::from_ref(result),
    };

    // The first repos.len() results correspond to event fetches;
    // the trailing invalidation_count results are CacheOk acks (ignore).
    if all_results.len() != repos.len() + invalidation_count {
        crate::omnifs::provider::log::log(&LogEntry {
            level: LogLevel::Warn,
            message: format!(
                "resume_events: result count mismatch: {} results, {} repos, {} invalidations",
                all_results.len(), repos.len(), invalidation_count
            ),
        });
    }
    let event_results = &all_results[..all_results.len().saturating_sub(invalidation_count)];

    let mut invalidation_prefixes: Vec<String> = Vec::new();

    for (repo, result) in repos.iter().zip(event_results.iter()) {
        let SingleEffectResult::HttpResponse(resp) = result else {
            continue;
        };
        if resp.status == 401 {
            enter_cache_only();
            continue;
        }
        if resp.status == 304 || resp.status >= 400 {
            continue;
        }
        if let Some(etag) = header_value(&resp.headers, "etag") {
            let _ = with_state(|state| {
                state.event_etags.insert(repo.clone(), etag);
            });
        }
        let Ok(json) = api::parse_json(&resp.body) else {
            continue;
        };
        let Some(events) = json.as_array() else {
            continue;
        };
        for event in events {
            invalidate_from_event(repo, event, &mut invalidation_prefixes);
        }
    }

    // Queue collected invalidation prefixes for the next timer tick.
    if !invalidation_prefixes.is_empty() {
        invalidation_prefixes.sort();
        invalidation_prefixes.dedup();
        let _ = with_state(|state| {
            state.pending_host_invalidations.extend(invalidation_prefixes);
        });
    }

    ProviderResponse::Done(ActionResult::Ok)
}

pub fn header_value(headers: &[Header], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.clone())
}

pub fn invalidate_from_event(repo: &str, event: &serde_json::Value, host_prefixes: &mut Vec<String>) {
    let Some((owner, name)) = repo.split_once('/') else {
        return;
    };
    let event_type = event
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    match event_type {
        "IssuesEvent" => {
            let prefix = format!("{owner}/{name}/issues/");
            let _ = with_state(|state| state.cache.remove_prefix(&prefix));
            host_prefixes.push(format!("{owner}/{name}/_issues/"));
        }
        "PullRequestEvent" => {
            let prefix = format!("{owner}/{name}/pulls/");
            let _ = with_state(|state| state.cache.remove_prefix(&prefix));
            host_prefixes.push(format!("{owner}/{name}/_prs/"));
        }
        "WorkflowRunEvent" => {
            let prefix = format!("{owner}/{name}/actions/runs/");
            let _ = with_state(|state| state.cache.remove_prefix(&prefix));
            host_prefixes.push(format!("{owner}/{name}/_actions/runs/"));
        }
        "IssueCommentEvent" => {
            if let Some(issue_number) = event
                .get("payload")
                .and_then(|payload| payload.get("issue"))
                .and_then(|issue| issue.get("number"))
                .and_then(serde_json::Value::as_u64)
            {
                let key = format!("{owner}/{name}/issues/{issue_number}/comments");
                let _ = with_state(|state| state.cache.remove(&key));
            }
            // GitHub uses the issues API for both issue and PR comments,
            // so invalidate both browse namespaces.
            host_prefixes.push(format!("{owner}/{name}/_issues/"));
            host_prefixes.push(format!("{owner}/{name}/_prs/"));
        }
        _ => {}
    }
}

pub fn resume_diff(path: &str, result: &SingleEffectResult) -> ProviderResponse {
    let cache_key = FsPath::parse(path).and_then(|p| match p {
        FsPath::ResourceFile {
            owner,
            repo,
            number,
            ..
        } => Some(format!("{owner}/{repo}/pulls/{number}/diff")),
        _ => None,
    });
    match result {
        SingleEffectResult::HttpResponse(resp) => {
            if resp.status == 401 {
                enter_cache_only();
                if let Some(cache_key) = &cache_key
                    && let Ok(Some(data)) =
                        with_state(|state| state.cache.get(cache_key).map(<[u8]>::to_vec))
                {
                    return ProviderResponse::Done(ActionResult::FileContent(data));
                }
                return err("diff not found in cache");
            }
            if resp.status >= 400 {
                return err(&format!("diff API error: {}", resp.status));
            }
            let content = truncate_content(resp.body.clone());
            if let Some(cache_key) = cache_key {
                let _ = with_state(|state| state.cache.set(cache_key, content.clone()));
            }
            ProviderResponse::Done(ActionResult::FileContent(content))
        }
        SingleEffectResult::EffectError(e) => err(&format!("diff fetch failed: {}", e.message)),
        _ => err("unexpected result"),
    }
}

pub fn resume_run_log(path: &str, result: &SingleEffectResult) -> ProviderResponse {
    let log_cache_key = FsPath::parse(path).and_then(|p| match p {
        FsPath::ActionRunFile {
            owner,
            repo,
            run_id,
            ..
        } => Some(format!("{owner}/{repo}/actions/runs/{run_id}/log")),
        _ => None,
    });
    let body = match result {
        SingleEffectResult::HttpResponse(resp) => {
            if resp.status == 401 {
                enter_cache_only();
                if let Some(cache_key) = &log_cache_key
                    && let Ok(Some(data)) =
                        with_state(|state| state.cache.get(cache_key).map(<[u8]>::to_vec))
                {
                    return ProviderResponse::Done(ActionResult::FileContent(data));
                }
                return err("log not found in cache");
            }
            if resp.status >= 400 {
                return err(&format!("log API error: {}", resp.status));
            }
            &resp.body
        }
        SingleEffectResult::EffectError(e) => {
            return err(&format!("log fetch failed: {}", e.message));
        }
        _ => return err("unexpected result"),
    };

    let content = unzip_logs(body);

    // Cache the extracted log.
    if let Some(cache_key) = log_cache_key {
        let _ = with_state(|state| state.cache.set(cache_key, content.clone()));
    }

    ProviderResponse::Done(ActionResult::FileContent(truncate_content(content)))
}

pub fn unzip_logs(bytes: &[u8]) -> Vec<u8> {
    use rc_zip_sync::ReadZip;

    let Ok(archive) = bytes.read_zip() else {
        return bytes.to_vec();
    };
    let mut output = Vec::new();
    for entry in archive.entries() {
        if entry.name.ends_with('/') {
            continue;
        }
        output.extend_from_slice(format!("=== {} ===\n", entry.name).as_bytes());
        if let Ok(data) = entry.bytes() {
            output.extend_from_slice(&data);
        }
        if !output.ends_with(b"\n") {
            output.push(b'\n');
        }
        if output.len() >= super::MAX_CONTENT_SIZE {
            return truncate_content(output);
        }
    }
    output
}
