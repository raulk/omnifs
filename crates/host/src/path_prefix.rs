pub(crate) fn path_prefix_matches(prefix: &str, path: &str) -> bool {
    if prefix.is_empty() || prefix == "/" {
        return true;
    }

    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::path_prefix_matches;

    #[test]
    fn root_and_empty_prefix_match_everything() {
        assert!(path_prefix_matches("", ""));
        assert!(path_prefix_matches("", "owner/repo"));
        assert!(path_prefix_matches("/", "/"));
        assert!(path_prefix_matches("/", "/owner/repo"));
    }

    #[test]
    fn segment_boundary_matching_is_exact() {
        assert!(path_prefix_matches("owner/repo", "owner/repo"));
        assert!(path_prefix_matches("owner/repo", "owner/repo/issues"));
        assert!(!path_prefix_matches("owner/repo", "owner/repobaz"));

        assert!(path_prefix_matches("/owner/repo", "/owner/repo"));
        assert!(path_prefix_matches("/owner/repo", "/owner/repo/issues/7"));
        assert!(!path_prefix_matches("/owner/repo", "/owner/repobaz"));
    }
}
