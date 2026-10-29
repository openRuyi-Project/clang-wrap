# SPDX-License-Identifier: MulanPSL-2.0
"""Mock postbuild plugin for installed clang-wrap LLVM IR artifacts."""

import json
import inspect
import os
import re
import shlex
import shutil
import subprocess
import tempfile
from pathlib import Path

requires_api_version = "1.1"
run_in_bootstrap = False

IR_PREFIXES = (
    "/usr/lib/llvmir/",
    "/usr/lib/llvmir-bin/",
    "/usr/lib/llvmir-sbin/",
    "/usr/lib/llvmir-libexec/",
)
EXCLUDED_SUFFIXES = ("_log", "_missing")
SPEC_MARKER = "# mock-llvmir-plugin-managed"
CMK_SPEC_MARKER = "# mock-llvmir-cmake-managed"
CMK_MODULE = "/usr/lib64/clang-wrap/cmake/CMakeEmitLLVMIR.cmake"
CLANG_WRAP_PATH = "/var/lib/clang-wrap"
BRP_HELPER_PATH = "/usr/lib/rpm/openruyi/brp-llvmir.py"
BRP_RULES_PATH = "/usr/lib/rpm/openruyi/llvmir-owner-rules.json"
BRP_HELPER_SOURCE = Path(__file__).with_name("brp-llvmir.py")


def _section_end(lines, start):
    section_directives = {
        "%prep", "%generate_buildrequires", "%conf", "%build", "%install",
        "%check", "%clean", "%files", "%package", "%description", "%changelog", "%autochangelog",
        "%pre", "%post", "%preun", "%postun", "%pretrans", "%posttrans",
        "%trigger", "%triggerin", "%triggerun", "%triggerpostun",
        "%filetriggerin", "%filetriggerun", "%filetriggerpostun",
        "%transfiletriggerin", "%transfiletriggerun", "%transfiletriggerpostun",
    }
    for index in range(start + 1, len(lines)):
        match = re.match(r"^%([A-Za-z][A-Za-z0-9_]*)\b", lines[index])
        if match and "%" + match.group(1) in section_directives:
            return index
    return len(lines)


def _files_package_name(header):
    tokens = shlex.split(header)
    if len(tokens) == 1:
        return "%{name}"
    if "-n" in tokens:
        index = tokens.index("-n")
        if index + 1 < len(tokens):
            return tokens[index + 1]
        raise ValueError("%files -n requires a package name: %s" % header)
    subname = next((token for token in tokens[1:] if not token.startswith("-")), None)
    if not subname:
        raise ValueError("cannot determine package from %files header: %s" % header)
    return "%{name}-" + subname


def _files_section_rules(spec_text):
    lines = spec_text.splitlines()
    sections = []
    for index, line in enumerate(lines):
        match = re.match(r"^%files(?:\s+(.*))?$", line.strip())
        if match:
            header = "%files" + (" " + match.group(1) if match.group(1) else "")
            header_tokens = shlex.split(header)
            manifest = None
            if "-f" in header_tokens:
                manifest_index = header_tokens.index("-f") + 1
                if manifest_index >= len(header_tokens):
                    raise ValueError("%files -f is missing its manifest path: %s" % header)
                manifest = header_tokens[manifest_index]
            sections.append(("%files" + (" " + match.group(1) if match.group(1) else ""),
                             lines[index + 1:_section_end(lines, index)], manifest))
    if not sections:
        raise ValueError("RPM spec has no %files sections")

    rules = []
    for header, body, manifest in sections:
        package = _files_package_name(header)
        if manifest:
            if "%{" in manifest or "$" in manifest:
                raise ValueError("cannot safely expand dynamic %files manifest path: %s" % manifest)
            rules.append((package, "@manifest@" + manifest))
        for line in body:
            value = line.strip()
            if not value or value.startswith("#"):
                continue
            excluded = value.startswith("%exclude ")
            tokens = shlex.split(value)
            if not tokens:
                continue
            if tokens[0] == "%dir":
                continue
            while tokens and tokens[0].startswith("%") and not tokens[0].startswith("%{"):
                tokens = tokens[1:]
            if not tokens:
                continue
            path = tokens[-1]
            if path.startswith(("/", "%{")):
                rules.append((package, "@exclude@" + path if excluded else path))
    return rules


def _shell_quote(value):
    return "'" + value.replace("'", "'\\''") + "'"


def _do_chroot_plugin(buildroot, command):
    """Run a plugin command across Mock's output-return API variants."""
    method = buildroot.doChrootPlugin
    parameters = inspect.signature(method).parameters
    if "returnOutput" in parameters or any(
        parameter.kind is inspect.Parameter.VAR_KEYWORD for parameter in parameters.values()
    ):
        return method(command, returnOutput=True)
    if "return_output" in parameters:
        return method(command, return_output=True)
    config = getattr(buildroot, "config", {})
    return buildroot.doChroot(
        command,
        shell=False,
        cwd=None,
        logger=buildroot.build_log,
        uid=buildroot.chrootuid,
        gid=buildroot.chrootgid,
        user=buildroot.chrootuser,
        unshare_net=not config.get("rpmbuild_networking", False),
        printOutput=config.get("print_main_output", True),
        returnOutput=True,
    )


def configure_cmake_llvmir(spec_path, module_path=CMK_MODULE, wrapper_path=CLANG_WRAP_PATH):
    """Detect a declarative CMake spec without modifying it."""
    spec_path = Path(spec_path)
    text = spec_path.read_text()
    return bool(re.search(r"^BuildSystem:\s*cmake\s*$", text, re.IGNORECASE | re.MULTILINE))


def install_buildroot_helper(buildroot, rules, package_suffix, is_cmake=False):
    helper_path = Path(buildroot.make_chroot_path(BRP_HELPER_PATH))
    rules_path = Path(buildroot.make_chroot_path(BRP_RULES_PATH))
    with buildroot.uid_manager.elevated_privileges():
        helper_path.parent.mkdir(parents=True, exist_ok=True)
        rules_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(BRP_HELPER_SOURCE, helper_path)
        helper_path.chmod(0o755)
        rules_path.write_text(
            json.dumps({"rules": rules, "package_suffix": package_suffix}, separators=(",", ":")) + "\n"
        )

    original, status = _do_chroot_plugin(
        buildroot, ["rpm", "--eval", "%{macrobody:__arch_install_post}"]
    )
    if status:
        raise RuntimeError("cannot read RPM install-post macro")
    install_post = original.rstrip() + "\n/usr/bin/python3 " + BRP_HELPER_PATH

    macrofile = Path(buildroot.make_chroot_path(buildroot.homedir, ".rpmmacros"))
    with macrofile.open("a", encoding="utf-8") as stream:
        stream.write("\n%%__arch_install_post %s\n" % install_post.replace("\n", chr(92) + "\n"))
        if is_cmake:
            stream.write("%%buildsystem_cmake_conf() %%cmake -DCMAKE_PROJECT_INCLUDE=%s %%*\n" % CMK_MODULE)


def owner_rule_config(spec_path, expanded_text=None):
    rules = []
    text = spec_path.read_text() if expanded_text is None else expanded_text
    for package, path in _files_section_rules(text):
        if path.startswith("@manifest@"):
            manifest = path[len("@manifest@"):]
            rules.append({"package": package, "path": manifest, "manifest": True})
        elif path.startswith("@exclude@"):
            rules.append({"package": package, "path": path[len("@exclude@"):],
                          "manifest": False, "exclude": True})
        else:
            rules.append({"package": package, "path": path, "manifest": False})
    return rules


def _enable_cmake_build_environment(buildroot, wrapper_path=CLANG_WRAP_PATH):
    environment = buildroot.env
    previous = {name: environment.get(name) for name in ("PATH", "CMAKE_EMIT_LLVMIR")}
    path_entries = environment.get("PATH", "").split(os.pathsep)
    environment["PATH"] = os.pathsep.join(entry for entry in path_entries if entry != wrapper_path)
    environment["CMAKE_EMIT_LLVMIR"] = "1"
    return previous


def _restore_build_environment(buildroot, previous):
    for name, value in previous.items():
        if value is None:
            buildroot.env.pop(name, None)
        else:
            buildroot.env[name] = value


def _match_rule(candidate, pattern):
    if pattern.startswith("!"):
        return False
    if Path(candidate).match(pattern):
        return True
    return candidate.startswith(pattern.rstrip("/") + "/")


def infer_package_owner(candidate_paths, rules):
    matches = []
    for candidate in candidate_paths:
        for package, pattern in rules:
            if _match_rule(candidate, pattern):
                matches.append(package)
    if not matches:
        return None
    return sorted(set(matches))


def _llvmir_package_name(package, suffix):
    return ("%{name}" if package == "%{name}" else package) + suffix


def is_publishable_path(path):
    return path.startswith(IR_PREFIXES) and not Path(path).name.endswith(EXCLUDED_SUFFIXES)


def parse_nevra(rpm_path):
    result = subprocess.run(
        ["rpm", "-qp", "--qf", "%{NAME}\n%{VERSION}\n%{RELEASE}\n%{ARCH}\n", str(rpm_path)],
        check=True, capture_output=True, text=True,
    )
    fields = result.stdout.strip().splitlines()
    if len(fields) != 4:
        raise ValueError("rpm returned incomplete package metadata for %s" % rpm_path)
    return fields


def package_filelist(rpm_path):
    result = subprocess.run(["rpm", "-qpl", str(rpm_path)], check=True, capture_output=True, text=True)
    return [line for line in result.stdout.splitlines() if line.startswith("/")]


def package_names(rpm_path):
    result = subprocess.run(
        ["rpm", "-qp", "--qf", "%{NAME}\n", str(rpm_path)],
        check=True, capture_output=True, text=True,
    )
    return result.stdout.splitlines()


def selected_files(rpm_path):
    return [path for path in package_filelist(rpm_path) if is_publishable_path(path)]


def _check_content_for_path_leaks(path, forbidden_roots):
    try:
        data = path.read_bytes()
    except OSError:
        return
    for root in forbidden_roots:
        if root and root.encode() in data:
            raise ValueError("build or DESTDIR path leaked into LLVM IR artifact %s" % path)


def build_llvmir_rpm(rpm_path, resultdir, package_suffix="-llvmir", destdir=None):
    name, version, release, arch = parse_nevra(rpm_path)
    if name.endswith(package_suffix) or arch == "src":
        return None
    files = selected_files(rpm_path)
    if not files:
        return None
    with tempfile.TemporaryDirectory(prefix="mock-llvmir-") as temp:
        topdir = Path(temp) / "rpmbuild"
        stage = Path(temp) / "stage"
        for directory in ("BUILD", "RPMS", "SOURCES", "SPECS", "SRPMS"):
            (topdir / directory).mkdir(parents=True)
        extraction_dir = Path(temp) / "extract"
        extraction_dir.mkdir()
        extract = subprocess.Popen(["rpm2cpio", str(rpm_path)], stdout=subprocess.PIPE)
        unpack = subprocess.run(["cpio", "-idm", "--quiet", "-D", str(extraction_dir)], stdin=extract.stdout)
        extract.stdout.close()
        if unpack.returncode or extract.wait():
            raise subprocess.CalledProcessError(unpack.returncode or extract.returncode, "rpm2cpio | cpio")
        for item in files:
            source = extraction_dir / item.lstrip("/")
            target = stage / item.lstrip("/")
            if not source.exists() and not source.is_symlink():
                raise FileNotFoundError("RPM listed LLVM IR path missing after extraction: %s" % item)
            target.parent.mkdir(parents=True, exist_ok=True)
            if source.is_symlink():
                target.symlink_to(os.readlink(source))
            elif source.is_dir():
                shutil.copytree(source, target, dirs_exist_ok=True, symlinks=True)
            else:
                shutil.copy2(source, target)
                _check_content_for_path_leaks(target, [temp, destdir, os.environ.get("DESTDIR")])
        spec = topdir / "SPECS" / "llvmir.spec"
        entries = "\n".join(sorted(set(files)))
        spec.write_text("""Name: %s
Version: %s
Release: %s
Summary: LLVM IR artifacts for %s
License: MIT
BuildArch: %s
Requires: %s = %s-%s
Supplements: (%s and llvmir-converter)

%%description
Installed LLVM IR artifacts extracted from %%s.

%%install
mkdir -p "${RPM_BUILD_ROOT}"
cp -a "%s/." "${RPM_BUILD_ROOT}/"

%%files
%s
""" % (name + package_suffix, version, release, name, arch, name, version, release,
       name, stage, entries))
        subprocess.run(
            ["rpmbuild", "-bb", "--define", "_topdir %s" % topdir, str(spec)],
            check=True, capture_output=True, text=True,
        )
        rpms = list((topdir / "RPMS").rglob("*.rpm"))
        if len(rpms) != 1:
            raise RuntimeError("expected one LLVM IR RPM, found %d" % len(rpms))
        output = Path(resultdir) / rpms[0].name
        shutil.copy2(rpms[0], output)
        return output


def postbuild(buildroot, conf, logger):
    if not conf.get("enable", True):
        return
    suffix = conf.get("package_suffix", "-llvmir")
    for rpm in buildroot.final_rpm_list or []:
        path = Path(rpm)
        if not path.is_absolute():
            path = Path(buildroot.resultdir) / path
        if path.name.endswith(".src.rpm") or suffix + "-" in path.name or suffix + "." in path.name:
            continue
        try:
            name, _, _, arch = parse_nevra(path)
            if arch == "src" or name.endswith(suffix) or name + suffix in package_names(path):
                continue
            build_llvmir_rpm(path, buildroot.resultdir, suffix, conf.get("destdir"))
        except Exception as exc:
            logger.error("Failed to create LLVM IR RPM for %s: %s", path, exc)


def init(plugins, conf, buildroot):
    logger = getattr(buildroot, "root_log", None)
    if logger is None:
        import logging
        logger = logging.getLogger(__name__)
    cmake_environment = {}
    saved_macros = {}

    def restore_build_configuration():
        if saved_macros:
            saved_macros["path"].write_bytes(saved_macros["content"])
            saved_macros.clear()
        if cmake_environment:
            _restore_build_environment(buildroot, cmake_environment)
            cmake_environment.clear()

    def build_spec_path():
        spec = getattr(buildroot, "spec", None)
        if spec:
            return spec

        spec_dir = Path(buildroot.make_chroot_path(buildroot.builddir, "SPECS"))
        specs = list(spec_dir.glob("*.spec"))
        if len(specs) != 1:
            raise RuntimeError("expected one prepared RPM spec in %s, found %d" % (spec_dir, len(specs)))
        return os.path.join(buildroot.builddir, "SPECS", specs[0].name)

    def prebuild_hook():
        spec = build_spec_path()
        spec_path = Path(buildroot.make_chroot_path(spec))
        macrofile = Path(buildroot.make_chroot_path(buildroot.homedir, ".rpmmacros"))
        saved_macros.update(path=macrofile, content=macrofile.read_bytes())
        try:
            expanded, status = _do_chroot_plugin(
                buildroot, ["rpmspec", "-P", spec]
            )
            if status:
                raise RuntimeError("cannot expand RPM spec for LLVM IR ownership")
            is_cmake = bool(re.search(r"^BuildSystem:\s*cmake\s*$", expanded, re.IGNORECASE | re.MULTILINE))
            if is_cmake:
                cmake_environment.update(_enable_cmake_build_environment(buildroot))
            install_buildroot_helper(
                buildroot, owner_rule_config(spec_path, expanded),
                conf.get("package_suffix", "-llvmir"), is_cmake,
            )
            buildroot.build_log.info("Configured LLVM IR install-post hook without modifying spec")
        except Exception:
            restore_build_configuration()
            raise

    def postbuild_hook():
        try:
            postbuild(buildroot, conf, logger)
        finally:
            restore_build_configuration()

    if conf.get("enable", False):
        plugins.add_hook("prebuild", prebuild_hook)
        plugins.add_hook("process_logs", restore_build_configuration)
        plugins.add_hook("postbuild", postbuild_hook)
