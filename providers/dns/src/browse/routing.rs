use super::{dir_entry, dispatch, dispatch_batch, err, file_entry, mk_dir, mk_file};
use super::{resolver_dir_names, resolvers_content};
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
        FsPath::ReverseIp { .. } => ProviderResponse::Done(ActionResult::DirEntries(DirListing {
            entries: vec![mk_file("PTR")],
            exhaustive: true,
        })),
        _ => err("not a directory"),
    }
}

fn list_root() -> ProviderResponse {
    let mut entries = vec![
        mk_file("_resolvers"),
        mk_dir("_reverse"),
    ];
    for name in resolver_dir_names() {
        entries.push(mk_dir(name));
    }
    ProviderResponse::Done(ActionResult::DirEntries(DirListing {
        entries,
        exhaustive: false,
    }))
}

fn list_domain() -> ProviderResponse {
    let mut entries: Vec<DirEntry> = RecordType::all()
        .iter()
        .map(|rt| mk_file(rt.as_str()))
        .collect();
    entries.push(mk_file("_all"));
    entries.push(mk_file("_raw"));
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
            let ctx = mk_ctx(resolver, domain);
            let effect = match with_state(|s| doh::query(&s.resolvers, resolver, domain, rtype)) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            dispatch(id, Continuation::Single { ctx, rtype }, effect)
        }
        FsPath::All { resolver, domain } => {
            let types = RecordType::common();
            let ctx = mk_ctx(resolver, domain);
            let effects = match with_state(|s| {
                types.iter().map(|&rt| doh::query(&s.resolvers, resolver, domain, rt)).collect()
            }) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            dispatch_batch(
                id,
                Continuation::All { ctx, results: Vec::new(), pending_types: types.to_vec() },
                effects,
            )
        }
        FsPath::Raw { resolver, domain } => {
            let ctx = mk_ctx(resolver, domain);
            let effect = match with_state(|s| doh::query(&s.resolvers, resolver, domain, RecordType::A)) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            dispatch(id, Continuation::Raw { ctx }, effect)
        }
        FsPath::ReverseIp { ip } => {
            let effect = match with_state(|s| doh::reverse_query(&s.resolvers, None, ip)) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            let ctx = QueryContext { resolver: None, domain: ip.to_string() };
            dispatch(id, Continuation::Single { ctx, rtype: RecordType::PTR }, effect)
        }
        _ => err("not a file"),
    }
}

fn mk_ctx(resolver: Option<&str>, domain: &str) -> QueryContext {
    QueryContext {
        resolver: resolver.map(String::from),
        domain: domain.to_string(),
    }
}
