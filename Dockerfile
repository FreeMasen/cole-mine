FROM ubuntu:22.04

RUN apt-get update && apt-get install -y \
  build-essential \
  git \
  curl \
  libdbus-1-dev \
  pkg-config

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  > ru.sh \
  && chmod +x ./ru.sh \
  && ./ru.sh -y
