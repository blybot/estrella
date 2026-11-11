# syntax=docker/dockerfile:1
ARG RUST=1.93.1
ARG NODE=24.22.2

# The web UI is embedded into the binary at compile time (include_dir!).
FROM node:${NODE}-trixie-slim AS frontend
WORKDIR /build/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
# The editor imports example documents from ../src/fixtures.
COPY src/fixtures/ /build/src/fixtures/
RUN npm run build

FROM rust:${RUST}-trixie AS chef
# libheif for HEIC photo uploads (the default `heif` feature).
RUN apt-get update \
    && apt-get install -y --no-install-recommends libheif-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked --version 0.1.78
WORKDIR /build

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
# Optional cap on parallel compile jobs, for memory-constrained build hosts.
ARG CARGO_BUILD_JOBS
COPY --from=planner /build/recipe.json recipe.json
RUN if [ -z "$CARGO_BUILD_JOBS" ]; then unset CARGO_BUILD_JOBS; fi \
    && cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
COPY --from=frontend /build/frontend/dist frontend/dist
RUN if [ -z "$CARGO_BUILD_JOBS" ]; then unset CARGO_BUILD_JOBS; fi \
    && cargo install --locked --path .

FROM debian:trixie-slim
# The HEVC plugin is what lets libheif decode iPhone HEIC photos.
RUN apt-get update \
    && apt-get install -y --no-install-recommends libheif1 libheif-plugin-libde265 tzdata \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/estrella /opt/estrella

ENV ESTRELLA_LISTEN=0.0.0.0:8080

USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/opt/estrella"]
CMD ["serve"]
