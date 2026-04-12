use omnifs_host::runtime::capability::{CapabilityChecker, CapabilityGrants};
use omnifs_host::runtime::cloner::GitCloner;
use omnifs_host::runtime::executor::{EffectResponse, ErrorKind};
use omnifs_host::runtime::git::GitExecutor;
use std::path::PathBuf;
use std::sync::Arc;

fn create_test_repo(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    let run = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "-b", "main"]);
    std::fs::write(dir.join("README.md"), "Hello\n").unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-m", "init"]);
}

fn git_executor() -> GitExecutor {
    let grants = CapabilityGrants {
        domains: vec![],
        git_repos: vec!["*".to_string()],
        max_memory_mb: 64,
        needs_git: true,
    };
    let capability = Arc::new(CapabilityChecker::new(grants));
    let cloner = Arc::new(GitCloner::new(PathBuf::from("/tmp/omnifs-test-cache")));
    GitExecutor::new(cloner, capability)
}

#[test]
fn test_list_tree_root() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    create_test_repo(&repo_path);

    let executor = git_executor();
    let repo_id = executor.register_local(repo_path);
    let result = executor.list_tree(repo_id, "refs/heads/main", "");
    match result {
        EffectResponse::GitTreeEntries(entries) => {
            let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
            assert!(
                names.contains(&"README.md"),
                "expected README.md in {names:?}"
            );
            assert!(names.contains(&"src"), "expected src in {names:?}");
        }
        other => panic!("expected GitTreeEntries, got {other:?}"),
    }
}

#[test]
fn test_list_tree_subdir() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    create_test_repo(&repo_path);

    let executor = git_executor();
    let repo_id = executor.register_local(repo_path);
    let result = executor.list_tree(repo_id, "refs/heads/main", "src");
    match result {
        EffectResponse::GitTreeEntries(entries) => {
            let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
            assert!(names.contains(&"main.rs"), "expected main.rs in {names:?}");
        }
        other => panic!("expected GitTreeEntries, got {other:?}"),
    }
}

#[test]
fn test_read_blob() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    create_test_repo(&repo_path);

    let executor = git_executor();
    let repo_id = executor.register_local(repo_path);

    let entries = match executor.list_tree(repo_id, "refs/heads/main", "") {
        EffectResponse::GitTreeEntries(e) => e,
        other => panic!("expected GitTreeEntries, got {other:?}"),
    };
    let readme_oid = entries
        .iter()
        .find(|e| e.name == "README.md")
        .expect("README.md not found")
        .oid
        .clone();

    let result = executor.read_blob(repo_id, &readme_oid);
    match result {
        EffectResponse::GitBlobData(data) => {
            let content = String::from_utf8_lossy(&data);
            assert!(
                content.contains("Hello"),
                "expected 'Hello' in blob content"
            );
        }
        other => panic!("expected GitBlobData, got {other:?}"),
    }
}

#[test]
fn test_head_ref() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path().join("repo");
    create_test_repo(&repo_path);

    let executor = git_executor();
    let repo_id = executor.register_local(repo_path);
    let result = executor.head_ref(repo_id);
    match result {
        EffectResponse::GitRef(ref_name) => {
            assert!(
                ref_name.contains("main"),
                "expected 'main' in head ref, got {ref_name}"
            );
        }
        other => panic!("expected GitRef, got {other:?}"),
    }
}

#[test]
fn test_unopened_repo_errors() {
    let executor = git_executor();
    let result = executor.list_tree(999, "refs/heads/main", "");
    match result {
        EffectResponse::Error { kind, .. } => {
            assert_eq!(kind, ErrorKind::NotFound);
        }
        other => panic!("expected Error, got {other:?}"),
    }
}
