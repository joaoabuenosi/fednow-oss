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
FROM rust:1-slim-bookworm@sha256:452176c0cefca88c0b3184ce85a4eb03e3d4fa05d2afb5366abcba853221019e AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p fednow-gateway

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587
COPY --from=build /src/target/release/fednow-gateway /usr/local/bin/fednow-gateway
ENV FEDNOW_GW_ADDR=0.0.0.0:8090
EXPOSE 8090
ENTRYPOINT ["fednow-gateway"]
