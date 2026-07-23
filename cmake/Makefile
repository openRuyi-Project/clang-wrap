# ============================================================================
# CMakeEmitLLVMIR - Makefile Build Entry
# ============================================================================
# For testing different environment variables and compiler configurations
#
# Usage:
#   make              - Default build
#   make llvmir       - Build with LLVM IR enabled
#   make clang        - Build with clang
#   make clean        - Clean build
#   make help         - Show help
# ============================================================================

CC ?= gcc
CMAKE_EMIT_LLVMIR ?= 0
BUILD_DIR ?= build
EXAMPLE_BUILD_DIR ?= $(BUILD_DIR)/example_build
INSTALL_DIR ?= $(CURDIR)/install

CMAKE = cmake

.PHONY: all configure build install clean distclean check smoke test help gcc gcc-llvmir clang llvmir llvmir-clang llvmir-clang-O3

all: configure build install
	@if [ -f $(INSTALL_DIR)/bin/myapp ]; then \
		LD_LIBRARY_PATH=$(INSTALL_DIR)/lib $(INSTALL_DIR)/bin/myapp; \
	fi
	@if [ -n "$(CMAKE_EMIT_LLVMIR)" ] && [ "$(CMAKE_EMIT_LLVMIR)" != "0" ] && [ "$(CMAKE_EMIT_LLVMIR)" != "OFF" ] && [ "$(CMAKE_EMIT_LLVMIR)" != "FALSE" ]; then \
		echo "Running libmylib.so.1_cmd..."; \
		cd $(INSTALL_DIR)/lib/llvmir && bash -x ./libmylib.so.1_cmd; \
		echo "Running myapp_cmd..."; \
		cd $(INSTALL_DIR)/lib/llvmir-bin && bash -x ./myapp_cmd; \
		echo "Running llvmir-bin/output/myapp with new libmylib.so.1..."; \
		LD_LIBRARY_PATH=$(INSTALL_DIR)/lib/llvmir/output $(INSTALL_DIR)/lib/llvmir-bin/output/myapp; \
	fi

configure:
	@mkdir -p $(EXAMPLE_BUILD_DIR)
	cd $(EXAMPLE_BUILD_DIR) && $(CMAKE) \
		-G "Unix Makefiles" \
		-DCMAKE_C_COMPILER=$(CC) \
		-DCMAKE_INSTALL_PREFIX=$(INSTALL_DIR) \
		-DCMAKE_BUILD_TYPE=Release \
		-DCMAKE_PROJECT_TOP_LEVEL_INCLUDES=$(CURDIR)/cmake/CMakeEmitLLVMIR.cmake \
		$(CURDIR)/example

build:
	$(CMAKE) --build $(EXAMPLE_BUILD_DIR) --verbose

install:
	$(CMAKE) --install $(EXAMPLE_BUILD_DIR) --prefix $(INSTALL_DIR)

gcc:
	@echo "Building gcc..."
	@$(MAKE) --no-print-directory CC=gcc CMAKE_EMIT_LLVMIR=0 EXAMPLE_BUILD_DIR=$(BUILD_DIR)/example_gcc INSTALL_DIR=$(INSTALL_DIR)/gcc
	@echo "gcc build successful"

gcc-llvmir:
	@echo "Building gcc-llvmir..."
	@$(MAKE) --no-print-directory CC=gcc CMAKE_EMIT_LLVMIR=1 EXAMPLE_BUILD_DIR=$(BUILD_DIR)/example_gcc_llvmir INSTALL_DIR=$(INSTALL_DIR)/gcc_llvmir
	@echo "gcc-llvmir build successful"

clang:
	@echo "Building clang..."
	@$(MAKE) --no-print-directory CC=clang CMAKE_EMIT_LLVMIR=0 EXAMPLE_BUILD_DIR=$(BUILD_DIR)/example_clang INSTALL_DIR=$(INSTALL_DIR)/clang
	@echo "clang build successful"

llvmir: llvmir-clang llvmir-clang-O3

llvmir-clang:
	@echo "Building llvmir-clang..."
	@$(MAKE) --no-print-directory CC=clang CMAKE_EMIT_LLVMIR=1 EXAMPLE_BUILD_DIR=$(BUILD_DIR)/example_clang_llvmir INSTALL_DIR=$(INSTALL_DIR)/clang_llvmir
	@echo "llvmir-clang build successful"

llvmir-clang-O3:
	@echo "Building llvmir-clang-O3..."
	@$(MAKE) --no-print-directory CC=clang CMAKE_EMIT_LLVMIR="-O3 -g" EXAMPLE_BUILD_DIR=$(BUILD_DIR)/example_clang_llvmir_O3 INSTALL_DIR=$(INSTALL_DIR)/clang_llvmir_O3
	@echo "llvmir-clang-O3 build successful"

clean:
	rm -rf $(BUILD_DIR)
	rm -rf $(INSTALL_DIR)

distclean: clean
	rm -rf $(EXAMPLE_BUILD_DIR)

check: gcc clang llvmir
	@echo "Testing gcc-llvmir (expected to fail)..."
	@if $(MAKE) gcc-llvmir; then \
		echo "Error: gcc-llvmir should fail but succeeded"; \
		exit 1; \
	else \
		echo "gcc-llvmir expected failure, test passed"; \
	fi
	@echo "Test complete"

smoke:
	@tmpdir=$$(mktemp -d "$${TMPDIR:-/tmp}/cmake-emit-llvmir-smoke.XXXXXX"); \
	trap 'rm -rf "$$tmpdir"' EXIT; \
	CMAKE_EMIT_LLVMIR=1 $(CMAKE) -S $(CURDIR)/example -B "$$tmpdir/build" -G Ninja \
		-DCMAKE_C_COMPILER=clang -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES=$(CURDIR)/cmake/CMakeEmitLLVMIR.cmake \
		-DCMAKE_INSTALL_PREFIX="$$tmpdir/install"; \
	$(CMAKE) --build "$$tmpdir/build" --parallel 2; \
	$(CMAKE) --install "$$tmpdir/build" --prefix "$$tmpdir/install"; \
	bash -n "$$tmpdir/install/lib/llvmir/libmylib.so.1_cmd"; \
	bash -n "$$tmpdir/install/lib/llvmir-bin/myapp_cmd"; \
	for cmd in "$$tmpdir/install/lib/llvmir/libmylib.so.1_cmd" "$$tmpdir/install/lib/llvmir-bin/myapp_cmd"; do \
		grep -qx "  '-x'" "$$cmd"; \
		grep -qx "  'ir'" "$$cmd"; \
		! grep -Eq 'CMakeFiles/|rpath|dependency-file|eval |read -r -a|_try_bc' "$$cmd"; \
	done; \
	(cd "$$tmpdir/install/lib/llvmir" && bash ./libmylib.so.1_cmd); \
	(cd "$$tmpdir/install/lib/llvmir-bin" && bash ./myapp_cmd); \
	LD_LIBRARY_PATH="$$tmpdir/install/lib/llvmir/output" "$$tmpdir/install/lib/llvmir-bin/output/myapp"

test:
	@tmpdir=$$(mktemp -d "$${TMPDIR:-/tmp}/cmake-emit-llvmir-ctest.XXXXXX"); \
	trap 'rm -rf "$$tmpdir"' EXIT; \
	$(CMAKE) -S $(CURDIR) -B "$$tmpdir/build" -G Ninja -DBUILD_TESTING=ON; \
	ctest --test-dir "$$tmpdir/build" --output-on-failure

help:
	@echo "CMakeEmitLLVMIR Build System"
	@echo ""
	@echo "Main targets:"
	@echo "  all          - Default build"
	@echo "  configure    - Configure only"
	@echo "  build        - Build only"
	@echo "  install      - Install"
	@echo "  smoke        - Build, install, relink, and run the bundled example"
	@echo "  test         - Run hermetic CTest fixtures"
	@echo "  clean        - Clean"
	@echo ""
	@echo "Preset configurations:"
	@echo "  gcc          - Use GCC"
	@echo "  clang        - Use Clang"
	@echo "  llvmir       - Clang + LLVM IR"
	@echo "  gcc-llvmir   - GCC + LLVM IR"
	@echo ""
	@echo "Available variables:"
	@echo "  CC              - Compiler (default: gcc)"
	@echo "  CMAKE_EMIT_LLVMIR - Enable LLVM IR (0/1)"
	@echo ""
	@echo "Examples:"
	@echo "  make llvmir"
	@echo "  make CC=clang"
	@echo "  make CMAKE_EMIT_LLVMIR=1"
