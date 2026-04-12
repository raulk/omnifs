# syntax=docker.io/docker/dockerfile:1.12-labs

FROM rust:1-bookworm AS toolchain

COPY rust-toolchain.toml .
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        fuse3 libfuse3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked \
    && cargo install cargo-component --locked \
    && rustup target add wasm32-wasip1

# --- Dependency cache (host crates) ---

FROM toolchain AS planner
WORKDIR /src
COPY --exclude=providers . .
RUN cargo chef prepare --recipe-path recipe.json

FROM toolchain AS deps
WORKDIR /src
COPY --from=planner /src/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo chef cook --release --recipe-path recipe.json

# --- Build providers ---

FROM toolchain AS providers
WORKDIR /src
COPY --exclude=crates . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo component build \
        --manifest-path providers/github/Cargo.toml \
        --release --target-dir /src/target

# --- Build host binary ---

FROM deps AS builder
WORKDIR /src
COPY --exclude=providers . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p omnifs-cli \
    && cp /src/target/release/omnifs /omnifs

# --- Runtime ---

FROM ubuntu:25.10

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash ca-certificates curl fuse3 gnupg \
        zsh git openssh-client procps \
        bat git-delta ripgrep util-linux \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /etc/apt/keyrings \
    && curl -fsSL https://repo.charm.sh/apt/gpg.key \
        | gpg --dearmor -o /etc/apt/keyrings/charm.gpg \
    && echo "deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *" \
        > /etc/apt/sources.list.d/charm.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends gum \
    && rm -rf /var/lib/apt/lists/*

RUN printf '%s\n' \
        'alias ls="ls --color=auto"' \
        'alias ll="ls -lrt"' \
        '' \
        'setopt NO_AUTO_CD' \
        'setopt PROMPT_SUBST' \
        'PROMPT="%F{blue}%~%f %# "' \
        'skip_global_compinit=1' \
        >/etc/zsh/zshrc

COPY --from=builder /omnifs /usr/local/bin/

RUN mkdir -p /root/.omnifs/plugins /root/.omnifs/providers
COPY --from=providers /src/target/wasm32-wasip1/release/github_provider.wasm \
     /root/.omnifs/plugins/

RUN cat > /root/.omnifs/providers/github.toml <<'CONF'
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
CONF

SHELL ["/bin/zsh", "-c"]
ENV SHELL=/bin/zsh
WORKDIR /
