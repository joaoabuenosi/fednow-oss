# Build from the repository root:
#   docker build -f simulator/Dockerfile -t fednow-sim .
FROM rust:1-slim AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p fednow-sim

FROM debian:bookworm-slim
COPY --from=build /src/target/release/fednow-sim /usr/local/bin/fednow-sim
ENV FEDNOW_SIM_ADDR=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["fednow-sim"]
