FROM node:26-slim AS frontend
WORKDIR /build/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:1.96-slim-bookworm AS server
WORKDIR /build/server
COPY server/ ./
# The registry and target directory are kept in BuildKit cache mounts so
# rebuilds only recompile what changed. The cache isn't part of the image, so
# the binary is copied out of it.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/server/target \
    cargo build --release --locked \
    && cp target/release/tributary /build/tributary

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=server /build/tributary /usr/local/bin/tributary
COPY --from=frontend /build/frontend/dist /app/static

ENV TRIBUTARY_SERVER__STATIC_DIR=/app/static \
    TRIBUTARY_SERVER__DATABASE=/data/tributary.sqlite
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["tributary", "--config", "/config/tributary.toml"]
