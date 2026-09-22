# Build from the repository root:
#   docker build -f gateway/Dockerfile -t fednow-gateway .
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
FROM rust:1-slim-bookworm@sha256:ff521445a372125ed4f76e1453a1f8098f2d05332d1601d30db1c1f62757e730 AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p fednow-gateway

FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251
COPY --from=build /src/target/release/fednow-gateway /usr/local/bin/fednow-gateway
ENV FEDNOW_GW_ADDR=0.0.0.0:8090
EXPOSE 8090
ENTRYPOINT ["fednow-gateway"]
