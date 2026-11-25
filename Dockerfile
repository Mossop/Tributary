FROM node:26-slim AS frontend
WORKDIR /build/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:1.96-slim-bookworm AS server
WORKDIR /build/server
COPY server/ ./
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=server /build/server/target/release/dashboard /usr/local/bin/dashboard
COPY --from=frontend /build/frontend/dist /app/static

ENV DASHBOARD_SERVER__STATIC_DIR=/app/static \
    DASHBOARD_SERVER__DATABASE=/data/dashboard.db
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["dashboard", "--config", "/config/config.toml"]
