SERVER_FEATURES = --features cli
SERVER_BIN := bindle-server
CLIENT_FEATURES ?= --features=cli
CLIENT_BIN := bindle
BINDLE_LOG_LEVEL ?= debug
BINDLE_ID ?= enterprise.com/warpcore/1.0.0
BINDLE_IFACE ?= 127.0.0.1:8080
MIME ?= "application/toml"
CERT_NAME ?= ssl-example
TLS_OPTS ?= --tls-cert $(CERT_NAME).crt.pem --tls-key $(CERT_NAME).key.pem
AUTH_MODE ?=
# Example of HTTP basic auth with the testing fixture data. 
#AUTH_MODE ?= --htpasswd-file test/data/htpasswd
EMBEDDED_FLAG ?= --use-embedded-db true

export RUST_LOG=error,warp=info,bindle=$(BINDLE_LOG_LEVEL)

.PHONY: test
test: build
test: test-fmt
test: test-e2e
test: test-docs

.PHONY: test-fmt
test-fmt:
	cargo fmt --all -- --check

# Not called by `make test` because `test-e2e` does all the things already.
.PHONY: test-unit
test-unit:
	cargo test --lib

.PHONY: test-docs
test-docs:
	cargo test --doc --all

.PHONY: test-e2e
test-e2e:
	cargo test --tests

.PHONY: serve-tls
serve-tls: $(CERT_NAME).crt.pem
serve-tls: EMBEDDED_FLAG =
serve-tls: _run

.PHONY: serve
serve: TLS_OPTS =
serve: EMBEDDED_FLAG =
serve: BINDLE_DIRECTORY = $(HOME)/.bindle/bindles
serve: _run

.PHONY: serve-embedded
serve-embedded: TLS_OPTS =
serve-embedded: BINDLE_DIRECTORY = $(HOME)/.bindle/bindles-embedded
serve-embedded: _run

.PHONY: serve-embedded-tls
serve-embedded-tls: $(CERT_NAME).crt.pem
serve-embedded-tls: BINDLE_DIRECTORY = $(HOME)/.bindle/bindles-embedded
serve-embedded-tls: _run

.PHONY: _run
_run:
	cargo run $(SERVER_FEATURES) --bin $(SERVER_BIN) -- --directory $(BINDLE_DIRECTORY) --address $(BINDLE_IFACE) $(TLS_OPTS) $(EMBEDDED_FLAG) $(AUTH_MODE)

# Sort of a wacky hack if you want to do `$(make client) --help`
.PHONY: client
client:
	@echo cargo run $(CLIENT_FEATURES) --bin $(CLIENT_BIN) -- 

.PHONY: build
build: build-server
build: build-client

.PHONY: build-server
build-server:
	cargo build $(SERVER_FEATURES) --bin $(SERVER_BIN)

.PHONY: build-client
build-client:
	cargo build $(CLIENT_FEATURES) --bin $(CLIENT_BIN)

$(CERT_NAME).crt.pem:
	openssl req -newkey rsa:2048 -nodes -keyout $(CERT_NAME).key.pem -x509 -days 365 -out $(CERT_NAME).crt.pem
