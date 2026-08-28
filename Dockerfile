# syntax=docker/dockerfile:1.7

FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132

ARG TARGETARCH
ARG VERSION=dev
ARG REVISION=unknown

LABEL org.opencontainers.image.title="servo-fetch" \
      org.opencontainers.image.description="Fetch, render, and extract web content via the Servo engine" \
      org.opencontainers.image.source="https://github.com/konippi/servo-fetch" \
      org.opencontainers.image.url="https://github.com/konippi/servo-fetch" \
      org.opencontainers.image.documentation="https://github.com/konippi/servo-fetch#readme" \
      org.opencontainers.image.authors="konippi" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0" \
      org.opencontainers.image.base.name="debian:trixie-slim" \
      org.opencontainers.image.base.digest="sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132"

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
      libegl1 libegl-mesa0 libfontconfig1 libfreetype6 libharfbuzz0b \
      libglib2.0-0t64 libssl3t64 \
      fonts-dejavu-core fonts-noto-core fonts-liberation2 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 1001 servo \
    && useradd --uid 1001 --gid servo \
       --shell /usr/sbin/nologin --home-dir /home/servo --create-home servo

COPY --chown=servo:servo --chmod=0755 \
     dist/${TARGETARCH}/servo-fetch /usr/local/bin/servo-fetch

RUN mkdir -p -m 0700 /tmp/runtime-servo && chown servo:servo /tmp/runtime-servo

USER servo
WORKDIR /home/servo

EXPOSE 3000
ENV XDG_CACHE_HOME=/tmp
ENV XDG_RUNTIME_DIR=/tmp/runtime-servo

ENTRYPOINT ["servo-fetch"]
CMD ["serve", "--host", "0.0.0.0", "--port", "3000"]
