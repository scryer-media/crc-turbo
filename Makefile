PROJECT_NAME := crc_turbo

# Detect operating system
UNAME_S := $(shell uname -s)

# Determine OS-specific variables
ifeq ($(UNAME_S),Linux)
	DESTDIR ?= /usr/local
    LIB_EXTENSION := so
    STATIC_LIB_EXTENSION := a
    INSTALL_LIB_DIR := /lib
    INSTALL_BIN_DIR := /bin
    INSTALL_INCLUDE_DIR := /include
    POST_INSTALL := ldconfig
    CAPI_STATIC_LIBS := -lpthread -ldl -lm
    CAPI_RUN_ENV := LD_LIBRARY_PATH=capi/target/release
else ifeq ($(UNAME_S),Darwin)
    DESTDIR ?=
    # on macOS, there's not really a default location, so `install` requires DESTDIR
    DESTDIR_REQUIRED := 1
    LIB_EXTENSION := dylib
    CAPI_RUN_ENV := DYLD_LIBRARY_PATH=capi/target/release
    STATIC_LIB_EXTENSION := a
    INSTALL_LIB_DIR := /lib
    INSTALL_BIN_DIR := /bin
    INSTALL_INCLUDE_DIR := /include
    POST_INSTALL := true
else
    # Windows
    DESTDIR ?=
    ifeq ($(DESTDIR),)
        $(error On Windows, DESTDIR must be set for installation. Common locations include C:\)
    endif
    LIB_EXTENSION := dll
    STATIC_LIB_EXTENSION := lib
    # Use relative paths when DESTDIR is set to avoid path joining issues
    PREFIX ?= Program Files\\$(PROJECT_NAME)
    INSTALL_LIB_DIR := $(PREFIX)\\bin
    INSTALL_BIN_DIR := $(PREFIX)\\bin
    INSTALL_INCLUDE_DIR := $(PREFIX)\\include
    POST_INSTALL := true
endif

# Library name with extension
LIB_NAME := lib$(PROJECT_NAME).$(LIB_EXTENSION)
STATIC_LIB_NAME := lib$(PROJECT_NAME).$(STATIC_LIB_EXTENSION)

# CLI binaries
CLI_BINARIES := checksum arch-check get-custom-params

# Default target
.PHONY: all
all: build

# Build the library using Cargo
.PHONY: build
build: test
	cargo build --locked --all-features --release
	cargo build --locked --release --manifest-path capi/Cargo.toml

# Test the library using Cargo
.PHONY: test
test:
	cargo test --locked --all-features

# Refuse to install without a destination where the OS has no default one
.PHONY: check-destdir
check-destdir:
	@if [ -n "$(DESTDIR_REQUIRED)" ] && [ -z "$(DESTDIR)" ]; then \
		echo "DESTDIR must be set for installation. Common locations include /usr/local or /opt/homebrew" >&2; \
		exit 1; \
	fi

# Install the library and headers
.PHONY: install
install: check-destdir print-paths build
	@install -d $(DESTDIR)$(INSTALL_LIB_DIR)
	@install -d $(DESTDIR)$(INSTALL_BIN_DIR)
	@install -d $(DESTDIR)$(INSTALL_INCLUDE_DIR)

	install -m 644 capi/target/release/$(LIB_NAME) $(DESTDIR)$(INSTALL_LIB_DIR)/
	install -m 644 capi/target/release/$(STATIC_LIB_NAME) $(DESTDIR)$(INSTALL_LIB_DIR)/

	install -m 644 lib$(PROJECT_NAME).h $(DESTDIR)$(INSTALL_INCLUDE_DIR)/

	@for bin in $(CLI_BINARIES); do \
		install -m 755 target/release/$$bin $(DESTDIR)$(INSTALL_BIN_DIR)/; \
	done

	@if [ -z "$(DESTDIR)" ] && [ "$(POST_INSTALL)" != "true" ]; then \
		$(POST_INSTALL); \
	fi

# Uninstall the library and headers
.PHONY: uninstall
uninstall: print-paths
	rm -f $(DESTDIR)$(INSTALL_LIB_DIR)/$(LIB_NAME)
	rm -f $(DESTDIR)$(INSTALL_LIB_DIR)/$(STATIC_LIB_NAME)
	rm -f $(DESTDIR)$(INSTALL_INCLUDE_DIR)/lib$(PROJECT_NAME).h

	@for bin in $(CLI_BINARIES); do \
		rm -f $(DESTDIR)$(INSTALL_BIN_DIR)/$$bin; \
	done

	@if [ -z "$(DESTDIR)" ] && [ "$(UNAME_S)" = "Linux" ]; then \
		ldconfig; \
	fi

# Run all code quality checks
.PHONY: check
check:
	cargo fmt --all
	cargo check --workspace --all-targets --all-features
	cargo clippy --workspace --all-targets --all-features --fix --allow-dirty -- -D warnings
#	cargo deny check all
#	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
#	cargo audit

# Clean build artifacts
.PHONY: clean
clean:
	cargo clean
	cargo clean --manifest-path capi/Cargo.toml

# Print installation paths (useful for debugging)
.PHONY: print-paths
print-paths:
	@echo "Installation paths:"
	@echo "Library dir: $(DESTDIR)$(INSTALL_LIB_DIR)"
	@echo "Binary dir: $(DESTDIR)$(INSTALL_BIN_DIR)"
	@echo "Include dir: $(DESTDIR)$(INSTALL_INCLUDE_DIR)"

# Build the C library and run the C smoke test against the static and the
# shared library
.PHONY: capi-test
capi-test:
	cargo build --locked --release --manifest-path capi/Cargo.toml
	$(CC) -O2 -I. capi/tests/smoke.c capi/target/release/$(STATIC_LIB_NAME) $(CAPI_STATIC_LIBS) -o capi/target/smoke-static
	capi/target/smoke-static
	$(CC) -O2 -I. capi/tests/smoke.c -Lcapi/target/release -l$(PROJECT_NAME) -o capi/target/smoke-shared
	$(CAPI_RUN_ENV) capi/target/smoke-shared
