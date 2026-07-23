# CMakeEmitLLVMIR

CMakeEmitLLVMIR is a CMake module that automatically generates and preserves LLVM IR/bitcode files during the build process.

## Features

- **Automatic Processing**: When enabled, processes all supported targets created with `add_executable()` and `add_library()` without modifying their build definitions
- **C/C++ Support**: Generates bitcode for targets containing C and C++ sources, validating each enabled compiler before use
- **Complete Bitcode Generation**: Generates individual .bc files for each source file and merges them into complete bitcode matching the final output name
- **Link Script Generation**: Generates scripts for relinking, supporting binary reconstruction from bitcode
- **Version Script Support**: Automatically handles version and export scripts for shared libraries
- **Installation Support**: Provides CMake installation rules with separate installation option for LLVM IR files

## Quick Start

### Requirements

- CMake 3.24 or higher
- Clang compiler (supports `-emit-llvm` option)
- LLVM toolchain (llvm-link, llvm-dis, lld, etc.)
- Linux host with Bash (generated relink scripts use Bash)
- Ninja or Unix Makefiles generator

### Basic Usage

```bash
# 1. Set environment variable to enable LLVM IR emission
export CMAKE_EMIT_LLVMIR=1

# 2. Use Clang compiler
export CC=clang

# 3. Configure project (using CMAKE_PROJECT_TOP_LEVEL_INCLUDES)
cmake -B build -S . -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES=cmake/CMakeEmitLLVMIR.cmake

# 4. Build project and LLVM IR artifacts
cmake --build build

# 5. Install (including LLVM IR files)
cmake --install build --prefix install
```

Run the repository's self-contained smoke test with:

```bash
make smoke
```

**Note:** `CMAKE_PROJECT_TOP_LEVEL_INCLUDES` works for both simple projects and projects that use `add_subdirectory()` to define targets in subdirectories (requires CMake 3.24+).

The module currently supports C and C++ source files only. Non-C/C++ sources are left to the native CMake build and are not included in the generated bitcode. Supported generators are Ninja and Unix Makefiles; other generators are rejected during configuration.

### Example Project

This project includes a complete example demonstrating how to use CMakeEmitLLVMIR:

```
CMakeEmitLLVMIR-project/
├── cmake/
│   └── CMakeEmitLLVMIR.cmake    # Core module
├── example/
│   ├── mylib/                   # Example library
│   │   ├── include/
│   │   │   └── mylib.h
│   │   ├── src/
│   │   │   ├── math_ops.c
│   │   │   ├── string_ops.c
│   │   │   └── utils.c
│   │   └── libmylib.version
│   └── myapp/                   # Example application
│       └── main.c
└── CMakeLists.txt
```

## Configuration Options

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `CMAKE_EMIT_LLVMIR` | Enable/disable LLVM IR emission. Set to `1` or `ON` to enable | Not set (disabled) |
| `CMAKE_EMIT_LLVMIR` (with compile options) | Can include additional compile options | - |

### CMake Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `CMAKE_EMIT_LLVMIR_OUTPUT_DIR` | LLVM IR output directory | `${CMAKE_BINARY_DIR}/llvmir` |
| `CMAKE_EMIT_LLVMIR_INSTALL_DIR` | LLVM IR installation directory | `${CMAKE_INSTALL_LIBDIR}/llvmir` |

`CMAKE_EMIT_LLVMIR_INSTALL_DIR` is resolved after `project()` enables languages. This avoids initialization warnings when the module is supplied through `CMAKE_PROJECT_TOP_LEVEL_INCLUDES`.

`CMAKE_EMIT_LLVMIR` is the only enablement switch. Leave it unset, or set it to `0`, `OFF`, or `FALSE` to keep the native build unchanged. Set it to `1`/`ON` to generate IR for every supported target. A non-boolean value is also treated as additional Clang options, for example `CMAKE_EMIT_LLVMIR="-march=native -O2"`.

When enabled, the module forces `CMAKE_EXPORT_COMPILE_COMMANDS=ON` and reads the generated `compile_commands.json` at build time. Bitcode compilation therefore reuses CMake's real per-source compiler invocation, including target, directory, interface, and source-specific options. The supported generators are Ninja and Unix Makefiles.

For these single-config generators, LLVMIR artifact names follow the active `CMAKE_BUILD_TYPE`, including configuration-specific output names and `<CONFIG>_POSTFIX` such as `DEBUG_POSTFIX`.

## Output Files

After building, LLVM IR files are placed below the configured output root. Targets
defined in subdirectories retain that binary-directory layout:

```
build/llvmir/
├── libmylib.a.bc              # Static library bitcode
├── libmylib.so.1.bc           # Shared library bitcode (with SOVERSION)
├── libmylib.so.1_cmd          # Shared library link script
├── libmylib.so.1_verscript    # Version export script
├── libmylib.so.1.version.txt  # Version info
├── libmylib.so.1.soversion.txt # SOVERSION info
├── myapp.bc                   # Executable bitcode
├── myapp_cmd                  # Executable link script
└── output/                    # Relinked output directory
    ├── libmylib.so.1          # Relinked shared library from bitcode
    └── myapp                  # Relinked executable from bitcode
```

Note: The `output/` directory contains binaries relinked from bitcode using the generated `_cmd` scripts. Executable artifacts are installed to `lib/llvmir-bin`; library artifacts are installed to `lib/llvmir` by default.

## Tests

The repository provides hermetic CTest fixtures for enabled and disabled environment-variable behavior:

```bash
cmake -S . -B build-tests -G Ninja -DBUILD_TESTING=ON
ctest --test-dir build-tests --output-on-failure
```

The hermetic fixtures cover Ninja and Unix Makefiles, enabled/disabled modes, custom output roots, repeated configuration, Debug postfixes, versioned and custom-suffix shared libraries, generated sources, same-stem C/C++ sources, OBJECT libraries, cyclic and forward-declared static dependencies, INTERFACE/alias dependency propagation, `EXPORT`/`FILE_SET` installation syntax, LLVMIR component installation, installed relink scripts, nested response-file expansion, and ambiguous Ninja/compilation-database command rejection.

## Known Limitations

- The module supports Clang C and C++ compilation on Linux hosts only. It requires Ninja or Unix Makefiles and rejects all other generators.
- Bitcode compilation is derived from `compile_commands.json`, so normal per-source options and evaluated compile settings are preserved. PCH, unity builds, IPO, and toolchains that do not emit a compilation database still require integration coverage.
- Relink commands are derived from generator-native metadata at build time. Complex imported/configuration-specific targets, compiler launchers, shell-wrapped link commands, cross-compilation toolchains, PCH, unity builds, IPO, and linker response files need an integration test before production use. Response files are recursively expanded with strict cycle, missing-file and quoting checks; ambiguous Ninja link-command discovery and ambiguous compile-database source matches are rejected rather than producing partial scripts.

## Installation

After running `cmake --install build --prefix install`, files are installed to the specified prefix:

```
install/
├── bin/
│   └── myapp                  # Installed executable
├── include/
│   └── mylib.h                # Installed header file
└── lib/
    ├── libmylib.a             # Installed static library
    ├── libmylib.so            # Installed shared library symlink
    ├── libmylib.so.1          # Installed shared library (SOVERSION)
    ├── libmylib.so.1.0.0      # Installed shared library (full version)
    ├── llvmir/                # LLVM IR files for shared library (when CMAKE_EMIT_LLVMIR is enabled)
    │   ├── libmylib.a.bc
    │   ├── libmylib.so.1.bc
    │   ├── libmylib.so.1_cmd          # Link script for shared library
    │   ├── libmylib.so.1_verscript    # Version export script
    │   ├── libmylib.so.1.version.txt  # Version info
    │   ├── libmylib.so.1.soversion.txt # SOVERSION info
    │   └── output/
    │       ├── libmylib.so.1          # Relinked library from bitcode
    │       └── libmylib.so.1_verscript
    └── llvmir-bin/            # LLVM IR files for executable (when CMAKE_EMIT_LLVMIR is enabled)
        ├── myapp.bc
        ├── myapp_cmd                  # Link script for executable
        └── output/
            └── myapp                  # Relinked executable from bitcode
```

Note: The `llvmir/` and `llvmir-bin/` directories are only created when `CMAKE_EMIT_LLVMIR` is set to a non-empty value (not `0`, `OFF`, or `FALSE`).

## Verification and Usage

### Check Generated Bitcode

```bash
# List all bitcode files
find build/llvmir -name '*.bc' -print

# Disassemble bitcode to view LLVM IR
llvm-dis build/llvmir/myapp.bc -o - | head -50

# Verify bitcode validity
llvm-link build/llvmir/myapp.bc -o /dev/null
echo "Bitcode is valid!"
```

### Relink from Bitcode

```bash
cd build/llvmir

# Relink executable from bitcode
bash myapp_cmd

# Relink shared library from bitcode
bash libmylib.so.1_cmd
```

### Run Example Program

```bash
# Build and run
cmake --build build
./build/myapp
```

Expected output:
```
=== MyApp - CMakeEmitLLVMIR Test ===

MyLib version 1.0.0

--- Math Operations ---
a = 10, b = 5
add(10, 5) = 15
subtract(10, 5) = 5
multiply(10, 5) = 50
divide(10, 5) = 2

--- String Operations ---
Length of "Hello, LLVM IR!" = 15
compare("abc", "abd") = -1

--- Utility Functions ---
factorial(5) = 120
fibonacci(10) = 55

=== Test Complete ===
```

## Using in Your Project

### Recommended Method: CMAKE_PROJECT_TOP_LEVEL_INCLUDES

The recommended way to use CMakeEmitLLVMIR is with `CMAKE_PROJECT_TOP_LEVEL_INCLUDES` (requires CMake 3.24+):

```bash
export CMAKE_EMIT_LLVMIR=1
export CC=clang
cmake -B build -S . -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES=/path/to/CMakeEmitLLVMIR.cmake
cmake --build build
```

This method works for **all project types**:
- Simple projects with targets defined in the top-level CMakeLists.txt
- Complex projects using `add_subdirectory()` to define targets in subdirectories

**Why CMAKE_PROJECT_TOP_LEVEL_INCLUDES?**

When using `CMAKE_PROJECT_TOP_LEVEL_INCLUDES`, the module is included at the beginning of the top-level `project()` call. This ensures that:
1. All targets are processed, regardless of where they are defined
2. Targets from `add_subdirectory()` calls are automatically included
3. No manual configuration is needed

**Note:** CMake 3.24 or higher is required for `CMAKE_PROJECT_TOP_LEVEL_INCLUDES`.

## Troubleshooting

### No Bitcode Files Generated

1. Ensure `CMAKE_EMIT_LLVMIR=1` is set
2. Ensure Clang is used: `export CC=clang`
3. Ensure `CMAKE_PROJECT_TOP_LEVEL_INCLUDES` points to the correct module path
4. Check configuration output for "LLVM IR/Bitcode emission enabled" message
5. Ensure CMake 3.24 or higher is used

### llvm-link Not Found

```bash
# Ubuntu/Debian
sudo apt-get install llvm

# Fedora/RHEL
sudo dnf install llvm
```

### Compiler Errors

Ensure you use a Clang-based compiler that supports the `-emit-llvm` option. The module validates Clang and matching LLVM tools during configuration.

## Tested Projects

This module has been tested with the following open-source projects:

### Libraries (Static + Dynamic)

| Project | Description | Test Command |
|---------|-------------|--------------|
| brotli | Compression library | `./testdeb.sh brotli` |
| c-ares | DNS resolution library | `./testdeb.sh c-ares` |
| libjpeg-turbo | Image codec library | `./testdeb.sh libjpeg-turbo` |
| libpng | PNG image processing library | `./testdeb.sh libpng` |
| zlib | Compression library | `./testdeb.sh zlib` |
| curl | HTTP client library | `./testdeb.sh curl` |
| tidy-html5 | HTML syntax checker | `./testdeb.sh tidy-html5` |

### Libraries (Separate Static/Dynamic Tests)

| Project | Description | Static Test | Dynamic Test |
|---------|-------------|-------------|--------------|
| leveldb | Key-value database | `./testdeb.sh leveldb-static` | `./testdeb.sh leveldb-shared` |
| spdlog | Logging library | `./testdeb.sh spdlog-static` | `./testdeb.sh spdlog-shared` |
| googletest | Testing framework | `./testdeb.sh googletest-static` | `./testdeb.sh googletest-shared` |
| cpuinfo | CPU detection library | `./testdeb.sh cpuinfo-static` | `./testdeb.sh cpuinfo-shared` |
| glfw | OpenGL window library | `./testdeb.sh glfw-static` | `./testdeb.sh glfw-shared` |
| yaml-cpp | YAML parser | `./testdeb.sh yaml-cpp-static` | `./testdeb.sh yaml-cpp-shared` |
| libxml2 | XML parser | `./testdeb.sh libxml2-static` | `./testdeb.sh libxml2-shared` |
| armadillo | Linear algebra library | `./testdeb.sh armadillo-static` | `./testdeb.sh armadillo-shared` |
| libwebp | Image codec library | `./testdeb.sh libwebp-static` | `./testdeb.sh libwebp-shared` |

### Header-Only Libraries

| Project | Description | Test Command |
|---------|-------------|--------------|
| rapidjson | JSON parser | `./testdeb.sh rapidjson` |

### Running All Tests

```bash
# Test all projects
./testdeb.sh

# Test specific project
./testdeb.sh brotli

# Test multiple projects
./testdeb.sh brotli c-ares zlib

# Clean test directories
./testdeb.sh --clean

# Show help
./testdeb.sh --help
```

## License

This module is licensed under the BSD 3-Clause License. See LICENSE file for details.

## Contributing

Issues and pull requests are welcome!
