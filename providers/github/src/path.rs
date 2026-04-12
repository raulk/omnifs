//! Parsed path type for the GitHub provider.
//!
//! This module provides type-safe path parsing for the virtual filesystem,
//! replacing raw string splitting and index-based access.

/// Parsed filesystem path representing the GitHub virtual directory structure.
#[derive(Debug, Clone, PartialEq)]
pub enum FsPath<'a> {
    /// Root level (empty path) - lists owners
    Root,
    /// Owner level - lists repos
    Owner { owner: &'a str },
    /// Repo level - lists namespace dirs: _repo, _issues, _prs, _actions
    Repo { owner: &'a str, repo: &'a str },
    /// Namespace level - _issues, _prs, _actions, or _repo
    Namespace {
        owner: &'a str,
        repo: &'a str,
        ns: Namespace,
    },
    /// Resource filter level - _open or _all under _issues/_prs
    ResourceFilter {
        owner: &'a str,
        repo: &'a str,
        kind: ResourceKind,
        filter: StateFilter,
    },
    /// Resource level - specific issue/PR number
    Resource {
        owner: &'a str,
        repo: &'a str,
        kind: ResourceKind,
        filter: StateFilter,
        number: &'a str,
    },
    /// Resource file level - title, body, state, user, or diff under a resource
    ResourceFile {
        owner: &'a str,
        repo: &'a str,
        kind: ResourceKind,
        filter: StateFilter,
        number: &'a str,
        file: ResourceFile,
    },
    /// Comments directory under an issue/PR
    Comments {
        owner: &'a str,
        repo: &'a str,
        kind: ResourceKind,
        filter: StateFilter,
        number: &'a str,
    },
    /// Individual comment file (1-indexed)
    CommentFile {
        owner: &'a str,
        repo: &'a str,
        kind: ResourceKind,
        filter: StateFilter,
        number: &'a str,
        idx: &'a str,
    },
    /// Action runs directory
    ActionRuns { owner: &'a str, repo: &'a str },
    /// Individual action run
    ActionRun {
        owner: &'a str,
        repo: &'a str,
        run_id: &'a str,
    },
    /// Action run file - status, conclusion, or log
    ActionRunFile {
        owner: &'a str,
        repo: &'a str,
        run_id: &'a str,
        file: RunFile,
    },
    /// Repository tree path (git file browsing)
    RepoTree {
        owner: &'a str,
        repo: &'a str,
        tree_path: &'a str,
    },
}

/// Namespace directories within a repository.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Namespace {
    /// _issues - issue tracking
    Issues,
    /// _prs - pull requests
    Prs,
    /// _actions - GitHub Actions
    Actions,
    /// _repo - git repository contents
    Repo,
}

/// Resource kind (issues or PRs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResourceKind {
    /// Issues namespace
    Issues,
    /// Pull requests namespace
    Prs,
}

/// State filter for resources.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StateFilter {
    /// Open items only
    Open,
    /// All items regardless of state
    All,
}

/// File types available under an issue or PR.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResourceFile {
    /// Title field
    Title,
    /// Body field
    Body,
    /// State field
    State,
    /// User/login field
    User,
    /// Diff (PRs only)
    Diff,
}

/// File types available under an action run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RunFile {
    /// Status field
    Status,
    /// Conclusion field
    Conclusion,
    /// Log file
    Log,
}

impl Namespace {
    /// Parse a namespace directory name.
    fn from_dir_name(name: &str) -> Option<Self> {
        match name {
            "_issues" => Some(Namespace::Issues),
            "_prs" => Some(Namespace::Prs),
            "_actions" => Some(Namespace::Actions),
            "_repo" => Some(Namespace::Repo),
            _ => None,
        }
    }
}

impl ResourceKind {
    /// Parse from namespace directory name.
    fn from_ns(ns: Namespace) -> Option<Self> {
        match ns {
            Namespace::Issues => Some(ResourceKind::Issues),
            Namespace::Prs => Some(ResourceKind::Prs),
            _ => None,
        }
    }

    /// Returns the API path segment for this resource kind.
    pub fn api_path(&self) -> &'static str {
        match self {
            ResourceKind::Issues => "issues",
            ResourceKind::Prs => "pulls",
        }
    }

    /// Returns the search qualifier for this resource kind.
    pub fn search_qualifier(&self) -> &'static str {
        match self {
            ResourceKind::Issues => "issue",
            ResourceKind::Prs => "pr",
        }
    }

    /// Returns the directory name for this resource kind.
    #[allow(dead_code)]
    pub fn dir_name(&self) -> &'static str {
        match self {
            ResourceKind::Issues => "_issues",
            ResourceKind::Prs => "_prs",
        }
    }
}

impl StateFilter {
    /// Parse a state filter directory name.
    fn from_dir_name(name: &str) -> Option<Self> {
        match name {
            "_open" => Some(StateFilter::Open),
            "_all" => Some(StateFilter::All),
            _ => None,
        }
    }
}

impl ResourceFile {
    /// Parse a resource file name.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "title" => Some(ResourceFile::Title),
            "body" => Some(ResourceFile::Body),
            "state" => Some(ResourceFile::State),
            "user" => Some(ResourceFile::User),
            "diff" => Some(ResourceFile::Diff),
            _ => None,
        }
    }

    /// Check if this file type is valid for the given resource kind.
    fn is_valid_for(&self, kind: ResourceKind) -> bool {
        match self {
            // Diff is only valid for PRs
            ResourceFile::Diff => kind == ResourceKind::Prs,
            // Title, body, state, user are valid for both
            _ => true,
        }
    }
}

impl RunFile {
    /// Parse a run file name.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "status" => Some(RunFile::Status),
            "conclusion" => Some(RunFile::Conclusion),
            "log" => Some(RunFile::Log),
            _ => None,
        }
    }
}

/// Validates that a path segment is a safe GitHub owner, repo, or numeric ID.
///
/// A segment is safe if it is:
/// - Non-empty
/// - Contains only ASCII alphanumeric characters plus `-`, `_`, and `.`
/// - Does not start with `.`
pub fn is_safe_segment(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    // Must not start with '.'
    if s.starts_with('.') {
        return false;
    }

    // All characters must be ASCII alphanumeric or - _ .
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// Validates that a string contains only ASCII digits.
fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Validates a repository tree path.
///
/// This rejects:
/// - Absolute paths (leading `/`)
/// - Path traversal components (`..`)
/// - Current directory components (`.`)
/// - Empty components (double `//`)
/// - NUL bytes
///
/// Everything else is allowed since repo paths can contain characters
/// that GitHub owner/repo names cannot (`+`, `?`, `#`, spaces, etc.).
pub fn is_safe_tree_path(s: &str) -> bool {
    // Reject leading slash (absolute path)
    if s.starts_with('/') {
        return false;
    }

    // Reject NUL bytes
    if s.bytes().any(|b| b == 0) {
        return false;
    }

    // Split and validate each component
    for component in s.split('/') {
        // Reject empty components (double slash)
        if component.is_empty() {
            return false;
        }
        // Reject path traversal and current directory
        if component == ".." || component == "." {
            return false;
        }
    }

    true
}

impl<'a> FsPath<'a> {
    /// Parse a filesystem path into a typed `FsPath`.
    ///
    /// Returns `None` if the path is invalid or contains unsafe segments.
    pub fn parse(path: &'a str) -> Option<FsPath<'a>> {
        // Empty path -> Root
        if path.is_empty() {
            return Some(FsPath::Root);
        }

        let parts: Vec<&'a str> = path.split('/').collect();

        match parts.len() {
            // Single segment: owner
            1 => {
                let owner = parts[0];
                if !is_safe_segment(owner) {
                    return None;
                }
                Some(FsPath::Owner { owner })
            }

            // Two segments: owner/repo
            2 => {
                let owner = parts[0];
                let repo = parts[1];
                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }
                Some(FsPath::Repo { owner, repo })
            }

            // Three segments: owner/repo/namespace
            3 => {
                let owner = parts[0];
                let repo = parts[1];
                let ns = Namespace::from_dir_name(parts[2])?;
                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }
                Some(FsPath::Namespace { owner, repo, ns })
            }

            // Four segments: could be state filter, runs, or repo tree
            4 => {
                let owner = parts[0];
                let repo = parts[1];
                let ns = parts[2];
                let fourth = parts[3];

                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }

                // Check for _issues/_prs with _open/_all
                if let Some(kind) = Namespace::from_dir_name(ns).and_then(ResourceKind::from_ns) {
                    let filter = StateFilter::from_dir_name(fourth)?;
                    return Some(FsPath::ResourceFilter {
                        owner,
                        repo,
                        kind,
                        filter,
                    });
                }

                // Check for _actions/runs
                if ns == "_actions" && fourth == "runs" {
                    return Some(FsPath::ActionRuns { owner, repo });
                }

                // Check for _repo/<path> (repo tree)
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

            // Five segments: resource number, run, or deep repo tree
            5 => {
                let owner = parts[0];
                let repo = parts[1];
                let ns = parts[2];
                let fourth = parts[3];
                let fifth = parts[4];

                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }

                // Check for _issues/_prs/_open|_all/<number>
                if let Some(kind) = Namespace::from_dir_name(ns).and_then(ResourceKind::from_ns) {
                    let filter = StateFilter::from_dir_name(fourth)?;
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

                // Check for _actions/runs/<run_id>
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

                // Check for _repo/<path> (deep repo tree)
                if ns == "_repo" {
                    let tree_path = &path[(path.find("/_repo/").unwrap() + 7)..];
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

            // Six segments: resource file, comments dir, run file, or deep repo tree
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

                // Check for _issues/_prs/_open|_all/<number>/<file>
                if let Some(kind) = Namespace::from_dir_name(ns).and_then(ResourceKind::from_ns) {
                    let filter = StateFilter::from_dir_name(fourth)?;
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

                    let file = ResourceFile::from_name(sixth)?;
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

                // Check for _actions/runs/<run_id>/<file>
                if ns == "_actions" && fourth == "runs" {
                    if !is_numeric(fifth) {
                        return None;
                    }
                    let file = RunFile::from_name(sixth)?;
                    return Some(FsPath::ActionRunFile {
                        owner,
                        repo,
                        run_id: fifth,
                        file,
                    });
                }

                // Check for _repo/<path> (deep repo tree)
                if ns == "_repo" {
                    let tree_path = &path[(path.find("/_repo/").unwrap() + 7)..];
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

            // Seven segments: comment file or deep repo tree
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

                // Check for _issues/_prs/_open|_all/<number>/comments/<idx>
                if let Some(kind) = Namespace::from_dir_name(ns).and_then(ResourceKind::from_ns) {
                    let filter = StateFilter::from_dir_name(fourth)?;
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

                // Check for _repo/<path> (deep repo tree)
                let ns_check = parts[2];
                if ns_check == "_repo" {
                    let tree_path = &path[(path.find("/_repo/").unwrap() + 7)..];
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

            // Eight or more segments: only valid as repo tree paths
            _ => {
                let owner = parts[0];
                let repo = parts[1];
                let ns = parts[2];

                if !is_safe_segment(owner) || !is_safe_segment(repo) {
                    return None;
                }

                // Only _repo can have deep paths
                if ns != "_repo" {
                    return None;
                }

                let tree_path = &path[(path.find("/_repo/").unwrap() + 7)..];
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

    /// Returns the owner component if present.
    pub fn owner(&self) -> Option<&'a str> {
        match self {
            FsPath::Root => None,
            FsPath::Owner { owner } => Some(owner),
            FsPath::Repo { owner, .. } => Some(owner),
            FsPath::Namespace { owner, .. } => Some(owner),
            FsPath::ResourceFilter { owner, .. } => Some(owner),
            FsPath::Resource { owner, .. } => Some(owner),
            FsPath::ResourceFile { owner, .. } => Some(owner),
            FsPath::Comments { owner, .. } => Some(owner),
            FsPath::CommentFile { owner, .. } => Some(owner),
            FsPath::ActionRuns { owner, .. } => Some(owner),
            FsPath::ActionRun { owner, .. } => Some(owner),
            FsPath::ActionRunFile { owner, .. } => Some(owner),
            FsPath::RepoTree { owner, .. } => Some(owner),
        }
    }

    /// Returns the repo component if present.
    pub fn repo(&self) -> Option<&'a str> {
        match self {
            FsPath::Root | FsPath::Owner { .. } => None,
            FsPath::Repo { repo, .. } => Some(repo),
            FsPath::Namespace { repo, .. } => Some(repo),
            FsPath::ResourceFilter { repo, .. } => Some(repo),
            FsPath::Resource { repo, .. } => Some(repo),
            FsPath::ResourceFile { repo, .. } => Some(repo),
            FsPath::Comments { repo, .. } => Some(repo),
            FsPath::CommentFile { repo, .. } => Some(repo),
            FsPath::ActionRuns { repo, .. } => Some(repo),
            FsPath::ActionRun { repo, .. } => Some(repo),
            FsPath::ActionRunFile { repo, .. } => Some(repo),
            FsPath::RepoTree { repo, .. } => Some(repo),
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
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123/state"),
            Some(FsPath::ResourceFile {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Issues,
                filter: StateFilter::Open,
                number: "123",
                file: ResourceFile::State,
            })
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123/user"),
            Some(FsPath::ResourceFile {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Issues,
                filter: StateFilter::Open,
                number: "123",
                file: ResourceFile::User,
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
        // Diff is only valid for PRs, not issues
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
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_prs/_all/456/comments"),
            Some(FsPath::Comments {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Prs,
                filter: StateFilter::All,
                number: "456",
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
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_prs/_all/456/comments/10"),
            Some(FsPath::CommentFile {
                owner: "mariozechner",
                repo: "omnifs",
                kind: ResourceKind::Prs,
                filter: StateFilter::All,
                number: "456",
                idx: "10",
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
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_actions/runs/12345/conclusion"),
            Some(FsPath::ActionRunFile {
                owner: "mariozechner",
                repo: "omnifs",
                run_id: "12345",
                file: RunFile::Conclusion,
            })
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_actions/runs/12345/log"),
            Some(FsPath::ActionRunFile {
                owner: "mariozechner",
                repo: "omnifs",
                run_id: "12345",
                file: RunFile::Log,
            })
        );
    }

    #[test]
    fn test_parse_repo_tree() {
        // Single file in repo root
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_repo/README.md"),
            Some(FsPath::RepoTree {
                owner: "mariozechner",
                repo: "omnifs",
                tree_path: "README.md",
            })
        );

        // File in subdirectory
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_repo/src/main.rs"),
            Some(FsPath::RepoTree {
                owner: "mariozechner",
                repo: "omnifs",
                tree_path: "src/main.rs",
            })
        );

        // Deep path
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_repo/a/b/c/d/e/file.txt"),
            Some(FsPath::RepoTree {
                owner: "mariozechner",
                repo: "omnifs",
                tree_path: "a/b/c/d/e/file.txt",
            })
        );
    }

    #[test]
    fn test_parse_invalid_segment() {
        // Special characters not allowed in owner
        assert_eq!(FsPath::parse("owner/name?query"), None);
        assert_eq!(FsPath::parse("owner/name#fragment"), None);
        assert_eq!(FsPath::parse("owner/plus+plus"), None);
        assert_eq!(FsPath::parse("owner/slash/name"), None);
        assert_eq!(FsPath::parse("owner/../name"), None);
        assert_eq!(FsPath::parse("./name"), None);

        // Leading dot not allowed
        assert_eq!(FsPath::parse(".hidden/repo"), None);
    }

    #[test]
    fn test_parse_non_numeric_number() {
        // Issue/PR numbers must be numeric
        assert_eq!(FsPath::parse("mariozechner/omnifs/_issues/_open/abc"), None);
        assert_eq!(FsPath::parse("mariozechner/omnifs/_issues/_open/12a"), None);
        assert_eq!(FsPath::parse("mariozechner/omnifs/_issues/_open/"), None);

        // Run IDs must be numeric
        assert_eq!(FsPath::parse("mariozechner/omnifs/_actions/runs/abc"), None);

        // Comment indices must be numeric
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123/comments/abc"),
            None
        );
    }

    #[test]
    fn test_parse_path_traversal() {
        // Path traversal in tree path not allowed
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_repo/../etc/passwd"),
            None
        );
        assert_eq!(FsPath::parse("mariozechner/omnifs/_repo/a/../b"), None);
        assert_eq!(FsPath::parse("mariozechner/omnifs/_repo/a/b/../../c"), None);
    }

    #[test]
    fn test_parse_absolute_tree_path() {
        // Absolute paths not allowed in tree
        assert_eq!(FsPath::parse("mariozechner/omnifs/_repo//etc/passwd"), None);
    }

    #[test]
    fn test_parse_extra_segments() {
        // Extra trailing segments not allowed
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123/title/extra"),
            None
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_prs/_open/123/comments/1/extra"),
            None
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_actions/runs/12345/status/extra"),
            None
        );
    }

    #[test]
    fn test_parse_nul_in_tree_path() {
        // NUL bytes not allowed
        assert_eq!(FsPath::parse("mariozechner/omnifs/_repo/file\0.txt"), None);
    }

    #[test]
    fn test_is_safe_segment() {
        // Valid segments
        assert!(is_safe_segment("mariozechner"));
        assert!(is_safe_segment("omnifs"));
        assert!(is_safe_segment("repo-name"));
        assert!(is_safe_segment("repo_name"));
        assert!(is_safe_segment("repo.name"));
        assert!(is_safe_segment("123"));
        assert!(is_safe_segment("a-b_c.1"));

        // Invalid segments
        assert!(!is_safe_segment("")); // Empty
        assert!(!is_safe_segment(".hidden")); // Leading dot
        assert!(!is_safe_segment("..")); // Double dot
        assert!(!is_safe_segment(".")); // Single dot
        assert!(!is_safe_segment("a/b")); // Contains slash
        assert!(!is_safe_segment("a?b")); // Contains query
        assert!(!is_safe_segment("a#b")); // Contains fragment
        assert!(!is_safe_segment("a+b")); // Contains plus
        assert!(!is_safe_segment("test space")); // Contains space
    }

    #[test]
    fn test_is_safe_tree_path() {
        // Valid paths
        assert!(is_safe_tree_path("README.md"));
        assert!(is_safe_tree_path("src/main.rs"));
        assert!(is_safe_tree_path("a/b/c/d/e/file.txt"));
        assert!(is_safe_tree_path("file with spaces.txt"));
        assert!(is_safe_tree_path("file+plus.txt"));
        assert!(is_safe_tree_path("file#hash.txt"));
        assert!(is_safe_tree_path("file?query.txt"));

        // Invalid paths
        assert!(!is_safe_tree_path("/absolute/path")); // Leading slash
        assert!(!is_safe_tree_path("path/../traversal")); // Contains ..
        assert!(!is_safe_tree_path("path/./here")); // Contains .
        assert!(!is_safe_tree_path("path//double")); // Empty component
        assert!(!is_safe_tree_path("path\0with\0nul")); // NUL bytes
    }

    #[test]
    fn test_resource_kind_helpers() {
        assert_eq!(ResourceKind::Issues.api_path(), "issues");
        assert_eq!(ResourceKind::Prs.api_path(), "pulls");

        assert_eq!(ResourceKind::Issues.search_qualifier(), "issue");
        assert_eq!(ResourceKind::Prs.search_qualifier(), "pr");

        assert_eq!(ResourceKind::Issues.dir_name(), "_issues");
        assert_eq!(ResourceKind::Prs.dir_name(), "_prs");
    }

    #[test]
    fn test_title_body_state_user_only_under_issues_prs() {
        // These files are valid under issues/PRs
        assert!(FsPath::parse("mariozechner/omnifs/_issues/_open/123/title").is_some());
        assert!(FsPath::parse("mariozechner/omnifs/_issues/_open/123/body").is_some());
        assert!(FsPath::parse("mariozechner/omnifs/_prs/_open/123/state").is_some());
        assert!(FsPath::parse("mariozechner/omnifs/_prs/_open/123/user").is_some());

        // These are NOT valid under _actions/runs
        // (extra segments would be None anyway, but the spec mentions this constraint)
        // The parser doesn't allow invalid files under _actions, but test explicitly
        assert!(FsPath::parse("mariozechner/omnifs/_actions/runs/123/title").is_none());
        assert!(FsPath::parse("mariozechner/omnifs/_actions/runs/123/body").is_none());
    }

    #[test]
    fn test_unknown_namespace() {
        assert_eq!(FsPath::parse("mariozechner/omnifs/_unknown"), None);
        assert_eq!(FsPath::parse("mariozechner/omnifs/_issues/_unknown"), None);
    }

    #[test]
    fn test_unknown_file() {
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_issues/_open/123/unknown"),
            None
        );
        assert_eq!(
            FsPath::parse("mariozechner/omnifs/_actions/runs/12345/unknown"),
            None
        );
    }
}
