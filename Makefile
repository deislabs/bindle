SERVER_FEATURES ?= "--all-features"
SERVER_BIN := "bindle-server"
CLIENT_FEATURES ?= "--features=cli"
CLIENT_BIN := "bindle"
BINDLE_LOG_LEVEL ?= "debug"

.PHONY: test
test:
	cargo test

.PHONY: serve
serve:
	RUST_LOG=debug,bindle::*=${BINDLE_LOG_LEVEL}\ 
	cargo run ${SERVER_FEATURES} --bin ${SERVER_BIN}

# Sort of a wacky hack if you want to do `$(make client) --help`
.PHONY: client
client:
	@echo cargo run ${CLIENT_FEATURES} --bin ${CLIENT_BIN} -- 

.PHONY: build
build: build-server
build: build-client

.PHONY: build-server
build-server:
	cargo build ${SERVER_FEATURES} --bin ${SERVER_BIN}

.PHONY: build-client
build-client:
	cargo build ${CLIENT_FEATURES} --bin ${CLIENT_BIN}
	