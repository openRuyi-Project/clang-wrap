# Mock LLVM IR RPM plugin

`mock-llvmir.py` enables LLVM IR generation and assigns installed IR files to
their native packages. It leaves the prepared RPM spec unchanged. For
declarative CMake specs, the plugin adds `CMAKE_PROJECT_INCLUDE` through an RPM
buildsystem macro, so RPM keeps its automatic configure, build, install, and
check stages. Meson specs are handled without changing `%__meson`: the
install-post helper discovers every Meson build directory with a
`meson-info/intro-installed.json` manifest below the package build directory
and runs `meson-install` against the current `RPM_BUILD_ROOT`. This works for
`BuildSystem: meson`, classic `%meson_install`, and hand-written `meson install`
invocations.

After `%install`, the plugin's RPM `__arch_install_post` hook runs a helper
that first synchronizes Meson-installed IR, then scans the populated buildroot,
resolves IR artifacts against the expanded `%files` rules, and writes an RPM
specpart. The existing architecture and operating-system install-post
processing is retained.

Each native package with publishable IR files gets a corresponding `-llvmir`
subpackage, which requires the corresponding native package at the same
version and release. It also has `Supplements: (<native-package> and
llvmir-converter)`, so the IR package is weakly selected when both the native
package and converter are installed. The generated file list owns the installed files,
preventing unpackaged-file errors. Packages without IR receive no empty
subpackage.
Unmatched or ambiguous ownership fails with the affected path rather than
silently assigning it. The postbuild RPM fallback remains available for
packages whose specs cannot be integrated.

The generated packages preserve `_cmd`, `_verscript`, version sidecars, and
`asm/` artifacts. Files ending in `_log` or `_missing` are excluded. This
follows the repository's [LLVM IR artifact protocol](../LLVMIR_ARTIFACT_PROTOCOL.md).

For a complete OpenRuyi rolling x86_64 example, start with
[`example/openruyi-rpm-rolling-unstable-x86_64.cfg`](example/openruyi-rpm-rolling-unstable-x86_64.cfg)
and replace `/path/to/clang-wrap/mock` with this repository's absolute `mock`
directory. The example enables the plugin through Mock's
`config_opts['plugin_conf']` table and installs `llvm`, which provides
`llvm-link` on OpenRuyi rolling. The plugin requires Mock's Python plugin API
1.1 plus `rpm`, `rpm2cpio`, `cpio`, and `rpmbuild` in the build host. Mock must
be able to read the input RPMs and write its result directory.

Enable the clang-wrap environment in Mock to generate IR:

```python
config_opts['environment']['EMIT_LLVMIR'] = '1'
config_opts['environment']['LLVM_IR_DIR'] = '/var/tmp/llvmir'
```

The supplied `llvmir-rpm.cfg` prepends `/var/lib/clang-wrap` to the buildroot's
`PATH` and sets `%toolchain` to `clang`. Install the wrappers at that path and
make the clang toolchain packages available in the buildroot. Meson builds also
require the `meson-install` companion from the clang-wrap package.

Run the unit tests with `python3 -m unittest discover -s mock/tests`. An
end-to-end build requires Mock and an RPM-based buildroot; the tests also
exercise RPM's dynamic specpart generation with local `rpmbuild` fixtures.

For package regression builds, `run-rpm-regression.sh` defaults to
`sqlite`, `libxml2`, `libwebp`, `brotli`, `nghttp3`, and `wayland` (`mariadb`
is currently commented out in the default matrix). Put one source RPM per
package in `mock/test-srpms/`; the script adjusts the example Mock config's
plugin directory automatically, then run:

```sh
mock/run-rpm-regression.sh
```

Pass package names to run a subset, or point the script at an existing SRPM
directory and Mock config:

```sh
mock/run-rpm-regression.sh --srpm-dir /path/to/srpms \
  --config /path/to/openruyi-mock.cfg libwebp sqlite
```

Each run gets a timestamped directory under `mock-regression-results/` with
one Mock log and a `summary.tsv`. A package passes only when Mock succeeds and
produces at least one `-llvmir` RPM. Missing or ambiguous SRPM matches fail
explicitly, making incomplete matrices visible.