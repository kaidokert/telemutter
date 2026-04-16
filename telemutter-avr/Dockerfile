FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:${PATH}

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    build-essential \
    gcc-avr \
    binutils-avr \
    avr-libc \
    && rm -rf /var/lib/apt/lists/*

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable

# Nightly + rust-src for no_std / build-std flows.
RUN rustup toolchain install nightly-2025-11-01 --profile minimal --component rust-src

WORKDIR /work

# Build command examples:
#   docker build -t telemutter-avr -f telemutter-avr/Dockerfile .
#   docker run --rm -it -v ${PWD}:/work telemutter-avr bash
#   cd telemutter-avr && cargo +nightly-2025-11-01 check --lib
