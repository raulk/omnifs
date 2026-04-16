use super::{dir_entry, dispatch, dispatch_batch, err, file_entry, mk_dir, mk_file};
use super::{resolver_dir_names, resolvers_content};
use crate::doh;
use crate::omnifs::provider::types::*;
use crate::path::{FsPath, RecordType};
use crate::{Continuation, with_state};

pub fn lookup_child(_id: u64, parent_path: &str, name: &str) -> ProviderResponse {
    // TODO: extract path manipulation to helpers.
    let full_path = match parent_path {
        "" => name.to_string(),
        p => format!("{p}/{name}"),
    };

    let Some(fs_path) = FsPath::parse(&full_path) else {
        return ProviderResponse::Done(ActionResult::DirEntryOption(None));
    };

    match fs_path {
        FsPath::Root => dir_entry(name),
        FsPath::Resolvers => file_entry("_resolvers"),
        FsPath::ReverseRoot => dir_entry("_reverse"),
        FsPath::ReverseIp { .. } | FsPath::DirectReverseIp { .. } => file_entry(name),
        FsPath::Resolver { .. } | FsPath::Domain { .. } => dir_entry(name),
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
        _ => err("not a directory"),
    }
}

fn list_root() -> ProviderResponse {
    let mut entries = vec![mk_file("_resolvers"), mk_dir("_reverse")];
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
        .map(|rt| mk_file(rt.as_ref()))
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
        FsPath::Record {
            resolver,
            domain,
            rtype,
        } => {
            let effect = match with_state(|s| doh::query(&s.resolvers, resolver, domain, rtype)) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            dispatch(id, Continuation::Single, effect)
        }
        FsPath::All { resolver, domain } => {
            let types = RecordType::common();
            let effects = match with_state(|s| {
                types
                    .iter()
                    .map(|&rt| doh::query(&s.resolvers, resolver, domain, rt))
                    .collect()
            }) {
                Ok(e) => e,
                Err(e) => return err(&e),
            };
            dispatch_batch(
                id,
                Continuation::All {
                    results: Vec::new(),
                },
                effects,
            )
        }
        FsPath::Raw { resolver, domain } => {
            let effect =
                match with_state(|s| doh::query(&s.resolvers, resolver, domain, RecordType::A)) {
                    Ok(e) => e,
                    Err(e) => return err(&e),
                };
            dispatch(
                id,
                Continuation::Raw {
                    domain: domain.to_string(),
                },
                effect,
            )
        }
        FsPath::ReverseIp { ip } => read_reverse_ip(id, None, ip),
        FsPath::DirectReverseIp { resolver, ip } => read_reverse_ip(id, resolver, ip),
        _ => err("not a file"),
    }
}

fn read_reverse_ip(id: u64, resolver: Option<&str>, ip: &str) -> ProviderResponse {
    let effect = match with_state(|s| doh::reverse_query(&s.resolvers, resolver, ip))
        .and_then(|result| result)
    {
        Ok(effect) => effect,
        Err(e) => return err(&e),
    };
    dispatch(id, Continuation::Single, effect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_bare_ip_returns_file_entry() {
        let response = lookup_child(1, "", "172.217.171.46");
        let ProviderResponse::Done(ActionResult::DirEntryOption(Some(entry))) = response else {
            panic!("expected file entry");
        };

        assert_eq!(entry.name, "172.217.171.46");
        assert_eq!(entry.kind, EntryKind::File);
    }

    #[test]
    fn list_bare_ip_returns_not_a_directory() {
        let response = list_children(1, "172.217.171.46");
        let ProviderResponse::Done(ActionResult::Err(message)) = response else {
            panic!("expected error");
        };

        assert_eq!(message, "not a directory");
    }
}
