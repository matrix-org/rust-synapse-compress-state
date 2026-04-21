# This uses the multi-stage build feature of Docker to build the binaries for multiple architectures without QEMU.
# The first stage is responsible for building binaries for all the supported architectures (amd64 and arm64), and the
# second stage only copies the binaries for the target architecture.
# We leverage Zig and cargo-zigbuild for providing a cross-compilation-capable C compiler and linker.

ARG RUSTC_VERSION=1.78.0
ARG ZIG_VERSION=0.14.1
ARG CARGO_ZIGBUILD_VERSION=0.20.1

FROM --platform=${BUILDPLATFORM} docker.io/rust:${RUSTC_VERSION} AS builder

# Install cargo-zigbuild for cross-compilation
ARG CARGO_ZIGBUILD_VERSION
RUN cargo install --locked cargo-zigbuild@=${CARGO_ZIGBUILD_VERSION}

# Download zig compiler for cross-compilation
ARG ZIG_VERSION
RUN curl -L "https://ziglang.org/download/${ZIG_VERSION}/zig-$(uname -m)-linux-${ZIG_VERSION}.tar.xz" | tar -J -x -C /usr/local && \
  ln -s "/usr/local/zig-$(uname -m)-linux-${ZIG_VERSION}/zig" /usr/local/bin/zig

# Install all cross-compilation targets
ARG RUSTC_VERSION
RUN rustup target add  \
    --toolchain "${RUSTC_VERSION}" \
    x86_64-unknown-linux-musl \
    aarch64-unknown-linux-musl

WORKDIR /opt/synapse-compressor/
COPY . .

# Build for all targets
RUN cargo zigbuild \
    --release \
    --workspace \
    --bins \
    --features "openssl/vendored" \
    --target aarch64-unknown-linux-musl \
    --target x86_64-unknown-linux-musl

# Move the binaries in a separate folder per architecture, so we can copy them using the TARGETARCH build arg
RUN mkdir -p /opt/binaries/amd64 /opt/binaries/arm64
RUN mv target/x86_64-unknown-linux-musl/release/synapse_compress_state \
       target/x86_64-unknown-linux-musl/release/synapse_auto_compressor \
       /opt/binaries/amd64
RUN mv target/aarch64-unknown-linux-musl/release/synapse_compress_state \
       target/aarch64-unknown-linux-musl/release/synapse_auto_compressor \
       /opt/binaries/arm64

FROM --platform=${TARGETPLATFORM} docker.io/alpine

ARG TARGETARCH

COPY --from=builder /opt/binaries/${TARGETARCH}/synapse_compress_state /usr/local/bin/synapse_compress_state
COPY --from=builder /opt/binaries/${TARGETARCH}/synapse_auto_compressor /usr/local/bin/synapse_auto_compressor