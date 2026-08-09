.PHONY: build install install-cli install-dev service-reinstall test check fmt lint clean changelog tauri dev reset-app reset-app-hard

BUNDLE_ID := com.beardedgiant.mdlive
APP_PATH := /Applications/mdlive.app

# default reset preserves LocalStorage (tabs, drafts, ui prefs).
# wipes only HTTP/asset caches + saved app state so new JS/HTML loads.
reset-app:
	@echo "Stopping mdlive daemon..."
	-@pkill -f "mdlive --daemon" 2>/dev/null || true
	-@rm -f $$HOME/.config/mdlive/daemon.port
	@echo "Wiping WKWebView HTTP/asset caches (LocalStorage preserved)..."
	-@find "$$HOME/Library/WebKit/$(BUNDLE_ID)/WebsiteData" -mindepth 1 -maxdepth 1 ! -name LocalStorage -exec rm -rf {} + 2>/dev/null || true
	-@rm -rf "$$HOME/Library/Caches/$(BUNDLE_ID)"
	-@rm -rf "$$HOME/Library/Saved Application State/$(BUNDLE_ID).savedState"
	@echo "App caches cleared (LocalStorage + Application Support kept)."

# nuclear option: wipe everything including LocalStorage + prefs.
# use when debugging from a clean slate.
reset-app-hard:
	@echo "Stopping mdlive daemon..."
	-@pkill -f "mdlive --daemon" 2>/dev/null || true
	-@rm -f $$HOME/.config/mdlive/daemon.port
	@echo "Wiping ALL WKWebView state for $(BUNDLE_ID) (incl. LocalStorage)..."
	-@rm -rf "$$HOME/Library/WebKit/$(BUNDLE_ID)"
	-@rm -rf "$$HOME/Library/Caches/$(BUNDLE_ID)"
	-@rm -rf "$$HOME/Library/Application Support/$(BUNDLE_ID)"
	-@rm -rf "$$HOME/Library/Saved Application State/$(BUNDLE_ID).savedState"
	-@defaults delete $(BUNDLE_ID) 2>/dev/null || true
	@echo "All app state cleared."

build:
	cargo build -p mdlive --release

tauri: build
	cp target/release/mdlive src-tauri/binaries/mdlive-cli-aarch64-apple-darwin
	cargo tauri build

install: reset-app tauri
	cp -r target/release/bundle/macos/mdlive.app /Applications/
	-xattr -dr com.apple.quarantine $(APP_PATH) 2>/dev/null || true
	cargo install --path .
	@echo "Installed mdlive.app to /Applications and CLI to ~/.cargo/bin/mdlive"

install-cli:
	cargo install --path .

# uninstall first: `service install` bakes current_exe() into the plist,
# so the CLI must be reinstalled to ~/.cargo/bin before install re-reads it.
service-reinstall:
	-mdlive service uninstall
	$(MAKE) install-cli
	mdlive service install
	mdlive service status

test:
	cargo test -p mdlive

check: fmt lint test

fmt:
	cargo fmt -p mdlive --check

lint:
	cargo clippy -p mdlive --all-targets --all-features -- -D warnings

clean:
	cargo clean

changelog:
	git cliff -o CHANGELOG.md

install-dev: reset-app
	MDLIVE_DEV=1 cargo build -p mdlive --release
	cp target/release/mdlive src-tauri/binaries/mdlive-cli-aarch64-apple-darwin
	MDLIVE_DEV=1 cargo tauri build
	cp -r target/release/bundle/macos/mdlive.app /Applications/
	-xattr -dr com.apple.quarantine $(APP_PATH) 2>/dev/null || true
	MDLIVE_DEV=1 cargo install --path .
	@echo "Installed dev build: $$(mdlive --version)"

dev:
	cargo run -p mdlive -- --daemon
