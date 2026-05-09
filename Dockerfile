# syntax=docker/dockerfile:1.7

ARG DEBIAN_IMAGE=debian:trixie-slim@sha256:109e2c65005bf160609e4ba6acf7783752f8502ad218e298253428690b9eaa4b

FROM ${DEBIAN_IMAGE}

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
      org.opencontainers.image.base.digest="sha256:109e2c65005bf160609e4ba6acf7783752f8502ad218e298253428690b9eaa4b"

RUN apt-get update && apt-get install -y --no-install-recommends \
      curl ca-certificates \
      libegl1 libegl-mesa0 libfontconfig1 libfreetype6 libharfbuzz0b \
      libglib2.0-0t64 libssl3t64 \
      fonts-dejavu-core fonts-noto-core fonts-liberation2 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 1001 servo \
    && useradd --uid 1001 --gid servo \
       --shell /usr/sbin/nologin --home-dir /home/servo --create-home servo

COPY --chown=servo:servo --chmod=0755 \
     dist/${TARGETARCH}/servo-fetch /usr/local/bin/servo-fetch

USER servo
WORKDIR /home/servo

EXPOSE 3000
# fontconfig cache on /tmp for --read-only compatibility
ENV XDG_CACHE_HOME=/tmp

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS --max-time 2 http://127.0.0.1:3000/health || exit 1

ENTRYPOINT ["servo-fetch"]
CMD ["serve", "--host", "0.0.0.0", "--port", "3000"]
