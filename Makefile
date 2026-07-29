WEB_DIR := apps/web

.PHONY: check lint build test format install dev-services dev-services-down api web

check: lint build test

lint:
	cargo fmt --check
	SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings
	npm --prefix $(WEB_DIR) run lint
	npm --prefix $(WEB_DIR) run format:check

build:
	SQLX_OFFLINE=true cargo build --workspace
	npm --prefix $(WEB_DIR) run build

test:
	SQLX_OFFLINE=true cargo test --workspace

format:
	cargo fmt
	npm --prefix $(WEB_DIR) run format

install:
	npm --prefix $(WEB_DIR) ci

dev-services:
	docker compose -f docker-compose.dev.yml up -d --wait

dev-services-down:
	docker compose -f docker-compose.dev.yml down

api:
	cargo run -p strife-api

web:
	npm --prefix $(WEB_DIR) run dev
