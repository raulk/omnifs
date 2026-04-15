use super::{dir_entry, dispatch, dispatch_batch, err, file_entry, resolver_dir_names, resolvers_content};
use crate::doh;
use crate::omnifs::provider::types::*;
use crate::path::{FsPath, RecordType};
use crate::{Continuation, QueryContext, with_state};

pub fn lookup_child(_id: u64, parent_path: &str, name: &str) -> ProviderResponse {
    let full_path = if parent_path.is_empty() {
        name.to_string()
    } else {
        format!("{parent_path}/{name}")
    };

    let Some(fs_path) = FsPath::parse(&full_path) else {
        return ProviderResponse::Done(ActionResult::DirEntryOption(None));
    };

    match fs_path {
        FsPath::Root => dir_entry(name),
        FsPath::Resolvers => file_entry("_resolvers"),
        FsPath::ReverseRoot => dir_entry("_reverse"),
        FsPath::ReverseIp { .. } | FsPath::Resolver { .. } | FsPath::Domain { .. } => {
            dir_entry(name)
        }
        FsPath::Record { .. } => file_entry(name),
        FsPath::All { .. } => file_entry("_all"),
        FsPath::Raw { .. } => file_entry("_raw"),
    }
}

pub fn list_children(_id: u64, path: &str) -> ProviderResponse {
    let Some(fs_path) = FsPath::parse(path) else {
        return err("invalid path");
    };

    match fs_path {
        FsPath::Root => list_root(),
        FsPath::Domain { .. } => list_domain(),
        FsPath::Resolver { .. } | FsPath::ReverseRoot => {
            ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries: vec![],
                exhaustive: false,
            }))
        }
        FsPath::ReverseIp { .. } => {
            ProviderResponse::Done(ActionResult::DirEntries(DirListing {
                entries: vec![DirEntry {
                    name: "PTR".to_string(),
                    kind: EntryKind::File,
                    size: Some(4096),
                    projected_files: None,
                }],
                exhaustive: true,
            }))
        }
        _ => err("not a directory"),
    }
}

fn list_root() -> ProviderResponse {
    let mut entries = vec![
        DirEntry {
            name: "_resolvers".to_string(),
            kind: EntryKind::File,
            size: None,
            projected_files: None,
        },
        DirEntry {
            name: "_reverse".to_string(),
            kind: EntryKind::Directory,
            size: None,
            projected_files: None,
        },
    ];

    for name in resolver_dir_names() {
        entries.push(DirEntry {
            name,
            kind: EntryKind::Directory,
            size: None,
            projected_files: None,
        });
    }

    ProviderResponse::Done(ActionResult::DirEntries(DirListing {
        entries,
        exhaustive: false,
    }))
}

fn list_domain() -> ProviderResponse {
    let mut entries: Vec<DirEntry> = RecordType::all()
        .iter()
        .map(|rt| DirEntry {
            name: rt.as_str().to_string(),
            kind: EntryKind::File,
            size: Some(4096),
            projected_files: None,
        })
        .collect();

    entries.push(DirEntry {
        name: "_all".to_string(),
        kind: EntryKind::File,
        size: Some(4096),
        projected_files: None,
    });
    entries.push(DirEntry {
        name: "_raw".to_string(),
        kind: EntryKind::File,
        size: Some(4096),
        projected_files: None,
    });

    ProviderResponse::Done(ActionResult::DirEntries(DirListing {
        entries,
        exhaustive: true,
    }))
}

pub fn read_file(id: u64, path: &str) -> ProviderResponse {
    let Some(fs_path) = FsPath::parse(path) else {
        return err("invalid path");
    };

    match fs_path {
        FsPath::Resolvers => resolvers_content(),
        FsPath::Record { resolver, domain, rtype } => {
            with_query(id, resolver, domain, |ctx, effect_fn| {
                let effect = effect_fn(ctx.resolver.as_deref(), &ctx.domain, rtype);
                dispatch(id, Continuation::Single { ctx, rtype }, effect)
            })
        }
        FsPath::All { resolver, domain } => {
            let types = RecordType::common();
            with_query(id, resolver, domain, |ctx, batch_fn| {
                let effects: Vec<SingleEffect> = types
                    .iter()
                    .map(|&rt| batch_fn(ctx.resolver.as_deref(), &ctx.domain, rt))
                    .collect();
                dispatch_batch(
                    id,
                    Continuation::All { ctx, results: Vec::new(), pending_types: types.to_vec() },
                    effects,
                )
            })
        }
        FsPath::Raw { resolver, domain } => {
            with_query(id, resolver, domain, |ctx, effect_fn| {
                let effect = effect_fn(ctx.resolver.as_deref(), &ctx.domain, RecordType::A);
                dispatch(id, Continuation::Raw { ctx }, effect)
            })
        }
        FsPath::ReverseIp { ip } => {
            let effect = match with_state(|s| doh::reverse_query(&s.resolvers, None, ip)) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            dispatch(
                id,
                Continuation::Single {
                    ctx: QueryContext { resolver: None, domain: ip.to_string() },
                    rtype: RecordType::PTR,
                },
                effect,
            )
        }
        _ => err("not a file"),
    }
}

/// Build a `QueryContext` and call `f` with it and a query function bound to the resolver config.
fn with_query(
    _id: u64,
    resolver: Option<&str>,
    domain: &str,
    f: impl FnOnce(QueryContext, &dyn Fn(Option<&str>, &str, RecordType) -> SingleEffect) -> ProviderResponse,
) -> ProviderResponse {
    let query_fn = match with_state(|s| {
        s.resolvers.clone()
    }) {
        Ok(cfg) => cfg,
        Err(e) => return err(&e),
    };

    let ctx = QueryContext {
        resolver: resolver.map(String::from),
        domain: domain.to_string(),
    };

    f(ctx, &|r, d, rt| doh::query(&query_fn, r, d, rt))
}
