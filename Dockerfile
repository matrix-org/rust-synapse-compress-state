FROM rust:1.43
WORKDIR /usr/src

RUN USER=root cargo new synapse-compress-state
WORKDIR /usr/src/synapse-compress-state
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo install --path .

ENTRYPOINT ["/usr/local/cargo/bin/synapse-compress-state"]
