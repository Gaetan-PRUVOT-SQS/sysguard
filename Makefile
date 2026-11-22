BINAIRE := build/sysguard

.PHONY: tout construire test lint fmt fmt-check couverture audit campagne propre

tout: construire test

construire:
	CARGO_BUILD_WARNINGS=deny cargo build --release --locked
	mkdir -p build
	cp target/release/sysguard $(BINAIRE)

test:
	CARGO_BUILD_WARNINGS=deny cargo test

lint:
	CARGO_BUILD_WARNINGS=deny cargo clippy --all-targets

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

couverture:
	cargo llvm-cov --fail-under-lines 90 --fail-under-regions 90 --summary-only

audit:
	cargo audit

campagne: fmt-check lint test couverture audit construire

propre:
	rm -rf build target
