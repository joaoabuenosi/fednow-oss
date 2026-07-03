# Build from the repository root:
#   docker build -f gateway/Dockerfile -t fednow-gateway .
FROM rust:1-slim AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p fednow-gateway

FROM debian:bookworm-slim
COPY --from=build /src/target/release/fednow-gateway /usr/local/bin/fednow-gateway
ENV FEDNOW_GW_ADDR=0.0.0.0:8090
EXPOSE 8090
ENTRYPOINT ["fednow-gateway"]
