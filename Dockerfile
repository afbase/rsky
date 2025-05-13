# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY . .

# Install production dependencies and build a release artifact.
RUN rustup toolchain install nightly-2025-01-03-aarch64-unknown-linux-gnu && \
    cargo build --package rsky-pds

# Run the web service on container startup.
CMD ["sh", "-c", "ROCKET_PORT=$PORT ROCKET_ADDRESS=0.0.0.0 cargo run --package rsky-pds"]