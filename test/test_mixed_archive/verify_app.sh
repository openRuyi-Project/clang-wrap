#!/bin/bash
# Verify install-time invariants of the appdemo executable.
#
# Invariants:
#   1. The installed binary runs and exits 0 (proves the link consumed
#      bitcode + asm/ helpers).
#   2. llvmir-bin/appdemo.bc is valid LLVM IR bitcode and embeds the
#      inlined ir_part1 / ir_part2 / helper_ir symbols from libmixed.a.
#   3. _cmd no longer references libmixed.a; it instead injects the two
#      missing native helpers (gcc-built) via `./asm/<hash>-<name>.o`.
#   4. The native fallbacks are materialised under asm/ beside _cmd.
#
# Env (set by the caller):
#   PREFIX   - install prefix (e.g. /usr/local)
#   DESTDIR  - staging root
#   LIBDIR   - install lib dir relative to PREFIX (e.g. /lib)
#   BINDIR   - install bin dir relative to PREFIX (e.g. /bin)
#   APP_INSTALL  - absolute path of the installed executable
#   APP_BIN      - executable name
set -e
PREFIX=${PREFIX:-/usr/local}
DESTDIR=${DESTDIR:-.}
LIBDIR_REL=${LIBDIR_REL:-/lib}
APP_BIN=${APP_BIN:-appdemo}
APP_INSTALL=${APP_INSTALL:-$DESTDIR$PREFIX/bin/$APP_BIN}

LLVMIR_BIN=$DESTDIR$PREFIX$LIBDIR_REL/llvmir-bin/$APP_BIN
INST_CMD=$DESTDIR$PREFIX$LIBDIR_REL/llvmir-bin/${APP_BIN}_cmd
INST_MISSING=$DESTDIR$PREFIX$LIBDIR_REL/llvmir-bin/${APP_BIN}_missing

fail() { echo "FAIL: $*" >&2; exit 1; }

[ -x "$APP_INSTALL" ] || fail "installed binary not executable: $APP_INSTALL"
[ -f "$LLVMIR_BIN" ] || fail "bitcode sidecar missing: $LLVMIR_BIN"
file "$LLVMIR_BIN" | grep -q 'LLVM IR bitcode' \
    || fail "$LLVMIR_BIN is not LLVM bitcode"

echo "==> bitcode defines (inlined from libmixed.a + helper_ir):"
llvm-dis "$LLVMIR_BIN" -o - 2>/dev/null | grep '^define' || true

# Symbols expected to be present in the executable bitcode:
#   * main, helper_ir, ir_part1, ir_part2 from clang-wrap (bitcode-backed)
# native_part and helper_native are gcc-built native objects, so they live
# in the .o sidecars — not in the bitcode.  They're verified via the
# runtime test below (running the binary).
for sym in ir_part1 ir_part2 helper_ir main; do
    llvm-dis "$LLVMIR_BIN" -o - 2>/dev/null | grep -q "@$sym\b" \
        || fail "bitcode missing symbol @$sym"
done

[ -f "$INST_CMD" ] || fail "_cmd missing: $INST_CMD"
grep -q 'libmixed\.a' "$INST_CMD" \
    && fail "_cmd still references libmixed.a"
grep -Eq -- '\./asm/[0-9a-f]+-helper_native\.o' "$INST_CMD" \
    || fail "_cmd missing ./asm/<hash>-helper_native.o"
grep -Eq -- '\./asm/[0-9a-f]+-native_part\.o' "$INST_CMD" \
    || fail "_cmd missing ./asm/<hash>-native_part.o"
[ -f "$DESTDIR$PREFIX$LIBDIR_REL/llvmir-bin/asm/"*'-helper_native.o' ] \
    || fail "installed helper_native fallback missing from asm/"
[ -f "$DESTDIR$PREFIX$LIBDIR_REL/llvmir-bin/asm/"*'-native_part.o' ] \
    || fail "installed native_part fallback missing from asm/"

echo "==> running the installed binary:"
"$APP_INSTALL"
RC=$?
echo "    exit code = $RC"
[ "$RC" = "0" ] || fail "installed binary exited $RC"

echo "==> OK: application install/runtime invariants passed."
