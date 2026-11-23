# Build from the repository root:
#   docker build -f simulator/Dockerfile -t fednow-sim .
#
# Both stages are pinned by digest, and both are Debian 12 "bookworm".
#
# The digest is the pin; the tag next to it is a human-readable label for which
# image that digest is, and nothing resolves it at build time. Dependabot keeps
# both current (.github/dependabot.yml, docker ecosystem).
#
# The builder is `rust:1-slim-bookworm`, not `rust:1-slim`. The floating
# `1-slim` tag now resolves to a Debian 13 "trixie" image, and this Dockerfile's
# runtime stage is bookworm: a binary linked against trixie's glibc 2.41 cannot
# start on bookworm's 2.36. Pinning `1-slim`'s current digest would have frozen
# that skew in place rather than fixed it. Keep the two stages on the same
# Debian release whenever either is bumped.
FROM rust:1-slim-bookworm@sha256:7c7d4eb4d05b7781187a327809466cfddf29753f6defdc3d19a0e291951b7a37 AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p fednow-sim

FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc
COPY --from=build /src/target/release/fednow-sim /usr/local/bin/fednow-sim
ENV FEDNOW_SIM_ADDR=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["fednow-sim"]
