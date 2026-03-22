#cargo install cargo-all-features


YELLOW := \033[1;33m
GREEN  := \033[1;32m
CYAN   := \033[1;36m
RED    := \033[1;31m
NC     := \033[0m

DB_PG := postgres://user:password@localhost:2345/test_me
DB_MY := mysql://user:password@localhost:6033/test_me

NEXTEST := cargo nextest run --profile ci -j 1

.PHONY: help test test-postgres test-mysql test-all test-sea test-sqlx publish check

help:
	@echo "$(CYAN)Команди тестування (Nextest + Ізоляція):$(NC)"
	@echo "  $(GREEN)make test$(NC)            	- Базові тести"
	@echo "  $(GREEN)make test-postgres$(NC)    - Усі postgres Sea-ORM + SQLX"
	@echo "  $(GREEN)make test-mysql$(NC)       - Усі mysql Sea-ORM + SQLX"
	@echo "  $(GREEN)make test-sea$(NC)         - Усі Sea-ORM (PG, MY, SQLite)"
	@echo "  $(GREEN)make test-sqlx$(NC)        - Усі SQLx (PG, MY, SQLite)"
	@echo "  $(GREEN)make test-all$(NC)         - Повний прогін усіх комбінацій$(NC)"
	@echo "  $(YELLOW)make check$(NC)            - fmt + clippy$(NC)"
	@echo "  $(RED)make publish$(NC)         	- smart-release$(NC)"

test:
	$(NEXTEST) --no-default-features
	$(NEXTEST) --no-default-features --features logging
	$(NEXTEST) --no-default-features --features logging,context


test-postgres:
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sea-orm-postgres
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sea-orm-postgres,context
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sea-orm-postgres,logging
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sea-orm-postgres,logging,context
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sqlx-postgres
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sqlx-postgres,context
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sqlx-postgres,logging
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sqlx-postgres,logging,context


test-mysql:
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sea-orm-mysql
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sea-orm-mysql,context
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sea-orm-mysql,logging
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sea-orm-mysql,logging,context
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sqlx-mysql
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sqlx-mysql,context
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sqlx-mysql,logging
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sqlx-mysql,logging,context


test-sea:
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sea-orm-postgres
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sea-orm-postgres,context
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sea-orm-postgres,logging
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sea-orm-postgres,logging,context
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sea-orm-mysql
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sea-orm-mysql,context
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sea-orm-mysql,logging
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sea-orm-mysql,logging,context

test-sqlx:
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sqlx-postgres
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sqlx-postgres,context
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sqlx-postgres,logging
	DATABASE_URL="$(DB_PG)" $(NEXTEST) --no-default-features --features sqlx-postgres,logging,context
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sqlx-mysql
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sqlx-mysql,context
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sqlx-mysql,logging
	DATABASE_URL="$(DB_MY)" $(NEXTEST) --no-default-features --features sqlx-mysql,logging,context

test-all: test test-sea test-sqlx
	@echo "$(GREEN)Абсолютно всі комбінації перевірено в ізольованих процесах!$(NC)"

check:
	cargo fmt --all -- --check
	cargo clippy --workspace -- -D warnings

publish:
	@if ! command -v cargo-smart-release > /dev/null; then cargo install cargo-smart-release; fi
	cargo smart-release --execute --allow-fully-generated-changelogs --allow-dirty rust_bus