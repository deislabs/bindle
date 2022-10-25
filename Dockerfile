FROM rust:1.64 as builder

WORKDIR /app
COPY . /app
RUN cargo build --release --all-features --bin bindle-server

FROM debian:bullseye-slim

ARG USERNAME=bindle
ARG USER_UID=1000
ARG USER_GID=$USER_UID

VOLUME [ "/bindle-data" ]

ENV BINDLE_IP_ADDRESS_PORT="0.0.0.0:8080"
ENV BINDLE_DIRECTORY="/bindle-data/bindles"

RUN groupadd --gid $USER_GID $USERNAME \
    && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME

COPY --from=builder --chown=$USERNAME /app/target/release/bindle-server /usr/local/bin/bindle-server

USER $USERNAME
CMD ["/usr/local/bin/bindle-server", "--unauthenticated", "--keyring", "/bindle-data/keyring.toml"]
