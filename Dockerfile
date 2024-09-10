FROM lukemathwalker/cargo-chef:latest-rust-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin beam-sel

FROM gcr.io/distroless/cc-debian12 AS runtime
COPY --from=builder /app/target/release/beam-sel /usr/local/bin/
STOPSIGNAL SIGINT
ENTRYPOINT ["/usr/local/bin/beam-sel"]
