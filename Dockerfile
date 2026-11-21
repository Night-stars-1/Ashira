# Ashira server image: the core (`ashira --listen`) with the web client built in.
# Release CI builds `ashira` and `web/dist` first and copies them in (see release.yml);
# the image adds the tools the AI's bash commands expect.
#
# Ubuntu 24.04: the core is built on ubuntu-latest and needs its glibc (2.39).
FROM ubuntu:24.04

ARG VERSION=dev
LABEL org.opencontainers.image.title="Ashira" \
      org.opencontainers.image.description="Ashira server with its web client" \
      org.opencontainers.image.source="https://github.com/Night-stars-1/Ashira" \
      org.opencontainers.image.version="${VERSION}"

# bash runs every command the AI issues; git, curl, ripgrep and the rest are what those
# commands reach for. tini reaps the processes they leave behind.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      bash ca-certificates curl git less nano openssh-client procps ripgrep tini tzdata unzip wget xz-utils \
 && rm -rf /var/lib/apt/lists/*

# A home of its own: data (config, sessions, memory) under /data, projects under /workspace.
# The base image's `ubuntu` user holds uid 1000; it goes first so `ashira` can take it.
RUN if id ubuntu >/dev/null 2>&1; then userdel --remove ubuntu; fi \
 && useradd --create-home --uid 1000 --user-group --shell /bin/bash ashira \
 && mkdir -p /data /workspace \
 && chown ashira:ashira /data /workspace

COPY --chmod=755 ashira /usr/local/bin/ashira
COPY web /opt/ashira/web

ENV ASHIRA_HOME=/data \
    ASHIRA_CONTAINER=1 \
    ASHIRA_SHELL=/bin/bash \
    LANG=C.UTF-8 \
    TZ=Asia/Shanghai

USER ashira
WORKDIR /workspace
VOLUME ["/data", "/workspace"]
EXPOSE 8787

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD curl -fsS -o /dev/null http://127.0.0.1:8787/ || exit 1

ENTRYPOINT ["/usr/bin/tini", "--", "ashira"]
CMD ["--listen", "0.0.0.0:8787", "--web", "/opt/ashira/web"]
