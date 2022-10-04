FROM docker.io/rust:alpine AS builder

RUN apk add python3 musl-dev pkgconfig openssl-dev make

ENV RUSTFLAGS="-C target-feature=-crt-static"

WORKDIR /opt/synapse-compressor/

COPY . .

RUN cargo build

WORKDIR /opt/synapse-compressor/synapse_auto_compressor/

RUN cargo build

FROM docker.io/alpine

RUN apk add --no-cache libgcc

COPY --from=builder /opt/synapse-compressor/target/debug/synapse_compress_state /usr/local/bin/synapse_compress_state
COPY --from=builder /opt/synapse-compressor/target/debug/synapse_auto_compressor /usr/local/bin/synapse_auto_compressor
