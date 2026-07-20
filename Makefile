SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.ONESHELL:

PREFIX ?= $(CURDIR)/clang-wrap-install
BINDIR := $(PREFIX)/bin
RELEASE_DIR := target/release
WRAPPERS := clang ar install ln mv cp strip meson-install
INSTALL ?= /usr/bin/install

.PHONY: all build install install-binaries install-links clean-install test clippy fmt-check

all: build

build:
	cargo build --release

install: build install-binaries install-links

install-binaries:
	mkdir -p "$(BINDIR)"
	for tool in $(WRAPPERS); do
		"$(INSTALL)" -m 755 "$(RELEASE_DIR)/$$tool" "$(BINDIR)/$$tool"
	done

install-links:
	mkdir -p "$(BINDIR)"
	bindir="$$(cd "$(BINDIR)" && pwd -P)"
	mapfile -t clang_names < <(
		IFS=':' read -ra path_dirs <<< "$${PATH:-}"
		for dir in "$${path_dirs[@]}"; do
			[ -n "$$dir" ] || dir=.
			[ -d "$$dir" ] || continue
			dir_real="$$(cd "$$dir" 2>/dev/null && pwd -P)" || continue
			[ "$$dir_real" != "$$bindir" ] || continue
			for path in "$$dir"/clang "$$dir"/clang++ "$$dir"/clang-[0-9]* "$$dir"/clang++-[0-9]*; do
				[ -e "$$path" ] || continue
				[ -x "$$path" ] || continue
				name="$${path##*/}"
				[[ "$$name" =~ ^clang(\+\+)?(-[0-9]+)?$$ ]] || continue
				printf '%s\n' "$$name"
			done
		done | sort -u
	)
	for name in "$${clang_names[@]}"; do
		if [ "$$name" = clang ]; then
			continue
		fi
		ln -sfn clang "$(BINDIR)/$$name"
		printf 'linked %s -> clang\n' "$(BINDIR)/$$name"
	done
	mapfile -t ar_names < <(
		IFS=':' read -ra path_dirs <<< "$${PATH:-}"
		for dir in "$${path_dirs[@]}"; do
			[ -n "$$dir" ] || dir=.
			[ -d "$$dir" ] || continue
			dir_real="$$(cd "$$dir" 2>/dev/null && pwd -P)" || continue
			[ "$$dir_real" != "$$bindir" ] || continue
			for path in "$$dir"/llvm-ar "$$dir"/*-*-ar; do
				[ -e "$$path" ] || continue
				[ -x "$$path" ] || continue
				name="$${path##*/}"
				[[ "$$name" != gcc-ar && "$$name" != *-gcc-ar ]] || continue
				[[ "$$name" = llvm-ar || "$$name" =~ ^[[:alnum:]_][[:alnum:]_.+]*(-[[:alnum:]_][[:alnum:]_.+]*){2,}-ar$$ ]] || continue
				printf '%s\n' "$$name"
			done
		done | sort -u
	)
	for name in "$${ar_names[@]}"; do
		ln -sfn ar "$(BINDIR)/$$name"
		printf 'linked %s -> ar\n' "$(BINDIR)/$$name"
	done

clean-install:
	rm -rf "$(BINDIR)"

test:
	cargo test

clippy:
	cargo clippy --all-targets -- -D warnings

fmt-check:
	cargo fmt --check