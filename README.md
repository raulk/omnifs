# omnifs

*the universe, mounted on your filesystem.*

omnifs projects the entire world into your local filesystem. GitHub repos, Hugging Face models, Kubernetes clusters, Slack channels, arXiv papers: each service mounts as a Wasm plugin you can `cd`, `ls`, `cat`, and `grep`. No SDKs. No pagination. No auth dances. Just paths.

Plan 9 was right, just 40 years early. Everything is a file. The world moved to APIs; omnifs moves it back to paths, for humans and agents alike.

## Quickstart

### Prerequisites

- Docker and Docker Compose
- SSH agent running with a GitHub key loaded
- `gh` CLI (for generating a token)

### Launch

```bash
git clone https://github.com/raulk/omnifs
cd omnifs

mkdir -p .secrets
gh auth token > .secrets/github_token

docker compose up --build -d
docker compose exec omnifs /bin/zsh
```

### Explore

```bash
cd /github/torvalds
ls

cd /github/ollama/ollama
ls

cd /github/ollama/ollama/_repo
ls

cd /github/ollama/_issues/_open
ls
```

Use `docker compose logs omnifs` to inspect logs and `docker compose down` to stop the container.

<details>
<summary>SSH agent troubleshooting</summary>

omnifs clones repos over SSH inside the container using your forwarded agent socket. This does not copy your private key into the container, but it does let the container ask your agent to sign while the socket is mounted.

Verify your setup:

```bash
echo "$SSH_AUTH_SOCK"
ssh-add -L
ssh -T git@github.com
```

</details>

## For agents

Agents should not have to deal with APIs. If you can read a file, you can read the world. No SDK to install, no authentication flow to implement, no pagination to manage. Just open a path and read. Write files, commit, push to sync back. The filesystem is the universal API.

## How it works

omnifs runs as a FUSE filesystem on Linux (macOS and Windows planned). The architecture has three layers:

```
                                                                   ┌────────────────┐
┌──────────────┐          ┌────────────────────────────┐           │ github.wasm    ├──▶ GitHub
│  your shell  │   FUSE   │         omnifs host         │  effects  │ linear.wasm    ├──▶ Linear
│  or agent    │ ◀──────▶ │  /github  /linear  /arxiv   │ ◀──────▶ │ arxiv.wasm     ├──▶ arXiv
└──────────────┘   files  └────────────────────────────┘           │ ...            │
                                                                   └────────────────┘
```

**Wasm providers** are plugins compiled to WebAssembly components. Each provider projects a domain (GitHub, Linear, S3, whatever) into the filesystem namespace. Drop a `.wasm` into `~/.omnifs/plugins/` and it mounts.

**Effect-based runtime** means providers never touch the network or Git directly. They describe what they need ("fetch this API endpoint", "clone this repo"), and the host executes. This keeps providers sandboxed and lets the host manage caching, rate limits, and concurrency.

**Git-backed reconciliation** means writes work through Git. Edit files in a transaction directory, then rename it to `commit/` to execute. The provider translates that into API calls. Everything stays auditable, revertible, and familiar.

## Status

Early release (v0.1.0). Read-only GitHub projection works end-to-end in a Linux container. Write-back and additional providers are next.

- Browse any GitHub repo's tree without cloning it locally
- Read issues, PRs, CI runs, and diffs as plain files
- Extend with new providers by dropping a `.wasm` plugin into `~/.omnifs/plugins/`
- Responses cached with LRU eviction; no redundant API calls

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
