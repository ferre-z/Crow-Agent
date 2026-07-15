# Crow v0 — local task runner.
#
# Wraps the same cargo invocations CI runs (see .github/workflows/ci.yml)
# so `make ci` on a workstation matches the green build. Toolchain
# pin (1.88) lives in `rust-toolchain.toml` and is enforced by rustup
# on `cd`.
#
# Conventions:
#   - One cargo invocation per target. `all` / `ci` compose.
#   - `--all-targets --all-features` is a no-op for the [features]
#     table today, but we keep them so the target stays correct when
#     features are added.
#   - `cargo install` uses `--locked` so the binary ships against
#     `Cargo.lock`, not whatever crates.io says today.
#
# Usage:
#   make help     — list every target
#   make test     — offline test suite (no API key needed)
#   make ci       — fmt + lint + build + test, identical to GitHub Actions
#   make install  — `cargo install --path . --locked` into ~/.cargo/bin
#   make smoke    — release build + `crow --version && crow doctor`

CARGO      ?= cargo
RUSTFLAGS  ?=
BIN        := crow
PROFILE    ?= debug  # set to "release" for install-release
CARGO_HOME ?= $(HOME)/.cargo
INSTALL_BIN := $(CARGO_HOME)/bin/$(BIN)

# Default goal. Run `make help` for the catalogue.
.DEFAULT_GOAL := help

.PHONY: help all build test lint fmt fmt-check install install-release run smoke ci clean

# `help` self-documents by grepping `# help ` comments from this file.
# Output is plain for CI logs (no colour).
help:  ## list every make target
	@awk 'BEGIN {FS = ":.*##"} \
	  /^[a-zA-Z_-]+:.*##/ { printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2 }' \
	  $(MAKEFILE_LIST) | sort
	@echo ""
	@echo "Common: make test | make ci | make install | make smoke"

all: fmt-check lint build test  ## run the full local gate (CI without the download cache)

build:  ## release build into target/release/crow
	$(CARGO) build --release

test:  ## run all tests offline (no API key, no network)
	$(CARGO) test --all-targets --all-features

lint:  ## clippy with -D warnings
	$(CARGO) clippy --all-targets --all-features -- -D warnings

fmt:  ## apply rustfmt
	$(CARGO) fmt --all

fmt-check:  ## verify rustfmt without changing files
	$(CARGO) fmt --all -- --check

# Default install: DEBUG build. Smaller on disk than release; fits
# on disk-quota boxes where 'cargo install --release' blows the
# quota at link time. After installing, runs 'cargo clean' to drop
# the build artifacts (the binary is already copied to ~/.cargo/bin)
# — leaves only the source clone + the installed binary behind.
install:  ## debug build + copy to $(INSTALL_BIN) + cargo clean
	$(CARGO) build --locked
	install -d $(CARGO_HOME)/bin
	install -m 0755 target/debug/$(BIN) $(INSTALL_BIN)
	$(CARGO) clean
	@echo "$(BIN) installed to $(INSTALL_BIN) (debug build)"

# Opt-in release install. ~600 MiB peak disk; build artifacts kept
# after install for incremental rebuilds. Pass PROFILE=release or
# use the install-release target.
install-release:  ## release build via 'cargo install --path . --locked'
	$(CARGO) install --path . --locked

run:  ## cargo run -- <args>  (pass CLI args after --)
	$(CARGO) run --

smoke: build  ## build then sanity-check --version and doctor
	./target/release/$(BIN) --version
	./target/release/$(BIN) doctor

ci: fmt-check lint build test  ## same gate as GitHub Actions (.github/workflows/ci.yml)

clean:  ## cargo clean (drop target/)
	$(CARGO) clean
