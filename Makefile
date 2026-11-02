PREFIX ?= /usr
DESTDIR ?=
USER_UNIT_DIR ?= $(PREFIX)/lib/systemd/user
CARGO ?= cargo
USER_PREFIX ?= $(HOME)/.local
VERSION := 0.1.0

.PHONY: build manifest test check docs-check install uninstall dist update-user recover-user
build:
	$(CARGO) build --workspace --release --locked
	python3 tests/build_manifest.py
manifest:
	python3 tests/build_manifest.py
test:
	$(CARGO) test --workspace --locked
	$(CARGO) build --workspace --locked
	python3 tests/native_arguments.py
	dbus-run-session -- python3 tests/integration.py
	dbus-run-session -- python3 tests/update_protocol.py
	dbus-run-session -- python3 tests/recording_responsiveness.py
check:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets --all-features --locked -- -D warnings
	python3 tests/check_docs.py
docs-check:
	python3 tests/check_docs.py
install: manifest
	install -Dm755 target/release/igpu-man "$(DESTDIR)$(PREFIX)/bin/igpu-man"
	install -Dm755 target/release/igpu-mand "$(DESTDIR)$(PREFIX)/bin/igpu-mand"
	install -Dm755 target/release/igpu-man-desktop "$(DESTDIR)$(PREFIX)/bin/igpu-man-desktop"
	install -Dm644 data/igpu-man.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/igpu-man.svg"
	install -Dm644 LICENSE "$(DESTDIR)$(PREFIX)/share/licenses/igpu-man/LICENSE"
	install -Dm644 data/config.example.toml "$(DESTDIR)$(PREFIX)/share/doc/igpu-man/config.example.toml"
	install -Dm644 README.md "$(DESTDIR)$(PREFIX)/share/doc/igpu-man/README.md"
	install -Dm644 target/release/build-info.json "$(DESTDIR)$(PREFIX)/share/doc/igpu-man/build-info.json"
	install -Dm644 AGENTS.md "$(DESTDIR)$(PREFIX)/share/doc/igpu-man/AGENTS.md"
	install -Dm644 CONTRIBUTING.md "$(DESTDIR)$(PREFIX)/share/doc/igpu-man/CONTRIBUTING.md"
	find docs -type f -exec install -Dm644 '{}' '$(DESTDIR)$(PREFIX)/share/doc/igpu-man/{}' \;
	python3 packaging/install_assets.py --prefix "$(PREFIX)" --unit-dir "$(USER_UNIT_DIR)" --destdir "$(DESTDIR)"
uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/igpu-man" "$(DESTDIR)$(PREFIX)/bin/igpu-mand" "$(DESTDIR)$(PREFIX)/bin/igpu-man-desktop"
	rm -f "$(DESTDIR)$(USER_UNIT_DIR)/igpu-man.service" "$(DESTDIR)$(PREFIX)/share/applications/igpu-man.desktop"
	rm -f "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/igpu-man.svg" "$(DESTDIR)$(PREFIX)/share/licenses/igpu-man/LICENSE"
	rm -rf -- "$(DESTDIR)$(PREFIX)/share/doc/igpu-man"
dist:
	python3 packaging/release.py

# Keep developer deployment separate from pacman-owned /usr files. Dependencies
# are invoked sequentially so `make -j` cannot deploy ahead of completed checks.
update-user:
	$(MAKE) check
	$(MAKE) test
	$(MAKE) build
	python3 packaging/update_user.py --prefix "$(USER_PREFIX)"
recover-user:
	@test -n "$(RUN)" || (echo 'Set RUN to the private interrupted update directory' >&2; exit 2)
	python3 packaging/update_user.py --prefix "$(USER_PREFIX)" --recover "$(RUN)"
