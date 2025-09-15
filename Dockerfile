FROM lukemathwalker/cargo-chef:latest-rust-1.89.0-alpine AS chef
WORKDIR /app

RUN apk add --no-cache \
    pkgconfig \
    openssl-dev \
    openssl-libs-static \
    musl-dev \
    build-base \
    cmake \
    git

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cacher
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM chef AS builder
COPY . .
COPY --from=cacher /app/target target
RUN cargo build --release --locked

FROM alpine:3.22.1 AS runtime
WORKDIR /app

RUN apk add --no-cache \
    ca-certificates \
    ffmpeg

COPY --from=builder /app/target/release/tidalcordrs /usr/local/bin/tidalcordrs

VOLUME ["/app/data"]

ENTRYPOINT ["/usr/local/bin/tidalcordrs"]