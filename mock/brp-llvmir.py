# SPDX-License-Identifier: MulanPSL-2.0
"""Create RPM specparts for LLVM IR files installed into BUILDROOT."""

import fnmatch
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


MESON_INSTALL_CANDIDATES = (
    "/usr/lib64/clang-wrap/bin/meson-install",
    "/usr/lib/clang-wrap/bin/meson-install",
    "/usr/libexec/clang-wrap/bin/meson-install",
)


def find_meson_install():
    for candidate in MESON_INSTALL_CANDIDATES:
        if os.access(candidate, os.X_OK):
            return candidate
    return shutil.which("meson-install")


def find_meson_manifests():
    """Find Meson build directories below the package build directory."""
    roots = []
    for candidate in (Path.cwd(), Path(os.environ.get("RPM_BUILD_DIR", ""))):
        if not candidate.is_dir():
            continue
        resolved = candidate.resolve()
        if resolved not in roots:
            roots.append(resolved)

    manifests = set()
    for root in roots:
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [
                name for name in dirnames
                if name not in (".git", "BUILDROOT", "SPECPARTS")
            ]
            path = Path(dirpath)
            if path.name != "meson-info" or "intro-installed.json" not in filenames:
                continue
            if not (path.parent / "meson-private" / "build.dat").is_file():
                continue
            manifests.add(path / "intro-installed.json")
    return sorted(manifests)


def filter_manifest_for_buildroot(manifest, buildroot):
    """Keep only Meson installation entries present in the staged buildroot."""
    try:
        installed = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    if not isinstance(installed, dict):
        return None
    return {
        source: destination
        for source, destination in installed.items()
        if isinstance(destination, str)
        and os.path.isabs(destination)
        and os.path.lexists(buildroot / destination.lstrip("/"))
    }


def sync_meson_llvmir():
    """Copy LLVM IR for products recorded by any Meson build in this package."""
    manifests = find_meson_manifests()
    if not manifests:
        return
    meson_install = find_meson_install()
    if meson_install is None:
        raise RuntimeError(
            "Meson install manifest found but meson-install is not available"
        )
    buildroot = os.environ["RPM_BUILD_ROOT"]
    buildroot_path = Path(buildroot)
    with tempfile.TemporaryDirectory(prefix="llvmir-meson-") as temp:
        for index, manifest in enumerate(manifests):
            installed = filter_manifest_for_buildroot(manifest, buildroot_path)
            if installed is not None and not installed:
                continue
            manifest_path = manifest
            if installed is not None:
                manifest_path = Path(temp) / ("intro-installed-%d.json" % index)
                manifest_path.write_text(json.dumps(installed), encoding="utf-8")
            print("meson-install: %s -> %s" % (manifest, buildroot))
            status = subprocess.run(
                [meson_install, str(manifest_path), buildroot]
            ).returncode
            if status:
                raise RuntimeError("meson-install failed for %s" % manifest)


def expand_rpm_path(path):
    path = path.replace("%{name}", os.environ["RPM_PACKAGE_NAME"])
    path = re.sub(r"%\{\?_isa\}", "", path)
    if "%{" in path:
        result = subprocess.run(
            ["rpm", "--eval", path], check=True, capture_output=True, text=True,
        )
        path = result.stdout.strip()
    return path


def expand_braces(pattern):
    """Expand RPM file-list brace alternatives before applying fnmatch."""
    start = pattern.find("{")
    if start < 0:
        return [pattern]

    depth = 0
    separators = []
    end = None
    for index in range(start, len(pattern)):
        char = pattern[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                end = index
                break
        elif char == "," and depth == 1:
            separators.append(index)

    if end is None or not separators:
        return [pattern]

    alternatives = []
    alternative_start = start + 1
    for separator in separators:
        alternatives.append(pattern[alternative_start:separator])
        alternative_start = separator + 1
    alternatives.append(pattern[alternative_start:end])
    prefix, suffix = pattern[:start], pattern[end + 1:]
    return [expanded
            for alternative in alternatives
            for expanded in expand_braces(prefix + alternative + suffix)]


def package_name(package):
    return package.replace("%{name}", os.environ["RPM_PACKAGE_NAME"])


def main():
    sync_meson_llvmir()

    rules_path = Path(sys.argv[1] if len(sys.argv) > 1 else "/usr/lib/rpm/openruyi/llvmir-owner-rules.json")
    with rules_path.open(encoding="utf-8") as stream:
        config = json.load(stream)
    rules = config["rules"]
    package_suffix = config["package_suffix"]

    buildroot = Path(os.environ["RPM_BUILD_ROOT"])
    specparts = Path(os.environ["RPM_SPECPARTS_DIR"])
    files_by_owner = {rule["package"]: [] for rule in rules}
    roots = [buildroot / "usr/lib", buildroot / "usr/lib64"]
    ir_paths = sorted(
        path for root in roots if root.is_dir()
        for path in root.rglob("*")
        if (path.is_file() or path.is_symlink()) and
        re.search(r"/llvmir(?:-bin|-sbin|-libexec)?/", path.as_posix()) and
        not path.name.endswith(("_log", "_missing"))
    )

    for path in ir_paths:
        relative = "/" + path.relative_to(buildroot).as_posix()
        ir_match = re.search(r"/(llvmir(?:-bin|-sbin|-libexec)?)/(.*)$", relative)
        kind, artifact = ir_match.groups()
        base = artifact.rsplit("/", 1)[-1]
        for suffix in ("_cmd", "_verscript", ".version.txt", ".soversion.txt", ".bc"):
            if base.endswith(suffix):
                base = base[:-len(suffix)]
                break
        if "/asm/" in relative:
            base = base.split("-", 1)[-1]

        if kind == "llvmir":
            ir_prefix = relative.split("/llvmir/", 1)[0]
            candidates = [ir_prefix + "/" + base]
            if not (buildroot / candidates[0].lstrip("/")).exists():
                candidates.extend(("/usr/lib64/" + base, "/usr/lib/" + base,
                                   "/lib64/" + base, "/lib/" + base))
        elif kind == "llvmir-bin":
            candidates = ["/usr/bin/" + base, "/bin/" + base]
        elif kind == "llvmir-sbin":
            candidates = ["/usr/sbin/" + base, "/sbin/" + base]
        else:
            candidates = ["/usr/libexec/" + base]

        candidates = [candidate for candidate in candidates
                      if (buildroot / candidate.lstrip("/")).exists()]
        if not candidates:
            raise RuntimeError("no installed native file for %s" % relative)

        owners = set()
        for index, rule in enumerate(rules):
            if rule["manifest"]:
                manifest = rule["path"]
                manifest = manifest.replace("%{builddir}", os.environ["RPM_BUILD_DIR"])
                manifest = manifest.replace("%{?buildsubdir}", os.environ.get("RPM_BUILD_SUBDIR", ""))
                manifest = expand_rpm_path(manifest)
                manifest_path = Path(manifest)
                if not manifest_path.is_absolute():
                    manifest_path = Path.cwd() / manifest_path
                if not manifest_path.is_file():
                    raise RuntimeError("%files manifest not found: %s" % manifest_path)
                patterns = [line.strip() for line in manifest_path.read_text().splitlines()
                            if line.strip() and not line.lstrip().startswith("#")]
            else:
                patterns = [rule["path"]]
            for pattern in patterns:
                pattern = expand_rpm_path(pattern)
                for expanded_pattern in expand_braces(pattern):
                    for candidate in candidates:
                        if fnmatch.fnmatchcase(candidate, expanded_pattern):
                            owners.add(rule["package"])

        if not owners:
            raise RuntimeError(f"no %files owner matches {relative}")
        if len(owners) != 1:
            raise RuntimeError(
                f"ambiguous %files owner for {relative}: {', '.join(sorted(owners))}"
            )
        files_by_owner[owners.pop()].append(relative)

    specpart = specparts / "llvmir.specpart"
    with specpart.open("w", encoding="utf-8") as output:
        for index, rule in enumerate({rule["package"]: rule for rule in rules}.values()):
            files = sorted(set(files_by_owner[rule["package"]]))
            if not files:
                continue
            manifest_name = "llvmir-files-%d.list" % index
            (specparts / manifest_name).write_text("\n".join(files) + "\n", encoding="utf-8")
            package = package_name(rule["package"])
            ir_package = package + package_suffix
            output.write("%%package -n %s\n" % ir_package)
            output.write("Summary: LLVM IR artifacts for %s\n" % package)
            output.write("Requires: %s = %%{version}-%%{release}\n\n" % package)
            output.write("Supplements: (%s and llvmir-converter)\n\n" % package)
            output.write("%%description -n %s\n" % ir_package)
            output.write("LLVM IR artifacts for native package %s\n\n" % package)
            output.write("%%files -n %s -f %%{specpartsdir}/%s\n\n" %
                         (ir_package, manifest_name))


if __name__ == "__main__":
    main()