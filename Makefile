# arrow — dev + local CI helpers for the parser/CLI (root) and egui app (gui/).
# Two independent Cargo workspaces; see CONTRIBUTING.md and .github/workflows/ci.yml.

CARGO ?= cargo
ifeq ($(shell command -v $(CARGO) 2>/dev/null),)
  CARGO := $(HOME)/.cargo/bin/cargo
endif

GUI_DIR := gui
GUI_MANIFEST := $(GUI_DIR)/Cargo.toml
CLI_BIN := target/release/arrow

.DEFAULT_GOAL := help

.PHONY: help build build-gui build-all run run-release test test-cli test-gui \
        fmt fmt-check clippy lint ci verify deps-linux clean

help: ## Show available targets
	@awk 'BEGIN {FS = ":.*##"; printf "Usage: make <target>\n\nTargets:\n"} \
		/^[a-zA-Z0-9_-]+:.*##/ { printf "  %-14s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

build: ## Build the parser/CLI binary (target/release/arrow)
	$(CARGO) build --release

build-gui: ## Build the egui desktop app
	$(CARGO) build --release --manifest-path $(GUI_MANIFEST)

build-all: build build-gui ## Build both crates

run: ## Run the egui app (dev build, fast iteration)
	$(CARGO) run --manifest-path $(GUI_MANIFEST)

run-release: ## Run the egui app (release build)
	$(CARGO) run --release --manifest-path $(GUI_MANIFEST)

test: test-cli test-gui ## Run tests in both crates

test-cli: ## Run parser/CLI unit tests
	$(CARGO) test --release

test-gui: ## Run egui app unit tests
	$(CARGO) test --release --manifest-path $(GUI_MANIFEST)

fmt: ## Format both crates
	$(CARGO) fmt
	cd $(GUI_DIR) && $(CARGO) fmt

fmt-check: ## Check formatting in both crates (fails on diff)
	$(CARGO) fmt --all --check
	cd $(GUI_DIR) && $(CARGO) fmt --all --check

clippy: ## Clippy both crates (warnings denied, mirrors CI)
	$(CARGO) clippy --all-targets --all-features -- -D warnings
	$(CARGO) clippy --all-targets --manifest-path $(GUI_MANIFEST) -- -D warnings

lint: fmt-check clippy ## fmt-check + clippy

ci: lint test ## Full local CI (lint + test)

verify: build ## Smoke-test parser against real ~/.claude data (requires jq)
	@command -v jq >/dev/null 2>&1 || { echo "error: jq is required for 'make verify'"; exit 1; }
	@echo "=== arrow --list (first 40 lines) ==="
	@$(CLI_BIN) --list | { head -40; cat >/dev/null; }
	@echo ""
	@echo "=== invariant: 0 internal ~/.claude/ paths in --json ==="
	@count=$$($(CLI_BIN) --json | jq --arg h "$$HOME/.claude/" \
		'[.repos[].sessions[].files[].path | select(startswith($$h))] | length'); \
	if [ "$$count" -ne 0 ]; then \
		echo "FAIL: found $$count internal ~/.claude/ path(s)"; exit 1; \
	else \
		echo "PASS: 0 internal paths"; \
	fi

deps-linux: ## Install egui/eframe build deps on Debian/Ubuntu (run once)
	sudo apt-get update
	sudo apt-get install -y \
		libgtk-3-dev \
		libxcb-render0-dev \
		libxcb-shape0-dev \
		libxcb-xfixes0-dev \
		libxkbcommon-dev \
		libssl-dev

clean: ## Remove build artifacts from both workspaces
	$(CARGO) clean
	cd $(GUI_DIR) && $(CARGO) clean
