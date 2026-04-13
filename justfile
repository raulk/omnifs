image := "omnifs-dev"
container := "omnifs-shell"

check: build-providers
    cargo fmt --all --check
    cargo clippy -- -D warnings
    just check-providers
    cargo test -p omnifs-host

check-providers:
    cd providers && cargo check --workspace --target wasm32-wasip1
    cd providers && cargo clippy --workspace --target wasm32-wasip1 -- -D warnings
    cd providers && cargo test --workspace --target wasm32-wasip1 --no-run

build-providers:
    #!/usr/bin/env bash
    set -euo pipefail
    for manifest in providers/*/Cargo.toml; do
        grep -q '^\[package\]' "$manifest" || continue
        cargo component build --manifest-path "$manifest" --release --target-dir target
    done

test: build-providers
    cargo test --workspace

test-integration: build-providers
    cargo test -p omnifs-host --test runtime_test

build:
    docker build -t {{image}} .

start: build
    #!/usr/bin/env bash
    set -euo pipefail
    export GITHUB_TOKEN="${GITHUB_TOKEN:-$(gh auth token)}"
    : "${SSH_AUTH_SOCK:?SSH_AUTH_SOCK must be set on the host}"
    docker rm -f {{container}} >/dev/null 2>&1 || true
    docker run -d \
      --name {{container}} \
      --device /dev/fuse \
      --cap-add SYS_ADMIN \
      --security-opt apparmor:unconfined \
      -e GITHUB_TOKEN="$GITHUB_TOKEN" \
      -e SSH_AUTH_SOCK=/ssh-agent \
      -e GIT_SSH_COMMAND='ssh -F /dev/null -o StrictHostKeyChecking=accept-new' \
      -v "$SSH_AUTH_SOCK:/ssh-agent" \
      -v "$(pwd)/scripts/demo.sh:/work/demo.sh:ro" \
      {{image}} \
      bash -lc 'RUST_LOG=info exec omnifs mount --mount-point /github --config-dir /root/.omnifs --cache-dir /tmp/omnifs-cache >/tmp/omnifs.log 2>&1'
    for _ in $(seq 1 60); do
      if docker exec {{container}} sh -lc "grep -qs ' /github ' /proc/mounts"; then
        exit 0
      fi
      if ! docker ps --format '{{"{{.Names}}"}}' | grep -qx {{container}}; then
        docker exec {{container}} sh -lc 'cat /tmp/omnifs.log' >&2 || true
        exit 1
      fi
      sleep 1
    done
    docker exec {{container}} sh -lc 'cat /tmp/omnifs.log' >&2 || true
    exit 1

shell:
    docker exec -it {{container}} /bin/zsh

logs:
    docker exec -it {{container}} sh -lc 'cat /tmp/omnifs.log'

stop:
    docker rm -f {{container}}
