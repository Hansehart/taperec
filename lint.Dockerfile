# typos from its release binary, verified by SHA-256.
FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587
ARG TYPOS=typos-v1.51.1-x86_64-unknown-linux-musl.tar.gz
ARG TYPOS_SHA256=93c301fd4120ff076e2cc2a6d6a61cab4a4bdb81899287c0938b452e56d1f454
ADD --checksum=sha256:${TYPOS_SHA256} https://github.com/crate-ci/typos/releases/download/v1.51.1/${TYPOS} /tmp/
RUN tar -xzf /tmp/${TYPOS} -C /usr/local/bin ./typos && rm /tmp/${TYPOS}
ENTRYPOINT ["typos"]
