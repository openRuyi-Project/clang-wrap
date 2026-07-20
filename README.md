# clang-wrap

A wrapper tool for clang/clang++ that automatically generates and manages LLVM IR files during compilation and linking.

## Overview

clang-wrap wraps the clang/clang++ compiler and related tools (ar, cp, mv, ln, install, strip) to automatically:

- Generate LLVM IR (`.bc` files) during compilation
- Merge LLVM IR files during linking using `llvm-link`
- Synchronize LLVM IR files when performing file operations (copy, move, link, install)
- Handle static library creation with LLVM IR merging

## SPDX License Information

```
SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
SPDX-License-Identifier: MulanPSL-2.0
```

## Components

### Core Wrappers

- **clang-wrap** (`src/main.rs`): Main clang/clang++ wrapper
  - Generates LLVM IR during compilation
  - Invokes `llvm-link` during linking to merge LLVM IR files

### Tool Wrappers

- **ar-wrap** (`src/bin/ar-wrap.rs`): ar/llvm-ar wrapper
  - Merges LLVM IR files when creating static libraries

- **cp-wrap** (`src/bin/cp-wrap.rs`): cp wrapper
  - Copies corresponding LLVM IR files when copying binaries

- **mv-wrap** (`src/bin/mv-wrap.rs`): mv wrapper
  - Moves corresponding LLVM IR files when moving binaries

- **ln-wrap** (`src/bin/ln-wrap.rs`): ln wrapper
  - Creates links for corresponding LLVM IR files

- **install-wrap** (`src/bin/install-wrap.rs`): install wrapper
  - Installs LLVM IR files to appropriate locations

- **strip-wrap** (`src/bin/strip-wrap.rs`): strip wrapper
  - Handles LLVM IR file copying during strip operations

- **meson-install** (`src/bin/meson-install.rs`): Meson installation companion
  - Parses Meson's `intro-installed.json` directly
  - Installs LLVM IR and related `_cmd` and `_verscript` files into a Meson `destdir`

### Common Library

- **lib.rs** (`src/lib.rs`): Shared functionality including:
  - Debug logging
  - PATH executable finding
  - Environment variable reading
  - LLVM IR path computation
  - Auxiliary file handling (_log, _cmd, _verscript)

## Environment Variables

- `EMIT_LLVMIR`: Enable LLVM IR generation (set to non-empty, non-zero value)
- `LLVM_IR_DIR`: Output directory for LLVM IR files (default: `~/tmp/llvmir`)
- `CLANG_WRAP_DEBUG`: Enable debug logging (set to non-empty, non-zero value)

## Output Files

For each compiled/linked artifact, clang-wrap generates:

- `.bc` file: LLVM bitcode
- `_cmd` file: Shell script for re-linking LLVM bitcode
- `_log` file: Log of llvm-link command
- `_verscript` file: Version script (if applicable)

## Building

```bash
make build
```

## Installation

The compiled binaries should be placed in PATH before the actual tools they wrap, so they intercept calls to clang, ar, cp, etc.

To build and install locally into `clang-wrap-install/bin`:

```bash
make install
export PATH="$PWD/clang-wrap-install/bin:$PATH"
```

`make install` also detects locally available `clang`, `clang++`, `clang-NN`, and `clang++-NN` commands and creates matching symlinks to the `clang` wrapper. It also detects `llvm-ar` and target-prefixed GNU `ar` commands such as `x86_64-linux-gnu-ar`, creating matching symlinks to the `ar` wrapper.

## Usage

Once installed, the wrappers work transparently. Set `EMIT_LLVMIR=1` to enable LLVM IR generation:

```bash
EMIT_LLVMIR=1 make
```

### Meson

Meson uses its own internal installation implementation, so the `install` wrapper does not
observe `meson install`. Build normally with the compiler wrappers in `PATH`, then invoke
`meson-install` after Meson has populated the staging directory:

```bash
export PATH="$PWD/clang-wrap-install/bin:$PATH"
export EMIT_LLVMIR=1
export LLVM_IR_DIR="$PWD/llvmir"
export CC=clang
export CXX=clang++
export AR=ar

meson setup build
meson compile -C build
meson install -C build --destdir "$PWD/image"
meson-install build/meson-info/intro-installed.json "$PWD/image"
```

`meson-install` takes exactly two arguments: the path to Meson's
`intro-installed.json` and the same `destdir` supplied to `meson install`. The JSON is parsed
internally as a map from build product paths to configured absolute installation paths. For each
entry that has LLVM IR in `LLVM_IR_DIR`, it copies the bitcode, `_cmd`, and `_verscript` files to
the staging tree, using the same target layout and shared-library SONAME rewriting as the `install`
wrapper. Build logs (`_log`) remain only in `LLVM_IR_DIR` and are not installed.

## License

This project is licensed under the MulanPSL-2.0 license.

## Copyright

Copyright (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)

Copyright (C) 2026 openRuyi Project Contributors