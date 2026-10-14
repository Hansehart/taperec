# Cargo, clippy and rustfmt for ./test.sh, matching rust-toolchain.toml.
ARG RUST_IMAGE=rust:1.99.0-bookworm
FROM ${RUST_IMAGE}
RUN rustup component add clippy rustfmt
# A toolchain mismatch fails fast.
ENV RUSTUP_AUTO_INSTALL=0
