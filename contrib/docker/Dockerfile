# Rucid CI Docker Image
# Multi-stage build for small image size

# Build stage
FROM rust:1.94-alpine AS builder

# Install build dependencies
RUN apk add --no-cache \
        cmake \
        capnproto \
        pkgconfig \
        openssl-dev \
        musl-dev

# Set working directory
WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY rucid/Cargo.toml ./rucid/Cargo.toml
COPY ruci/Cargo.toml ./ruci/Cargo.toml
COPY ruci-core/Cargo.toml ./ruci-core/Cargo.toml
COPY ruci-protocol/Cargo.toml ./ruci-protocol/Cargo.toml

# Create dummy source files for dependency caching
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    mkdir -p rucid/src && \
    echo "fn main() {}" > rucid/src/main.rs && \
    mkdir -p ruci/src && \
    echo "fn main() {}" > ruci/src/main.rs && \
    mkdir -p ruci-core/src && \
    echo "fn main() {}" > ruci-core/src/main.rs && \
    mkdir -p ruci-protocol/src && \
    echo "fn main() {}" > ruci-protocol/src/main.rs

# Build dependencies first (for caching)
RUN cargo build --release --bin rucid 2>/dev/null || true

# Copy actual source code
COPY . .

# Build release binary
RUN cargo build --release --bin rucid

# Runtime stage
FROM alpine:3.19

# Install runtime dependencies
RUN apk add --no-cache \
        ca-certificates \
        tini \
        sqlite-libs

# Create rucid user
RUN addgroup -g 1000 ruci && \
    adduser -u 1000 -G ruci -h /var/lib/ruci -s /bin/sh -D rucid

# Create necessary directories
RUN mkdir -p /var/lib/ruci /var/log/ruci && \
    chown -R ruci:ruci /var/lib/ruci /var/log/ruci

# Copy binary from builder
COPY --from=builder /build/target/release/rucid /usr/local/bin/rucid

# Copy entrypoint script
COPY contrib/docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# Switch to rucid user
USER rucid

# Expose ports (RPC + Web UI)
EXPOSE 7741 8080

# Volume mounts
VOLUME ["/var/lib/ruci/jobs", "/var/lib/ruci/run", "/var/lib/ruci/archive", "/var/lib/ruci/logs", "/run/ruci"]

ENTRYPOINT ["/entrypoint.sh"]
CMD ["rucid"]
