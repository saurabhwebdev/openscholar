# syntax=docker/dockerfile:1
# OpenScholar — multi-stage: rust builder + debian slim runtime.
# Askama templates + sqlx migrations are EMBEDDED at compile time;
# only ./static is served from disk at runtime (ServeDir).

FROM rust:1.99-bookworm AS builder
WORKDIR /app

# Cache dependencies first (rebuilt only when manifests change).
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
  && echo 'fn main() {}' > src/main.rs \
  && touch src/lib.rs \
  && cargo build --release \
  && rm -rf src target/release/deps/opencodetest* target/release/opencodetest*

# Real sources: templates/ + migrations/ must be present at compile time (embedded).
COPY src ./src
COPY templates ./templates
COPY migrations ./migrations
# Drop the dummy local-crate artifacts (COPY preserves host mtimes, which
# would look older than the dummy fingerprint) but keep compiled deps.
RUN cargo clean -p opencodetest && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && useradd -m -u 10001 app
WORKDIR /app
COPY --from=builder /app/target/release/opencodetest ./opencodetest
COPY static ./static
RUN chown -R app:app /app
USER app

ENV BIND_ADDR=0.0.0.0:8081
ENV PORT=8081
EXPOSE 8081
CMD ["./opencodetest"]
