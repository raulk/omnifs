//! Parsed path type for the GitHub provider.
//!
//! This module provides type-safe path parsing for the virtual filesystem.
//! Domain types (`Namespace`, `ResourceKind`, etc.) live in `crate::types`.

use crate::types::*;

/// Parsed filesystem path representing the GitHub virtual directory structure.
#[derive(Debug, Clone, PartialEq)]
pub enum FsPath<'a> {
    Root,
    Owner {
        owner: &'a str,
    },
    Repo {
        owner: &'a str,
        repo: &'a str,
    },
    Namespace {
        owner: &'a str,
        repo: &'a str,
        ns: Namespace,
    },
    ResourceFilter {
        owner: &'a str,
        repo: &'a str,
        kind: ResourceKind,
        filter: StateFilter,
    },
    Resource {
        owner: &'a str,
        repo: &'a str,
        kind: ResourceKind,
        filter: StateFilter,
        number: &'a str,
    },
    ResourceFile {
        owner: &'a str,
        repo: &'a str,
        kind: ResourceKind,
        filter: StateFilter,
        number: &'a str,
        file: ResourceFile,
    },
    Comments {
        owner: &'a str,
        repo: &'a str,
        kind: ResourceKind,
        filter: StateFilter,
        number: &'a str,
    },
    CommentFile {
        owner: &'a str,
        repo: &'a str,
        kind: ResourceKind,
        filter: StateFilter,
        number: &'a str,
        idx: &'a str,
    },
    ActionRuns {
        owner: &'a str,
        repo: &'a str,
    },
    ActionRun {
        owner: &'a str,
        repo: &'a str,
        run_id: &'a str,
    },
    ActionRunFile {
        owner: &'a str,
        repo: &'a str,
        run_id: &'a str,
        file: RunFile,
    },
    RepoTree {
        owner: &'a str,
        repo: &'a str,
        tree_path: &'a str,
    },
}

impl<'a> FsPath<'a> {
    /// Parse a filesystem path into a typed `FsPath`.
    ///
    /// Returns `None` if the path is invalid or contains unsafe segments.
    pub fn parse(path: &'a str) -> Option<FsPath<'a>> {
        fn repo_tree_path<'a>(path: &'a str) -> Option<&'a str> {
            let marker = "/_repo/";
            let pos = path.find(marker)?;
            Some(&path[(pos + marker.len())..])
        }

        if path.is_empty() {
            return Some(FsPath::Root);
        }

        let parts: Vec<&'a str> = path.split('/').collect();

        match parts.len() {
            1 => {
                let owner = parts[0];
                if !is_safe_segment(owner) {
                    return None;
                }
                Some(FsPath::Owner { owner })
            }

            2 => {
                let owner = parts[0];
                let repo = parts[1];
                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }
                Some(FsPath::Repo { owner, repo })
            }

            3 => {
                let owner = parts[0];
                let repo = parts[1];
                let ns = Namespace::from_dir_name(parts[2])?;
                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }
                Some(FsPath::Namespace { owner, repo, ns })
            }

            4 => {
                let owner = parts[0];
                let repo = parts[1];
                let ns = parts[2];
                let fourth = parts[3];

                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }

                if let Some(kind) = Namespace::from_dir_name(ns).and_then(ResourceKind::from_ns) {
                    let filter: StateFilter = fourth.parse().ok()?;
                    return Some(FsPath::ResourceFilter {
                        owner,
                        repo,
                        kind,
                        filter,
                    });
                }

                if ns == "_actions" && fourth == "runs" {
                    return Some(FsPath::ActionRuns { owner, repo });
                }

                if ns == "_repo" {
                    let tree_path = fourth;
                    if !is_safe_tree_path(tree_path) {
                        return None;
                    }
                    return Some(FsPath::RepoTree {
                        owner,
                        repo,
                        tree_path,
                    });
                }

                None
            }

            5 => {
                let owner = parts[0];
                let repo = parts[1];
                let ns = parts[2];
                let fourth = parts[3];
                let fifth = parts[4];

                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }

                if let Some(kind) = Namespace::from_dir_name(ns).and_then(ResourceKind::from_ns) {
                    let filter: StateFilter = fourth.parse().ok()?;
                    if !is_numeric(fifth) {
                        return None;
                    }
                    return Some(FsPath::Resource {
                        owner,
                        repo,
                        kind,
                        filter,
                        number: fifth,
                    });
                }

                if ns == "_actions" && fourth == "runs" {
                    if !is_numeric(fifth) {
                        return None;
                    }
                    return Some(FsPath::ActionRun {
                        owner,
                        repo,
                        run_id: fifth,
                    });
                }

                if ns == "_repo" {
                    let tree_path = repo_tree_path(path)?;
                    if !is_safe_tree_path(tree_path) {
                        return None;
                    }
                    return Some(FsPath::RepoTree {
                        owner,
                        repo,
                        tree_path,
                    });
                }

                None
            }

            6 => {
                let owner = parts[0];
                let repo = parts[1];
                let ns = parts[2];
                let fourth = parts[3];
                let fifth = parts[4];
                let sixth = parts[5];

                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }

                if let Some(kind) = Namespace::from_dir_name(ns).and_then(ResourceKind::from_ns) {
                    let filter: StateFilter = fourth.parse().ok()?;
                    if !is_numeric(fifth) {
                        return None;
                    }

                    if sixth == "comments" {
                        return Some(FsPath::Comments {
                            owner,
                            repo,
                            kind,
                            filter,
                            number: fifth,
                        });
                    }

                    let file: ResourceFile = sixth.parse().ok()?;
                    if !file.is_valid_for(kind) {
                        return None;
                    }
                    return Some(FsPath::ResourceFile {
                        owner,
                        repo,
                        kind,
                        filter,
                        number: fifth,
                        file,
                    });
                }

                if ns == "_actions" && fourth == "runs" {
                    if !is_numeric(fifth) {
                        return None;
                    }
                    let file: RunFile = sixth.parse().ok()?;
                    return Some(FsPath::ActionRunFile {
                        owner,
                        repo,
                        run_id: fifth,
                        file,
                    });
                }

                if ns == "_repo" {
                    let tree_path = repo_tree_path(path)?;
                    if !is_safe_tree_path(tree_path) {
                        return None;
                    }
                    return Some(FsPath::RepoTree {
                        owner,
                        repo,
                        tree_path,
                    });
                }

                None
            }

            7 => {
                let owner = parts[0];
                let repo = parts[1];
                let ns = parts[2];
                let fourth = parts[3];
                let fifth = parts[4];
                let sixth = parts[5];
                let seventh = parts[6];

                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }

                if let Some(kind) = Namespace::from_dir_name(ns).and_then(ResourceKind::from_ns) {
                    let filter: StateFilter = fourth.parse().ok()?;
                    if !is_numeric(fifth) || sixth != "comments" || !is_numeric(seventh) {
                        return None;
                    }
                    return Some(FsPath::CommentFile {
                        owner,
                        repo,
                        kind,
                        filter,
                        number: fifth,
                        idx: seventh,
                    });
                }

                if ns == "_repo" {
                    let tree_path = repo_tree_path(path)?;
                    if !is_safe_tree_path(tree_path) {
                        return None;
                    }
                    return Some(FsPath::RepoTree {
                        owner,
                        repo,
                        tree_path,
                    });
                }

                None
            }

            _ => {
                let owner = parts[0];
                let repo = parts[1];
                let ns = parts[2];

                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }

                if ns != "_repo" {
                    return None;
                }

                let tree_path = repo_tree_path(path)?;
                if !is_safe_tree_path(tree_path) {
                    return None;
                }
                Some(FsPath::RepoTree {
                    owner,
                    repo,
                    tree_path,
                })
            }
        }
    }

    #[allow(dead_code)]
    pub fn owner(&self) -> Option<&'a str> {
        match self {
            FsPath::Root => None,
            FsPath::Owner { owner } => Some(owner),
            FsPath::Repo { owner, .. }
            | FsPath::Namespace { owner, .. }
            | FsPath::ResourceFilter { owner, .. }
            | FsPath::Resource { owner, .. }
            | FsPath::ResourceFile { owner, .. }
            | FsPath::Comments { owner, .. }
            | FsPath::CommentFile { owner, .. }
            | FsPath::ActionRuns { owner, .. }
            | FsPath::ActionRun { owner, .. }
            | FsPath::ActionRunFile { owner, .. }
            | FsPath::RepoTree { owner, .. } => Some(owner),
        }
    }

    #[allow(dead_code)]
    pub fn repo(&self) -> Option<&'a str> {
        match self {
            FsPath::Root | FsPath::Owner { .. } => None,
            FsPath::Repo { repo, .. }
            | FsPath::Namespace { repo, .. }
            | FsPath::ResourceFilter { repo, .. }
            | FsPath::Resource { repo, .. }
            | FsPath::ResourceFile { repo, .. }
            | FsPath::Comments { repo, .. }
            | FsPath::CommentFile { repo, .. }
            | FsPath::ActionRuns { repo, .. }
            | FsPath::ActionRun { repo, .. }
            | FsPath::ActionRunFile { repo, .. }
            | FsPath::RepoTree { repo, .. } => Some(repo),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_root() {
        assert_eq!(FsPath::parse(""), Some(FsPath::Root));
    }

    #[test]
    fn test_parse_owner() {
        assert_eq!(
            FsPath::parse("mariozechner"),
            Some(FsPath::Owner {
                owner: "mariozechner"
            })
        );
    }

    #[test]
    fn test_parse_repo() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs"),
            Some(FsPath::Repo {
                owner: "mariozechner",
                repo: "omnifs"
            })
        );
    }

    #[test]
    fn test_parse_namespace() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues"),
            Some(FsPath::Namespace {
                owner: "mariozechner",
                repo: "omnifs",
                ns: Namespace::Issues
            })
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_prs"),
            Some(FsPath::Namespace {
                owner: "mariozechner",
                repo: "omnifs",
                ns: Namespace::Prs
            })
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_actions"),
            Some(FsPath::Namespace {
                owner: "mariozechner",
                repo: "omnifs",
                ns: Namespace::Actions
            })
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_repo"),
            Some(FsPath::Namespace {
                owner: "mariozechner",
                repo: "omnifs",
                ns: Namespace::Repo
            })
        );
    }

    #[test]
    fn test_parse_resource_filter() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open"),
            Some(FsPath::ResourceFilter {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Issues,
                filter: StateFilter::Open,
            })
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_all"),
            Some(FsPath::ResourceFilter {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Issues,
                filter: StateFilter::All,
            })
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_prs/_open"),
            Some(FsPath::ResourceFilter {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Prs,
                filter: StateFilter::Open,
            })
        );
    }

    #[test]
    fn test_parse_resource() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123"),
            Some(FsPath::Resource {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Issues,
                filter: StateFilter::Open,
                number: "123",
            })
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_prs/_all/456"),
            Some(FsPath::Resource {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Prs,
                filter: StateFilter::All,
                number: "456",
            })
        );
    }

    #[test]
    fn test_parse_resource_file() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123/title"),
            Some(FsPath::ResourceFile {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Issues,
                filter: StateFilter::Open,
                number: "123",
                file: ResourceFile::Title,
            })
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123/body"),
            Some(FsPath::ResourceFile {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Issues,
                filter: StateFilter::Open,
                number: "123",
                file: ResourceFile::Body,
            })
        );
    }

    #[test]
    fn test_parse_diff_under_prs() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_prs/_open/123/diff"),
            Some(FsPath::ResourceFile {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Prs,
                filter: StateFilter::Open,
                number: "123",
                file: ResourceFile::Diff,
            })
        );
    }

    #[test]
    fn test_parse_diff_under_issues() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123/diff"),
            None
        );
    }

    #[test]
    fn test_parse_comments() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123/comments"),
            Some(FsPath::Comments {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Issues,
                filter: StateFilter::Open,
                number: "123",
            })
        );
    }

    #[test]
    fn test_parse_comment_file() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123/comments/1"),
            Some(FsPath::CommentFile {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Issues,
                filter: StateFilter::Open,
                number: "123",
                idx: "1",
            })
        );
    }

    #[test]
    fn test_parse_action_runs() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_actions/runs"),
            Some(FsPath::ActionRuns {
                owner: "mariozechner",
                repo: "omnifs",
            })
        );
    }

    #[test]
    fn test_parse_action_run() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_actions/runs/12345"),
            Some(FsPath::ActionRun {
                owner: "mariozechner",
                repo: "omnifs",
                run_id: "12345",
            })
        );
    }

    #[test]
    fn test_parse_action_run_file() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_actions/runs/12345/status"),
            Some(FsPath::ActionRunFile {
                owner: "mariozechner",
                repo: "omnifs",
                run_id: "12345",
                file: RunFile::Status,
            })
        );
    }

    #[test]
    fn test_parse_repo_tree() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_repo/README.md"),
            Some(FsPath::RepoTree {
                owner: "mariozechner",
                repo: "omnifs",
                tree_path: "README.md",
            })
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_repo/src/main.rs"),
            Some(FsPath::RepoTree {
                owner: "mariozechner",
                repo: "omnifs",
                tree_path: "src/main.rs",
            })
        );
    }

    #[test]
    fn test_parse_invalid_segment() {
        assert_eq!(FsPath::parse("owner/name?query"), None);
        assert_eq!(FsPath::parse(".hidden/repo"), None);
    }

    #[test]
    fn test_parse_non_numeric_number() {
        assert_eq!(FsPath::parse("mariozechner/omnifs/_issues/_open/abc"), None);
        assert_eq!(FsPath::parse("mariozechner/omnifs/_actions/runs/abc"), None);
    }

    #[test]
    fn test_parse_path_traversal() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_repo/../etc/passwd"),
            None
        );
    }

    #[test]
    fn test_is_safe_segment() {
        assert!(is_safe_segment("mariozechner"));
        assert!(is_safe_segment("repo-name"));
        assert!(!is_safe_segment(""));
        assert!(!is_safe_segment(".hidden"));
        assert!(!is_safe_segment("a/b"));
    }

    #[test]
    fn test_is_safe_tree_path() {
        assert!(is_safe_tree_path("README.md"));
        assert!(is_safe_tree_path("src/main.rs"));
        assert!(!is_safe_tree_path("/absolute/path"));
        assert!(!is_safe_tree_path("path/../traversal"));
    }
}
