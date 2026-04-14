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
COPY . .
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
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo component build \
        --manifest-path providers/github/Cargo.toml \
        --release --target-dir /src/target

# --- Build host binary ---

FROM deps AS builder
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p omnifs-cli \
    && cp /src/target/release/omnifs /omnifs

# --- Runtime ---

FROM ubuntu:25.10 AS runtime-base

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

COPY scripts/demo.sh /tmp/demo.sh
RUN chmod 0755 /tmp/demo.sh \
    && mkdir -p /root/.omnifs/plugins /root/.omnifs/providers

RUN cat > /root/.omnifs/providers/github.toml <<'CONF'
plugin = "omnifs_provider_github.wasm"
mount = "github"
root_mount = true

[auth]
type = "bearer-token"
token_env = "GITHUB_TOKEN"
token_file = "/run/secrets/github_token"

[capabilities]
domains = ["api.github.com"]
git_repos = ["git@github.com:*"]
max_memory_mb = 256
CONF

SHELL ["/bin/zsh", "-c"]
ENV SHELL=/bin/zsh
WORKDIR /

FROM runtime-base AS runtime-prebuilt

COPY dist/omnifs /usr/local/bin/omnifs
COPY dist/omnifs_provider_github.wasm /root/.omnifs/plugins/

FROM runtime-base AS runtime

COPY --from=builder /omnifs /usr/local/bin/
COPY --from=providers /src/target/wasm32-wasip1/release/omnifs_provider_github.wasm \
     /root/.omnifs/plugins/
