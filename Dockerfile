FROM docker.io/rust:alpine AS builder

RUN apk add python3 musl-dev pkgconfig openssl-dev make git

WORKDIR /opt/synapse-compressor/
COPY . .

ENV RUSTFLAGS="-C target-feature=-crt-static"

# arm64 builds consume a lot of memory if `CARGO_NET_GIT_FETCH_WITH_CLI` is not
# set to true, so we expose it as a build-arg.
ARG CARGO_NET_GIT_FETCH_WITH_CLI=false
ENV CARGO_NET_GIT_FETCH_WITH_CLI=$CARGO_NET_GIT_FETCH_WITH_CLI
ARG BUILD_PROFILE=dev

RUN cargo build --profile=$BUILD_PROFILE

WORKDIR /opt/synapse-compressor/synapse_auto_compressor/

RUN cargo build

FROM docker.io/alpine

RUN apk add --no-cache libgcc

COPY --from=builder /opt/synapse-compressor/target/*/synapse_compress_state /usr/local/bin/synapse_compress_state
COPY --from=builder /opt/synapse-compressor/target/*/synapse_auto_compressor /usr/local/bin/synapse_auto_compressor
