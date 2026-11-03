# Minimal, non-root image for manifex-agent.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release --locked || cargo build --release

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/manifex-agent /manifex-agent
USER nonroot:nonroot
EXPOSE 8443
ENTRYPOINT ["/manifex-agent"]
