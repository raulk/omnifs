//! Domain types for the GitHub provider's virtual filesystem structure.

use core::str::FromStr;

/// Namespace directories within a repository.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Namespace {
    Issues,
    Prs,
    Actions,
    Repo,
}

/// Resource kind (issues or PRs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Issues,
    Prs,
}

/// State filter for resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFilter {
    Open,
    All,
}

/// File types available under an issue or PR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceFile {
    Title,
    Body,
    State,
    User,
    Diff,
}

/// File types available under an action run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunFile {
    Status,
    Conclusion,
    Log,
}

impl Namespace {
    pub fn from_dir_name(name: &str) -> Option<Self> {
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
    pub fn from_ns(ns: Namespace) -> Option<Self> {
        match ns {
            Namespace::Issues => Some(ResourceKind::Issues),
            Namespace::Prs => Some(ResourceKind::Prs),
            _ => None,
        }
    }

    pub fn api_path(&self) -> &'static str {
        match self {
            ResourceKind::Issues => "issues",
            ResourceKind::Prs => "pulls",
        }
    }

    pub fn search_qualifier(&self) -> &'static str {
        match self {
            ResourceKind::Issues => "issue",
            ResourceKind::Prs => "pr",
        }
    }

    #[allow(dead_code)]
    pub fn dir_name(&self) -> &'static str {
        match self {
            ResourceKind::Issues => "_issues",
            ResourceKind::Prs => "_prs",
        }
    }
}

impl FromStr for ResourceKind {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "_issues" => Ok(Self::Issues),
            "_prs" => Ok(Self::Prs),
            _ => Err(()),
        }
    }
}

impl FromStr for StateFilter {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "_open" => Ok(Self::Open),
            "_all" => Ok(Self::All),
            _ => Err(()),
        }
    }
}

impl FromStr for ResourceFile {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "title" => Ok(Self::Title),
            "body" => Ok(Self::Body),
            "state" => Ok(Self::State),
            "user" => Ok(Self::User),
            "diff" => Ok(Self::Diff),
            _ => Err(()),
        }
    }
}

impl ResourceFile {
    pub fn is_valid_for(&self, kind: ResourceKind) -> bool {
        match self {
            ResourceFile::Diff => kind == ResourceKind::Prs,
            _ => true,
        }
    }
}

impl FromStr for RunFile {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "status" => Ok(Self::Status),
            "conclusion" => Ok(Self::Conclusion),
            "log" => Ok(Self::Log),
            _ => Err(()),
        }
    }
}

/// Validates that a path segment is a safe GitHub owner, repo, or numeric ID.
pub fn is_safe_segment(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if s.starts_with('.') {
        return false;
    }
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// Validates that a string contains only ASCII digits.
pub fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Validates a repository tree path.
pub fn is_safe_tree_path(s: &str) -> bool {
    if s.starts_with('/') {
        return false;
    }
    if s.bytes().any(|b| b == 0) {
        return false;
    }
    for component in s.split('/') {
        if component.is_empty() {
            return false;
        }
        if component == ".." || component == "." {
            return false;
        }
    }
    true
}
