FROM rust:1.97.1-slim-trixie AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        cmake \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .sqlx ./.sqlx
COPY crates ./crates
RUN SQLX_OFFLINE=true cargo build --release --locked -p strife-api -p strife-worker

FROM debian:trixie-slim AS api

ARG STRIFE_REVISION=unknown
LABEL org.opencontainers.image.title="Strife API" \
      org.opencontainers.image.revision="${STRIFE_REVISION}" \
      org.opencontainers.image.source="https://github.com/s4njee/strife"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl file \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 strife \
    && useradd --uid 10001 --gid 10001 --no-create-home --home-dir /nonexistent strife

COPY --from=builder /src/target/release/strife-api /usr/local/bin/strife-api

USER 10001:10001
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/strife-api"]

FROM debian:trixie-slim AS worker

ARG STRIFE_REVISION=unknown
LABEL org.opencontainers.image.title="Strife Worker" \
      org.opencontainers.image.revision="${STRIFE_REVISION}" \
      org.opencontainers.image.source="https://github.com/s4njee/strife"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        file \
        imagemagick \
        libimage-exiftool-perl \
        libraw-bin \
        libreoffice-writer \
        unzip \
        zip \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 strife \
    && useradd --uid 10001 --gid 10001 --no-create-home --home-dir /tmp/strife-home strife

COPY --from=builder /src/target/release/strife-worker /usr/local/bin/strife-worker

ENV HOME=/tmp/strife-home
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/strife-worker"]
