FROM node:26-slim AS frontend
WORKDIR /build/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY frontend/ ./
RUN npm run build

# Dependencies are built by cargo-chef in a layer of their own, keyed only on
# Cargo.toml and Cargo.lock, so builds that change only the server's code
# reuse it. It's an ordinary layer (not a cache mount) so CI's layer cache
# keeps it.
FROM rust:1.100-slim-bookworm AS chef
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install cargo-chef --locked --version 0.1.78
WORKDIR /build/server

FROM chef AS planner
COPY server/ ./
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS server
COPY --from=planner /build/server/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --locked --recipe-path recipe.json
COPY server/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=server /build/server/target/release/tributary /usr/local/bin/tributary
COPY --from=frontend /build/frontend/dist /app/static

ENV TRIBUTARY_SERVER__STATIC_DIR=/app/static \
    TRIBUTARY_SERVER__DATABASE=/data/tributary.sqlite
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["tributary", "--config", "/config/tributary.toml"]
