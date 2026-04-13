# omnifs

*the universe, mounted on your filesystem.*

Plan 9 was right, just 40 years early. Everything is a file: that's the Unix philosophy at its purest. omnifs takes that idea seriously and projects the entire world (starting with GitHub) into a filesystem you can `cd`, `ls`, `cat`, `grep`, and `tail`.

No SDKs. No pagination. No auth dances. Just files.

## What it looks like

```bash
$ cd ~/omnifs/github
$ ls
raulk/  torvalds/  rust-lang/

$ cd raulk/omnifs
$ ls
issues/  pulls/  actions/  src/

$ cat issues/1/title.txt
Add Linear provider

$ grep -r "bug" issues/
issues/3/labels.txt:bug
issues/7/labels.txt:bug,priority:high

$ tail -f actions/runs/latest/steps/2/log
Running cargo test...
test runtime::tests::test_effect_loop ... ok
test config::tests::test_parse_provider ... ok
```

That's it. You browse repos like directories. Issues are folders with `title.txt`, `body.md`, `labels.txt`. PRs expose `diff.patch`. CI logs stream like any other file. Use the tools you already know.

## For agents

Agents should not have to deal with APIs. If you can read a file, you can read the world. No SDK to install, no authentication flow to implement, no pagination to manage. Just open a path and read. Write files, commit, push to sync back. The filesystem is the universal API.

## How it works

omnifs runs as a FUSE filesystem on Linux (macOS and Windows planned). The architecture has three layers:

**Wasm providers** are plugins compiled to WebAssembly components. Each provider projects a domain (GitHub, Linear, S3, whatever) into the filesystem namespace. Drop a `.wasm` into `~/.omnifs/plugins/` and it mounts.

**Effect-based runtime** means providers never touch the network or Git directly. They describe what they need ("fetch this API endpoint", "clone this repo"), and the host executes. This keeps providers sandboxed and lets the host manage caching, rate limits, and concurrency.

**Git-backed reconciliation** means writes work through Git. Edit files in a transaction directory, then rename it to `commit/` to execute. The provider translates that into API calls. Everything stays auditable, revertible, and familiar.

## Getting started

Build from source:

```bash
git clone https://github.com/raulk/omnifs
cd omnifs
cargo build --release
```

Build the GitHub provider (requires the `wasm32-wasip1` target):

```bash
rustup target add wasm32-wasip1
just build-providers
mkdir -p ~/.omnifs/plugins
cp target/wasm32-wasip1/release/omnifs_provider_github.wasm ~/.omnifs/plugins/
```

Configure the provider at `~/.omnifs/providers/github.toml`:

```toml
plugin = "github_provider.wasm"
mount = "github"
root_mount = true

[auth]
type = "bearer-token"
token_env = "GITHUB_TOKEN"

[capabilities]
domains = ["api.github.com"]
git_repos = ["git@github.com:*"]
max_memory_mb = 256
```

Mount it:

```bash
export GITHUB_TOKEN="$(gh auth token)"
mkdir -p ~/omnifs
./target/release/omnifs mount --mount-point ~/omnifs
```

Or use the container workflow:

```bash
just start   # build image and start container with omnifs mounted at /github
just shell   # interactive shell inside
just logs    # view runtime log
just stop    # tear down
```

## Status

Pre-release v0.1.0. This is a proof of concept that focuses on the container workflow on Linux.

What works:
- GitHub read-only projection (repos, issues, PRs, actions, releases)
- Blobless partial clone for repository contents
- Wasm provider sandboxing with effect-based runtime
- LRU caching for API responses

## What's coming

### Core omnifs
- Write-back via git push (mutations through staging transactions)
- Better caching (hot-path memoization, negative caching, smarter invalidation)
- Background indexing for large trees and expensive projections
- Search across projected content, metadata, and repo history
- Tracing and observability for provider calls, cache behavior, and FUSE latency
- Better prefetching and pagination strategies for large orgs and repos
- Persistent inode stability across remounts
- Offline-friendly local snapshots and replayable sync
- Mutation workflows beyond read-only browsing
- macOS and Windows support

### Provider roadmap

| Provider | What it could project |
| --- | --- |
| GitHub | Commits, branches, reviews, checks, releases, and discussion state |
| Hugging Face | Models, datasets, spaces, cards, files, versions, and download metadata as browsable trees |
| arXiv | Papers by category, author, and query, with abstracts, source, PDFs, references, and update feeds |
| Linear | Teams, projects, issues, cycles, comments, labels, and workflow state with draftable mutations |
| DNS | Zones, records, history, propagation state, and provider-backed change transactions |
| S3 and object stores | Buckets, prefixes, object metadata, versions, lifecycle rules, and event streams |
| OCI registries | Images, tags, manifests, layers, SBOMs, and signature material as mountable content |
| Kubernetes | Clusters, namespaces, workloads, logs, events, and live resource views |
| Postgres and SQLite | Schemas, tables, rows, views, and queryable virtual files for inspection and export |
| Slack and Discord | Channels, threads, message history, attachments, and searchable conversation snapshots |

## License

MIT OR Apache-2.0
