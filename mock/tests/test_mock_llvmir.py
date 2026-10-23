import importlib.util
import json
import os
import shlex
import sys
import shutil
import subprocess
import tempfile
import types
import unittest
from contextlib import contextmanager
from pathlib import Path
from unittest import mock


PLUGIN_PATH = Path(__file__).resolve().parents[1] / "mock-llvmir.py"
spec = importlib.util.spec_from_file_location("mock_llvmir", PLUGIN_PATH)
plugin = importlib.util.module_from_spec(spec)
spec.loader.exec_module(plugin)

BRP_HELPER_PATH = Path(__file__).resolve().parents[1] / "brp-llvmir.py"
brp_spec = importlib.util.spec_from_file_location("brp_llvmir", BRP_HELPER_PATH)
brp_helper = importlib.util.module_from_spec(brp_spec)
brp_spec.loader.exec_module(brp_helper)


class MockLLVMIRTests(unittest.TestCase):
    @staticmethod
    def uid_manager(events=None):
        @contextmanager
        def elevated_privileges():
            if events is not None:
                events.append("enter")
            try:
                yield
            finally:
                if events is not None:
                    events.append("exit")

        return types.SimpleNamespace(elevated_privileges=elevated_privileges)

    def hook_build_command(self, topdir, spec_path, extra=()):
        original = spec_path.read_bytes()
        rules = topdir / "llvmir-rules.json"
        rules.write_text(json.dumps({"rules": plugin.owner_rule_config(spec_path),
                                     "package_suffix": "-llvmir"}))
        post = subprocess.run(["rpm", "--eval", "%{macrobody:__arch_install_post}"], check=True, capture_output=True, text=True).stdout.rstrip()
        hook = post + "\n" + shlex.join([sys.executable, str(plugin.BRP_HELPER_SOURCE), str(rules)])
        self.assertEqual(spec_path.read_bytes(), original)
        macrohome = topdir / "macrohome"
        macrohome.mkdir()
        (macrohome / ".rpmmacros").write_text("%__arch_install_post " + hook.replace("\n", chr(92) + "\n") + "\n")
        return ["env", "RPM_CONFIG_HOME=" + str(macrohome), "HOME=" + str(macrohome),
                "rpmbuild", "-bb", "--nodeps", "--define", "_topdir %s" % topdir,
                *extra, str(spec_path)]

    def test_install_hook_preserves_existing_macro_and_cmake_arguments(self):
        with tempfile.TemporaryDirectory() as temp:
            rootpath = Path(temp)
            macrofile = rootpath / "builddir/.rpmmacros"
            macrofile.parent.mkdir()
            macrofile.write_text("%toolchain clang\n")
            privilege_events = []
            root = types.SimpleNamespace(
                homedir="/builddir",
                make_chroot_path=lambda *parts: str(rootpath.joinpath(*(part.lstrip("/") for part in parts))),
                doChrootPlugin=mock.Mock(return_value=("echo original-arch-post\n", 0)),
                uid_manager=self.uid_manager(privilege_events),
            )
            plugin.install_buildroot_helper(root, [], "-llvmir", True)
            self.assertEqual(privilege_events, ["enter", "exit"])
            self.assertTrue(Path(root.make_chroot_path(plugin.BRP_HELPER_PATH)).is_file())
            self.assertNotIn("%__os_install_post", macrofile.read_text())
            self.assertNotIn("%__meson", macrofile.read_text())
            self.assertIn("original-arch-post", macrofile.read_text())
            self.assertIn("%buildsystem_cmake_conf()", macrofile.read_text())
            self.assertIn("-DCMAKE_PROJECT_INCLUDE=", macrofile.read_text())

    def test_prebuild_restores_configuration_and_never_writes_spec(self):
        with tempfile.TemporaryDirectory() as temp:
            rootpath = Path(temp)
            specpath = rootpath / "builddir/build/SPECS/app.spec"
            specpath.parent.mkdir(parents=True)
            source = "Name: app\nBuildSystem: cmake\n%files\n/usr/bin/app\n"
            specpath.write_text(source)
            macrofile = rootpath / "builddir/.rpmmacros"
            macrofile.write_text("%toolchain clang\n")
            root = types.SimpleNamespace(
                homedir="/builddir", spec="/builddir/build/SPECS/app.spec",
                env={"PATH": "/var/lib/clang-wrap:/usr/bin"},
                make_chroot_path=lambda *parts: str(rootpath.joinpath(*(part.lstrip("/") for part in parts))),
                doChrootPlugin=mock.Mock(side_effect=[(source, 0), ("%{nil}", 0)]),
                root_log=mock.Mock(), build_log=mock.Mock(),
                uid_manager=self.uid_manager(),
            )
            hooks = {}
            plugins = mock.Mock()
            plugins.add_hook.side_effect = lambda name, fn: hooks.update({name: fn})
            plugin.init(plugins, {"enable": True}, root)
            hooks["prebuild"]()
            self.assertEqual(specpath.read_text(), source)
            self.assertIn("%__arch_install_post", macrofile.read_text())
            hooks["process_logs"]()
            self.assertEqual(macrofile.read_text(), "%toolchain clang\n")
            self.assertEqual(root.env, {"PATH": "/var/lib/clang-wrap:/usr/bin"})

    def test_prebuild_finds_spec_when_mock_does_not_expose_spec_attribute(self):
        with tempfile.TemporaryDirectory() as temp:
            rootpath = Path(temp)
            specpath = rootpath / "builddir/build/SPECS/app.spec"
            specpath.parent.mkdir(parents=True)
            source = "Name: app\nBuildSystem: cmake\n%files\n/usr/bin/app\n"
            specpath.write_text(source)
            macrofile = rootpath / "builddir/.rpmmacros"
            macrofile.write_text("%toolchain clang\n")
            root = types.SimpleNamespace(
                homedir="/builddir", builddir="/builddir/build",
                env={"PATH": "/var/lib/clang-wrap:/usr/bin"},
                make_chroot_path=lambda *parts: str(rootpath.joinpath(*(part.lstrip("/") for part in parts))),
                doChrootPlugin=mock.Mock(return_value=(source, 0)),
                root_log=mock.Mock(), build_log=mock.Mock(),
                uid_manager=self.uid_manager(),
            )
            hooks = {}
            plugins = mock.Mock()
            plugins.add_hook.side_effect = lambda name, fn: hooks.update({name: fn})
            plugin.init(plugins, {"enable": True}, root)

            hooks["prebuild"]()

            root.doChrootPlugin.assert_any_call(
                ["rpmspec", "-P", "/builddir/build/SPECS/app.spec"], returnOutput=True,
            )
            self.assertTrue((rootpath / "usr/lib/rpm/openruyi/brp-llvmir.py").is_file())

    def test_chroot_plugin_supports_worker_without_output_keyword(self):
        calls = []

        class Buildroot:
            def doChrootPlugin(self, command):
                raise AssertionError("the plugin API fallback must call doChroot")

            def doChroot(self, command, **kwargs):
                calls.append((command, kwargs))
                return "expanded", 0

            build_log = object()
            chrootuid = 50000
            chrootgid = 50000
            chrootuser = "mockbuild"
            config = {}

        self.assertEqual(
            plugin._do_chroot_plugin(Buildroot(), ["rpmspec", "-P", "app.spec"]),
            ("expanded", 0),
        )
        self.assertEqual(calls[0][0], ["rpmspec", "-P", "app.spec"])
        self.assertTrue(calls[0][1]["returnOutput"])

    def test_cmake_detection_preserves_spec(self):
        with tempfile.TemporaryDirectory() as temp:
            spec_path = Path(temp) / "mariadb.spec"
            spec_path.write_text("""Name: mariadb
BuildSystem: cmake
BuildOption(conf): -DWITH_SSL=system
BuildRequires: cmake

%files
/usr/bin/mariadb
""")

            self.assertTrue(plugin.configure_cmake_llvmir(spec_path))
            result = spec_path.read_text()

            self.assertEqual(result, """Name: mariadb
BuildSystem: cmake
BuildOption(conf): -DWITH_SSL=system
BuildRequires: cmake

%files
/usr/bin/mariadb
""")
            self.assertTrue(plugin.configure_cmake_llvmir(spec_path))

    def test_non_cmake_spec_is_unchanged(self):
        with tempfile.TemporaryDirectory() as temp:
            spec_path = Path(temp) / "example.spec"
            original = """Name: example
BuildSystem: meson

%build
%meson
"""
            spec_path.write_text(original)

            self.assertFalse(plugin.configure_cmake_llvmir(spec_path))
            self.assertEqual(spec_path.read_text(), original)

    def test_declarative_cmake_spec_preserves_automatic_stages(self):
        with tempfile.TemporaryDirectory() as temp:
            spec_path = Path(temp) / "nghttp3.spec"
            spec_path.write_text("""Name: nghttp3
BuildSystem: cmake
BuildOption(conf): -DENABLE_STATIC_LIB:BOOL=OFF
%files
/usr/lib64/libnghttp3.so.9
""")

            self.assertTrue(plugin.configure_cmake_llvmir(spec_path))
            result = spec_path.read_text()
            self.assertNotIn(plugin.SPEC_MARKER, result)
            self.assertNotIn("%install", result)
            self.assertEqual(plugin.owner_rule_config(spec_path), [
                {"package": "%{name}", "path": "/usr/lib64/libnghttp3.so.9", "manifest": False},
            ])

    def test_cmake_detection_without_build_options_preserves_spec(self):
        with tempfile.TemporaryDirectory() as temp:
            spec_path = Path(temp) / "libwebp.spec"
            spec_path.write_text("""Name: libwebp
BuildSystem: cmake
BuildRequires: cmake
%files
/usr/lib64/libwebp.so.7
""")

            self.assertTrue(plugin.configure_cmake_llvmir(spec_path))
            result = spec_path.read_text()
            self.assertEqual(result, """Name: libwebp
BuildSystem: cmake
BuildRequires: cmake
%files
/usr/lib64/libwebp.so.7
""")

    def test_cmake_build_environment_is_scoped_and_restored(self):
        root = types.SimpleNamespace(env={
            "PATH": "/var/lib/clang-wrap:/usr/bin:/bin",
            "EMIT_LLVMIR": "1",
            "CMAKE_EMIT_LLVMIR": "0",
        })
        previous = plugin._enable_cmake_build_environment(root)
        self.assertEqual(root.env["PATH"], "/usr/bin:/bin")
        self.assertEqual(root.env["CMAKE_EMIT_LLVMIR"], "1")
        self.assertEqual(root.env["EMIT_LLVMIR"], "1")

        plugin._restore_build_environment(root, previous)
        self.assertEqual(root.env["PATH"], "/var/lib/clang-wrap:/usr/bin:/bin")
        self.assertEqual(root.env["CMAKE_EMIT_LLVMIR"], "0")

    def test_find_meson_manifests_requires_a_build_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            builddir = root / "build"
            (builddir / "meson-info").mkdir(parents=True)
            (builddir / "meson-private").mkdir()
            manifest = builddir / "meson-info" / "intro-installed.json"
            manifest.write_text("{}")
            (builddir / "meson-private" / "build.dat").write_text("")

            source = root / "source"
            (source / "meson-info").mkdir(parents=True)
            (source / "meson-info" / "intro-installed.json").write_text("{}")

            previous = os.getcwd()
            os.chdir(root)
            self.addCleanup(os.chdir, previous)
            with mock.patch.dict(os.environ, {"RPM_BUILD_DIR": str(root)}):
                self.assertEqual(brp_helper.find_meson_manifests(), [manifest])

    def test_sync_meson_llvmir_invokes_install_helper(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest = root / "build" / "meson-info" / "intro-installed.json"
            image = root / "image"
            (image / "usr/bin").mkdir(parents=True)
            (image / "usr/bin" / "app").write_text("")
            manifest.parent.mkdir(parents=True)
            manifest.write_text(json.dumps({
                "build/app": "/usr/bin/app",
                "build/skipped": "/usr/bin/skipped",
            }))

            def check_manifest(args):
                self.assertEqual(args[0], "/helpers/meson-install")
                self.assertEqual(json.loads(Path(args[1]).read_text()), {
                    "build/app": "/usr/bin/app",
                })
                self.assertEqual(args[2], str(image))
                return types.SimpleNamespace(returncode=0)

            with mock.patch.object(brp_helper, "find_meson_manifests",
                                   return_value=[manifest]), \
                 mock.patch.object(brp_helper, "find_meson_install",
                                   return_value="/helpers/meson-install"), \
                 mock.patch.dict(os.environ, {"RPM_BUILD_ROOT": str(image)}), \
                 mock.patch.object(brp_helper.subprocess, "run",
                                   side_effect=check_manifest) as run, \
                 mock.patch("builtins.print"):
                brp_helper.sync_meson_llvmir()
            run.assert_called_once()

    def test_publishable_paths(self):
        for path in ("/usr/lib/llvmir/libfoo.so", "/usr/lib/llvmir-bin/foo", "/usr/lib/llvmir-sbin/foo", "/usr/lib/llvmir-libexec/foo"):
            self.assertTrue(plugin.is_publishable_path(path))
        for path in ("/usr/lib/libfoo.so", "/usr/lib/llvmir/foo_log", "/usr/lib/llvmir/foo_missing"):
            self.assertFalse(plugin.is_publishable_path(path))

    def test_expand_rpm_brace_patterns(self):
        self.assertEqual(
            brp_helper.expand_braces("/usr/bin/ip{,6}tables{,-save,-restore}"),
            [
                "/usr/bin/iptables",
                "/usr/bin/iptables-save",
                "/usr/bin/iptables-restore",
                "/usr/bin/ip6tables",
                "/usr/bin/ip6tables-save",
                "/usr/bin/ip6tables-restore",
            ],
        )
        self.assertEqual(
            brp_helper.expand_braces("/usr/lib/lib{ip{4,6},xt}tc.so.*"),
            ["/usr/lib/libip4tc.so.*", "/usr/lib/libip6tc.so.*", "/usr/lib/libxttc.so.*"],
        )

    def test_owner_rule_config_preserves_package_ownership(self):
        with tempfile.TemporaryDirectory() as temp:
            spec_path = Path(temp) / "example.spec"
            spec_path.write_text("""Name: example
Version: 1
Release: 2
Summary: fixture
License: MIT

%description
fixture

%install
install -D app %{buildroot}/usr/bin/app

%files
%{_bindir}/app

%changelog
* Thu Jan 01 2026 Test <test@example.invalid> - 1-2
""")
            original = spec_path.read_text()
            self.assertEqual(plugin.owner_rule_config(spec_path), [
                {"package": "%{name}", "path": "%{_bindir}/app", "manifest": False},
            ])
            self.assertEqual(spec_path.read_text(), original)

    def test_files_parser_tracks_main_and_subpackage_rules(self):
        rules = plugin._files_section_rules("""Name: fixture
%description
fixture
%files
%license LICENSE
%{_bindir}/fixture
%files libs
%{_libdir}/libfixture.so.*
%config(noreplace) %{_sysconfdir}/fixture.conf
%changelog
""")
        self.assertIn(("%{name}", "%{_bindir}/fixture"), rules)
        self.assertIn(("%{name}-libs", "%{_libdir}/libfixture.so.*"), rules)
        self.assertIn(("%{name}-libs", "%{_sysconfdir}/fixture.conf"), rules)

    def test_wildcard_and_specific_rules_are_ambiguous(self):
        with self.assertRaisesRegex(ValueError, "ambiguous RPM package ownership"):
            plugin.infer_package_owner(
                ["/usr/bin/fixture"],
                [("%{name}", "/usr/bin/*"), ("%{name}-tools", "/usr/bin/fixture")],
            )

    def test_equal_specificity_owner_collision_fails(self):
        with self.assertRaisesRegex(ValueError, "ambiguous RPM package ownership"):
            plugin.infer_package_owner(
                ["/usr/bin/fixture"],
                [("%{name}", "/usr/bin/fixture"), ("%{name}-tools", "/usr/bin/fixture")],
            )

    def test_relative_files_manifest_is_parsed_as_an_owner_rule(self):
        rules = plugin._files_section_rules("""Name: fixture
%description
fixture
%files -n %{name}-tools -f tools.list
%changelog
""")
        self.assertIn(("%{name}-tools", "@manifest@tools.list"), rules)

    def test_named_files_manifest_is_parsed_as_an_owner_rule(self):
        rules = plugin._files_section_rules("""Name: fixture
%description
fixture
%files -n %{name}-tools -f fixture.list
%changelog
""")
        self.assertIn(("%{name}-tools", "@manifest@fixture.list"), rules)

    @unittest.skipUnless(shutil.which("rpmbuild"), "rpmbuild is required")
    def test_rpmbuild_maps_executable_and_library_ir_to_owning_subpackages(self):
        with tempfile.TemporaryDirectory() as temp:
            topdir = Path(temp) / "rpmbuild"
            for directory in ("BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"):
                (topdir / directory).mkdir(parents=True)
            spec_path = topdir / "SPECS" / "fixture.spec"
            spec_path.write_text("""Name: fixture
Version: 1
Release: 1
Summary: Multi-output specparts fixture
License: MIT
BuildRequires: gcc

%description
fixture

%package libs
Summary: fixture shared library

%description libs
fixture library

%install
mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/lib64
printf executable > %{buildroot}/usr/bin/fixture
printf library > %{buildroot}/usr/lib64/libfixture.so.1
mkdir -p %{buildroot}/usr/lib/llvmir-bin %{buildroot}/usr/lib/llvmir
printf executable-ir > %{buildroot}/usr/lib/llvmir-bin/fixture
printf library-ir > %{buildroot}/usr/lib/llvmir/libfixture.so.1

%files
/usr/bin/fixture

%files libs
/usr/lib64/libfixture.so.*

%changelog
* Thu Jan 01 2026 Test <test@example.invalid> - 1-1
Initial fixture
""")
            original_spec = spec_path.read_bytes()
            build = subprocess.run(
                self.hook_build_command(topdir, spec_path),
                capture_output=True, text=True, timeout=60,
            )
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            self.assertEqual(spec_path.read_bytes(), original_spec)
            rpms = list((topdir / "RPMS").rglob("*.rpm"))
            by_name = {plugin.parse_nevra(rpm)[0]: rpm for rpm in rpms}
            self.assertIn("fixture-llvmir", by_name)
            self.assertIn("fixture-libs-llvmir", by_name)
            self.assertIn("/usr/lib/llvmir-bin/fixture", plugin.package_filelist(by_name["fixture-llvmir"]))
            self.assertIn("/usr/lib/llvmir/libfixture.so.1", plugin.package_filelist(by_name["fixture-libs-llvmir"]))
            fixture_requires = subprocess.run(
                ["rpm", "-qp", "--requires", str(by_name["fixture-llvmir"])],
                check=True, capture_output=True, text=True,
            ).stdout.splitlines()
            libs_requires = subprocess.run(
                ["rpm", "-qp", "--requires", str(by_name["fixture-libs-llvmir"])],
                check=True, capture_output=True, text=True,
            ).stdout.splitlines()
            self.assertIn("fixture = 1-1", fixture_requires)
            self.assertIn("fixture-libs = 1-1", libs_requires)
            fixture_supplements = subprocess.run(
                ["rpm", "-qp", "--supplements", str(by_name["fixture-llvmir"])],
                check=True, capture_output=True, text=True,
            ).stdout.splitlines()
            libs_supplements = subprocess.run(
                ["rpm", "-qp", "--supplements", str(by_name["fixture-libs-llvmir"])],
                check=True, capture_output=True, text=True,
            ).stdout.splitlines()
            self.assertIn("(fixture and llvmir-converter)", fixture_supplements)
            self.assertIn("(fixture-libs and llvmir-converter)", libs_supplements)

    @unittest.skipUnless(shutil.which("rpmbuild"), "rpmbuild is required")
    def test_rpmbuild_fails_for_ambiguous_ir_owner(self):
        with tempfile.TemporaryDirectory() as temp:
            topdir = Path(temp) / "rpmbuild"
            for directory in ("BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"):
                (topdir / directory).mkdir(parents=True)
            spec_path = topdir / "SPECS" / "ambiguous.spec"
            spec_path.write_text("""Name: ambiguous
Version: 1
Release: 1
Summary: ambiguous owner fixture
License: MIT

%description
fixture

%package tools
Summary: tools

%description tools
tools

%install
mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/lib/llvmir-bin
printf app > %{buildroot}/usr/bin/app
printf ir > %{buildroot}/usr/lib/llvmir-bin/app

%files
/usr/bin/*

%files tools
/usr/bin/app

%changelog
* Thu Jan 01 2026 Test <test@example.invalid> - 1-1
Initial fixture
""")
            build = subprocess.run(
                self.hook_build_command(topdir, spec_path),
                capture_output=True, text=True, timeout=60,
            )
            self.assertNotEqual(build.returncode, 0)
            self.assertIn("ambiguous %files owner", build.stdout + build.stderr)

    @unittest.skipUnless(shutil.which("rpmbuild"), "rpmbuild is required")
    def test_rpmbuild_only_creates_subpackage_when_ir_exists(self):
        for include_ir in (False, True):
            with self.subTest(include_ir=include_ir), tempfile.TemporaryDirectory() as temp:
                topdir = Path(temp) / "rpmbuild"
                for directory in ("BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"):
                    (topdir / directory).mkdir(parents=True)
                spec_path = topdir / "SPECS" / "fixture.spec"
                spec_path.write_text("""Name: fixture
Version: 1
Release: 1
Summary: RPM specparts fixture
License: MIT
BuildArch: noarch

%description
fixture

%install
mkdir -p %{buildroot}/usr/share/fixture
printf main > %{buildroot}/usr/share/fixture/main
""" + ("mkdir -p %{buildroot}/usr/bin %{buildroot}/usr/lib/llvmir-bin\nprintf native > %{buildroot}/usr/bin/fixture\nprintf bitcode > %{buildroot}/usr/lib/llvmir-bin/fixture\n" if include_ir else "") + """

%files
/usr/share/fixture/main
""" + ("/usr/bin/fixture\n" if include_ir else "") + """

%changelog
* Thu Jan 01 2026 Test <test@example.invalid> - 1-1
Initial fixture
""")
                build = subprocess.run(
                    self.hook_build_command(topdir, spec_path, extra=("--nocheck",)),
                    capture_output=True, text=True, timeout=60,
                )
                self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
                rpms = list((topdir / "RPMS").rglob("*.rpm"))
                names = [plugin.parse_nevra(rpm)[0] for rpm in rpms]
                self.assertEqual("fixture-llvmir" in names, include_ir)
                if include_ir:
                    ir_rpm = next(rpm for rpm in rpms if plugin.parse_nevra(rpm)[0] == "fixture-llvmir")
                    self.assertIn("/usr/lib/llvmir-bin/fixture", plugin.package_filelist(ir_rpm))
                self.assertNotIn("Executing(%check)", build.stdout + build.stderr)

    @mock.patch.object(plugin, "package_filelist", return_value=[
        "/usr/lib/llvmir/a.bc", "/usr/lib/llvmir/a_cmd", "/usr/lib/llvmir/a_verscript",
        "/usr/lib/llvmir/a_log", "/usr/lib/llvmir/a_missing", "/usr/lib/llvmir/asm/hash-obj",
        "/usr/bin/a",
    ])
    def test_selected_files(self, _):
        self.assertEqual(plugin.selected_files("pkg.rpm"), [
            "/usr/lib/llvmir/a.bc", "/usr/lib/llvmir/a_cmd", "/usr/lib/llvmir/a_verscript",
            "/usr/lib/llvmir/asm/hash-obj",
        ])

    def test_spec_and_nevra_inheritance(self):
        with tempfile.TemporaryDirectory() as temp:
            rpm = Path(temp) / "app.rpm"
            rpm.touch()
            with mock.patch.object(plugin, "parse_nevra", return_value=("app", "1.2", "7", "x86_64")), \
                 mock.patch.object(plugin, "selected_files", return_value=["/usr/lib/llvmir/app"]), \
                 mock.patch.object(plugin.subprocess, "Popen") as popen, \
                 mock.patch.object(plugin.subprocess, "run") as run:
                proc = mock.Mock()
                proc.stdout = mock.Mock()
                proc.wait.return_value = 0
                popen.return_value = proc
                generated_specs = []

                def run_side_effect(args, **kwargs):
                    if args[0] == "cpio":
                        extract_dir = Path(args[args.index("-D") + 1])
                        artifact = extract_dir / "usr/lib/llvmir/app"
                        artifact.parent.mkdir(parents=True)
                        artifact.write_text("bitcode")
                    if args[0] == "rpmbuild":
                        generated_specs.append(Path(args[-1]).read_text())
                        topdir = Path(args[args.index("--define") + 1].split(" ", 1)[1])
                        out = topdir / "RPMS" / "x86_64"
                        out.mkdir(parents=True)
                        (out / "app-llvmir-1.2-7.x86_64.rpm").touch()
                    return mock.Mock(returncode=0, stderr="")
                run.side_effect = run_side_effect
                # Extraction creates an empty tree; install the fixture source expected by the plugin.
                with mock.patch.object(plugin, "_check_content_for_path_leaks"):
                    result = plugin.build_llvmir_rpm(rpm, temp)
                self.assertEqual(result.name, "app-llvmir-1.2-7.x86_64.rpm")
                self.assertIn("Requires: app = 1.2-7", generated_specs[0])
                self.assertIn("Supplements: (app and llvmir-converter)", generated_specs[0])

    def test_postbuild_filters_and_hook(self):
        plugins = mock.Mock()
        conf = {"enable": True, "package_suffix": "-llvmir"}
        plugin.init(plugins, conf, mock.Mock())
        hook = plugins.add_hook.call_args.args[1]
        hook_owner = types.SimpleNamespace(root_log=mock.Mock())
        root = types.SimpleNamespace(final_rpm_list=["src.src.rpm", "already-llvmir.x86_64.rpm", "app.x86_64.rpm"], resultdir="/tmp/results")
        with mock.patch.object(plugin, "parse_nevra", side_effect=[("app", "1", "1", "x86_64")]), \
             mock.patch.object(plugin, "package_names", return_value=["app"]), \
             mock.patch.object(plugin, "build_llvmir_rpm") as build:
            plugin.postbuild(root, conf, hook_owner.root_log)
        build.assert_called_once()
        self.assertEqual(build.call_args.args[0], Path("/tmp/results/app.x86_64.rpm"))


if __name__ == "__main__":
    unittest.main()