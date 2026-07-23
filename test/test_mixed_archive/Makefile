# Makefile for a mixed static archive test:
#   * native_part.o built directly with gcc (no clang-wrap, no .bc)
#   * ir_part1.o / ir_part2.o built with clang-wrap (-ffat-lto-objects, .bc produced)
#   * libmixed.a packaged via ar-wrap
#   * libmixed.a installed into $(DESTDIR)/usr/lib via install-wrap
#
# This directory exercises the full clang-wrap / ar-wrap / install-wrap chain:
#   * libmixed.a    — a static library mixing native (gcc) and bitcode-backed
#                     (clang-wrap) members, packaged via ar-wrap.
#   * appdemo       — an end-user application that links the installed
#                     libmixed.a (-lmixed) and libm (-lm), and is installed
#                     into $(DESTDIR)$(BINDIR) via install-wrap.
#
# The `verify` target pins the install-time invariants of libmixed.a:
#   1. llvmir/libmixed.a is a valid LLVM bitcode archive containing ir_part1.o,
#      ir_part2.o, and native_part.o (the latter appended from _missing).
#   2. The build-time _missing sidecar is consumed in place by install-wrap;
#      no surviving sidecar file remains next to the installed archive.
#   3. The staging $(DESTDIR)$(LIBDIR)/libmixed.a holds all three native .o
#      members, so any downstream consumer can still link against it.
#
# The `verify_app` target pins the application invariants:
#   * app_main.c + helper_ir.c (clang-wrap) + helper_native.c (gcc) compile
#     into an executable that links the installed libmixed.a (-lmixed) and
#     libm (-lm).  The application's main() calls every symbol exported by
#     libmixed.a (native_part / ir_part1 / ir_part2) plus helper_ir /
#     helper_native, exercising both independent .o files.
#   * install-wrap installs appdemo into $(DESTDIR)$(BINDIR) and synchronises
#     its sidecars under $(DESTDIR)$(LIBDIR)/llvmir-bin/.
#   * The installed binary still runs (returns 0) and `nm` confirms that all
#     three libmixed.a symbols + the helpers ended up as defined text
#     segments, while sin/cos/sqrt remain undefined and are resolved
#     out-of-line by libm at runtime.

CLANG   = ../../target/release/clang
CC      = gcc
AR      = ../../target/release/ar
INSTALL = ../../target/release/install

# Enable LLVM IR generation for clang-wrap / ar-wrap / install-wrap
EMIT_LLVMIR ?= 1
LLVM_IR_DIR  = $(CURDIR)/llvmir
export EMIT_LLVMIR
export LLVM_IR_DIR

DESTDIR ?= $(CURDIR)/image
PREFIX  ?= /usr/local
LIBDIR    = $(PREFIX)/lib
BINDIR    = $(PREFIX)/bin
INCLUDEDIR = $(PREFIX)/include

CFLAGS = -O2 -fPIC

# Sources
OBJECTS_NATIVE = native_part.o
OBJECTS_IR     = ir_part1.o ir_part2.o
OBJECTS        = $(OBJECTS_NATIVE) $(OBJECTS_IR)

# Archive (built by ar-wrap) and its installed destination
LIB = libmixed.a
LIB_INSTALL = $(DESTDIR)$(LIBDIR)/libmixed.a

# End-user application that links libmixed.a and libm, then is installed.
APP_HELPERS = helper_ir.o helper_native.o
APP_OBJS    = app_main.o $(APP_HELPERS)
APP_BIN     = appdemo
APP_INSTALL = $(DESTDIR)$(BINDIR)/$(APP_BIN)

.PHONY: all build_lib install verify clean \
        app install_app verify_app

# Default goal: archive → install → application build + install + verify.
all: install_app verify_app

# 1) Compile native_part.c with the system cc, no wrapper involvement.
native_part.o: native_part.c
	$(CC) $(CFLAGS) -c -o $@ $<

# 2) Compile ir_part{1,2}.c with clang-wrap; -ffat-lto-objects ensures ar-wrap
#    treats them as bitcode-backed members.
ir_part1.o: ir_part1.c
	$(CLANG) $(CFLAGS) -ffat-lto-objects -c -o $@ $<

ir_part2.o: ir_part2.c
	$(CLANG) $(CFLAGS) -ffat-lto-objects -c -o $@ $<

# 3) Archive the three .o files using ar-wrap → produces llvmir/libmixed.a plus
#    a _missing sidecar pointing at native_part.o.
$(LIB): $(OBJECTS)
	$(AR) crsD $@ $^

build_lib: $(LIB)

# 4) Install the archive into the staging tree via install-wrap.  The wrapper
#    will also materialise:
#       * $(DESTDIR)/usr/lib/llvmir/libmixed.a   (bitcode archive whose
#         `native_part.o` member is appended from the _missing sidecar)
#       * (no surviving $(DESTDIR)/usr/lib/llvmir/libmixed.a_missing file)
install: build_lib
	$(INSTALL) -d $(DESTDIR)$(LIBDIR)
	$(INSTALL) -m 644 $(LIB) $(LIB_INSTALL)

# ---------------------------------------------------------------------------
# End-user application: helper_ir (clang-wrap) + helper_native (gcc) +
# app_main.c (clang-wrap), linked against the installed libmixed.a and
# -lm, then installed via install-wrap into $(DESTDIR)$(BINDIR).
# ---------------------------------------------------------------------------

# helper_ir.o: clang-wrap produces both .o and .bc sidecar.
helper_ir.o: helper_ir.c
	$(CLANG) $(CFLAGS) -ffat-lto-objects -c -o $@ $<

# helper_native.o: gcc, no wrapper involvement, no .bc produced.
helper_native.o: helper_native.c
	$(CC) $(CFLAGS) -c -o $@ $<

# app_main.o: clang-wrap, so the final link picks up _cmd / _missing sidecars.
app_main.o: app_main.c
	$(CLANG) $(CFLAGS) -ffat-lto-objects -c -o $@ $<

# Final link: link against the build-time libmixed.a (the bitcode archive
# produced by ar-wrap in this directory, not the install-time copy) by
# giving its archive path directly, plus -lm for libm.  Using the
# build-tree $(LIB) path keeps the wrapper's image lookup in sync with
# the original ar-wrap output and lets the static library's bitcode be
# inlined into appdemo.bc.
$(APP_BIN): $(APP_OBJS) $(LIB)
	$(CLANG) $(CFLAGS) -o $@ $(APP_OBJS) \
	    $(LIB) -lm -lresolv

app: $(APP_BIN)

# Install the executable via install-wrap.  Synchronises llvmir-bin/$(APP).bc.
install_app: $(APP_BIN)
	$(INSTALL) -d $(DESTDIR)$(BINDIR)
	$(INSTALL) -m 755 $(APP_BIN) $(APP_INSTALL)

# Verify the application:
#   * installed binary exists, is executable, and runs successfully
#   * the bitcode sidecar's @ir_part1 and @ir_part2 come from libmixed.a
#     being merged into appdemo.bc (so libmixed.a should NOT be referenced
#     as a static archive in the _cmd sidecar any longer)
#   * nm shows the libmixed.a symbols as defined text segments while
#     sin/cos/sqrt remain undefined and are resolved out-of-line by libm
# Wrapper behaviour pinned by verify_lib.sh / verify_app.sh:
#   * ar-wrap produces native libmixed.a + llvmir/libmixed.a (bitcode),
#     with native_part.o recorded in libmixed.a_missing.
#   * install-wrap appends _missing entries into llvmir/libmixed.a via
#     `llvm-ar rcsD`, leaving no surviving sidecar.
#   * For executables, install-wrap rewrites _cmd to reference
#     asm/<hash>-<name>.o and leaves _missing pointing at those files.

LIBDIR_REL := $(patsubst $(PREFIX)%,%,$(LIBDIR))

verify_app: install_app
	APP_BIN='$(APP_BIN)' APP_INSTALL='$(APP_INSTALL)' \
	PREFIX='$(PREFIX)' DESTDIR='$(DESTDIR)' LIBDIR_REL='$(LIBDIR_REL)' \
	bash $(CURDIR)/verify_app.sh

verify: install
	PREFIX='$(PREFIX)' DESTDIR='$(DESTDIR)' LIBDIR_REL='$(LIBDIR_REL)' \
	bash $(CURDIR)/verify_lib.sh

clean:
	rm -rf $(OBJECTS_NATIVE) $(OBJECTS_IR) $(LIB) \
	       app_main.o helper_ir.o helper_native.o $(APP_BIN) \
	       llvmir image
