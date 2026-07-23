#!/bin/bash
# Verify install-time invariants of the libmixed.a static archive.
#
# Invariants:
#   1. llvmir/libmixed.a is a valid LLVM bitcode archive holding all three
#      members: ir_part1.o, ir_part2.o (bitcode) + native_part.o (appended
#      from _missing via llvm-ar rcsD).
#   2. The _missing sidecar is consumed in place; no surviving sidecar file.
#   3. The staging native libmixed.a also holds all three .o members, so a
#      downstream consumer can link it with plain `ar` + `gcc`.
#
# Env (set by the caller):
#   PREFIX   - install prefix (e.g. /usr/local)
#   DESTDIR  - staging root
#   LIBDIR   - install lib dir relative to PREFIX (e.g. /lib)
set -e
PREFIX=${PREFIX:-/usr/local}
DESTDIR=${DESTDIR:-.}
LIBDIR_REL=${LIBDIR_REL:-/lib}

INST_LLVMIR=$DESTDIR$PREFIX$LIBDIR_REL/llvmir/libmixed.a
INST_LIB=$DESTDIR$PREFIX$LIBDIR_REL/libmixed.a
INST_MISSING=$DESTDIR$PREFIX$LIBDIR_REL/llvmir/libmixed.a_missing

echo "==> llvmir archive members:"
llvm-ar t "$INST_LLVMIR"

MEMBERS=$(llvm-ar t "$INST_LLVMIR" | sort | tr '\n' ' ')
[ "$MEMBERS" = "ir_part1.o ir_part2.o native_part.o " ] \
    || { echo "FAIL: llvmir/libmixed.a members = '$MEMBERS'" >&2; exit 1; }

[ ! -e "$INST_MISSING" ] \
    || { echo "FAIL: $INST_MISSING should not exist after install" >&2; exit 1; }

for m in native_part ir_part1 ir_part2; do
    ar t "$INST_LIB" | grep -qx "$m.o" \
        || { echo "FAIL: staging libmixed.a missing member $m.o" >&2; exit 1; }
done

echo "==> OK: static archive install invariants passed."
