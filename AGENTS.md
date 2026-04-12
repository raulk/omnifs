# AGENTS.md

Repository-local guidance for working in `omnifs`.

## Scope

- This repo is currently **Linux-only**.
- Do not reintroduce macOS-specific mount behavior, `diskutil`, or macFUSE assumptions unless explicitly requested.
- The primary supported workflow is the **container workflow** in `justfile`.

## Current workflow

Use these commands:

```bash
just start
just shell
just logs
just stop
```

Behavior:

- `just start` builds the image, starts a named container, and mounts omnifs at `/github` inside the container.
- `just shell` opens an interactive `zsh` shell in the running container.
- `just logs` prints `/tmp/omnifs.log` from the container.
- `just stop` removes the running container.

Do not add alternate local mount recipes unless explicitly requested.

## Auth and cloning

Current auth model:

- GitHub API auth uses `GITHUB_TOKEN`.
- The container receives `GITHUB_TOKEN` from the host via `just start`.
- Git clone currently uses SSH:
  - remote format: `git@github.com:<owner>/<repo>.git`
  - auth comes from forwarded `SSH_AUTH_SOCK`
  - do not mount host private keys into the container

Container startup requires:

- host `gh auth token` works, or `GITHUB_TOKEN` is already set
- host `SSH_AUTH_SOCK` is set
- host SSH agent has a usable GitHub key loaded

Useful checks on the host:

```bash
gh auth token >/dev/null
ssh-add -L
ssh -T git@github.com
```

Useful checks in the container:

```bash
cat /tmp/omnifs.log
ssh -F /dev/null -T git@github.com
```

## Logging and debugging

- Runtime log file is `/tmp/omnifs.log` inside the container.
- Clone failures should surface there with `git clone` stderr.
- FUSE `access(...)` warnings are expected noise unless they correlate with a real failure.

When a repo path returns `Input/output error`, check:

1. `just logs`
2. SSH auth inside the container
3. whether the mount is still present in `/proc/mounts`

## Shell expectations

The runtime image uses Ubuntu 25.10 and `zsh`.

Expected interactive shell behavior:

- `ls` is aliased to `ls --color=auto`
- `ll` is aliased to `ls -lrt`

If changing shell behavior, prefer putting it in the image rather than generating per-session shell config.

## Build and test

Rust validation:

```bash
cargo fmt
cargo test
```

Docker build:

```bash
just build
```

The Dockerfile is intentionally cache-oriented:

- multi-stage build
- `cargo-chef`
- BuildKit cache mounts
- minimal build context via `.dockerignore`

Preserve that structure unless there is a clear regression or simplification with equal caching behavior.

## Codebase expectations

- Keep changes small and local.
- Prefer preserving the current architecture:
  - inode table
  - router
  - providers
  - GitHub cache/scheduler/poller
  - clone manager
- Do not silently change the auth model or transport model.
- If switching clone transport from SSH to HTTPS/token, call that out explicitly because it changes the operational contract.

## Mutation protocol

Mutations are not implemented yet.

If adding them, prefer:

- read model remains read-only
- drafts live under a draft namespace
- execution is triggered by moving a prepared transaction directory into a control namespace

Do not make projected issue/PR files directly writable as an implicit mutation mechanism.
