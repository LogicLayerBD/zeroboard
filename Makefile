.PHONY: dev dev-backend dev-frontend release clean frontend backend setup db-reset backup

# ── Dev ──────────────────────────────────────────────────────────────────────

dev:
	@echo "Starting dev servers..."
	@$(MAKE) -j2 dev-backend dev-frontend

dev-backend:
	cd backend && cargo watch -x run

dev-frontend:
	cd frontend && npm run dev

# ── Build ─────────────────────────────────────────────────────────────────────

frontend:
	cd frontend && npm install && npm run build

backend: frontend
	cd backend && cargo build

release: frontend
	cd backend && cargo build --release
	@echo ""
	@echo "✅ Binary ready: backend/target/release/zeroboard"
	@ls -lh backend/target/release/zeroboard

# ── Setup ─────────────────────────────────────────────────────────────────────

setup:
	@echo "Installing frontend deps..."
	cd frontend && npm install
	@echo "Checking Rust toolchain..."
	rustc --version
	cargo --version
	@echo "✅ Setup complete. Run 'make dev' to start."

# ── Database ──────────────────────────────────────────────────────────────────

db-reset:
	rm -f backend/data/zeroboard.db backend/data/zeroboard.db-wal backend/data/zeroboard.db-shm
	@echo "Database reset. Will be recreated on next start."

backup:
	mkdir -p backups
	cd backend && cargo run -- backup --output ../backups/zeroboard-$$(date +%Y%m%d-%H%M%S).db

# ── Clean ─────────────────────────────────────────────────────────────────────

clean:
	cd frontend && rm -rf dist node_modules
	cd backend && cargo clean
