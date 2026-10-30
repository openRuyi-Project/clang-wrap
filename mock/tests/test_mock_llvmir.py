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

    def test_files_manifest_argument_is_not_treated_as_subpackage_name(self):
        rules = plugin._files_section_rules("""Name: gnupg
%files -f gnupg2.lang
%{_bindir}/gpg
%files tools -f tools.lang
%{_bindir}/gpgconf
""")
        self.assertIn(("%{name}", "@manifest@gnupg2.lang"), rules)
        self.assertIn(("%{name}", "%{_bindir}/gpg"), rules)
        self.assertIn(("%{name}-tools", "@manifest@tools.lang"), rules)
        self.assertIn(("%{name}-tools", "%{_bindir}/gpgconf"), rules)

    def test_owner_rules_preserve_package_exclusions(self):
        rules = plugin.owner_rule_config(Path("unused.spec"), expanded_text="""Name: fixture
%files client
/usr/bin/virt-admin
%files others
/usr/bin/virt*
%exclude /usr/bin/virt-admin
""")
        self.assertIn({"package": "%{name}-others", "path": "/usr/bin/virt-admin",
                       "manifest": False, "exclude": True}, rules)

    def test_file_rule_globs_respect_directory_boundaries(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "usr/lib64/libvirt/connection-driver").mkdir(parents=True)
            candidate = "/usr/lib64/libvirt/connection-driver/libvirt_driver_qemu.so"
            self.assertFalse(brp_helper.matches_file_rule(
                candidate, "/usr/lib64/libvirt*.so", root))
            self.assertTrue(brp_helper.matches_file_rule(
                candidate, "/usr/lib64/libvirt/connection-driver/", root))
            self.assertTrue(brp_helper.matches_file_rule(
                candidate, "/usr/lib64/libvirt/*", root))

    def test_owner_inference_preserves_multiple_rpm_owners(self):
        rules = [("%{name}", "/usr/bin/*"), ("%{name}-tools", "/usr/bin/fixture")]
        for ordered in (rules, list(reversed(rules))):
            self.assertEqual(plugin.infer_package_owner(
                ["/usr/bin/fixture"],
                ordered,
            ), ["%{name}", "%{name}-tools"])

    def test_equal_specificity_owner_collision_is_preserved(self):
        self.assertEqual(plugin.infer_package_owner(
            ["/usr/bin/fixture"],
            [("%{name}", "/usr/bin/fixture"), ("%{name}-tools", "/usr/bin/fixture")],
        ), ["%{name}", "%{name}-tools"])

    def test_brp_owner_sharing_preserves_exclusions(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            buildroot = root / "buildroot"
            specparts = root / "specparts"
            specparts.mkdir()
            for name in ("usr/bin/virt-admin", "usr/lib/llvmir-bin/virt-admin"):
                path = buildroot / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("fixture")
            config = root / "rules.json"
            exact = {"package": "client", "path": "/usr/bin/virt-admin", "manifest": False}
            wildcard = {"package": "others", "path": "/usr/bin/virt*", "manifest": False}
            cases = [
                ([exact, wildcard], ["client", "others"], None),
                ([wildcard, exact], ["client", "others"], None),
                ([wildcard, exact, dict(exact, exclude=True)], ["others"], None),
            ]
            with mock.patch.dict(os.environ, {
                "RPM_BUILD_ROOT": str(buildroot), "RPM_SPECPARTS_DIR": str(specparts),
                "RPM_BUILD_DIR": str(root), "RPM_PACKAGE_NAME": "fixture",
            }), mock.patch.object(sys, "argv", [str(BRP_HELPER_PATH), str(config)]), \
                    mock.patch.object(brp_helper, "sync_meson_llvmir"):
                for rules, owner, error in cases:
                    with self.subTest(rules=rules):
                        config.write_text(json.dumps({"rules": rules, "package_suffix": "-llvmir"}))
                        if error:
                            with self.assertRaisesRegex(RuntimeError, error):
                                brp_helper.main()
                        else:
                            brp_helper.main()
                            specpart = (specparts / "llvmir.specpart").read_text()
                            for package in owner:
                                self.assertIn("%package -n " + package + "-llvmir", specpart)
                            self.assertEqual(specpart.count("%package -n "), len(owner))

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
printf '{ exported; };' > %{buildroot}/usr/lib/llvmir-bin/fixture_dynamiclist
printf library-ir > %{buildroot}/usr/lib/llvmir/libfixture.so.1
printf '{ exported; };' > %{buildroot}/usr/lib/llvmir/libfixture.so.1_dynamiclist
mkdir -p %{buildroot}/usr/lib/llvmir/asm
printf assembly > %{buildroot}/usr/lib/llvmir/asm/hash-trampoline.S.o
printf "cmd=( './asm/hash-trampoline.S.o' './libfixture.so.1' )\n" > %{buildroot}/usr/lib/llvmir/libfixture.so.1_cmd
mkdir -p %{buildroot}/usr/lib64/fixture/llvmir
printf plugin > %{buildroot}/usr/lib64/fixture/plugin.so
printf plugin-ir > %{buildroot}/usr/lib64/fixture/llvmir/plugin.so

%files
/usr/bin/fixture
/usr/lib64/fixture/
%exclude /usr/lib64/fixture/plugin.so
%exclude /usr/lib64/fixture/llvmir

%files libs
/usr/lib64/libfixture.so.*
/usr/lib64/fixture/
%exclude /usr/lib64/fixture/llvmir

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
            self.assertEqual(plugin.package_filelist(by_name["fixture-llvmir"]), [
                "/usr/lib/llvmir-bin/fixture",
                "/usr/lib/llvmir-bin/fixture_dynamiclist",
            ])
            self.assertEqual(plugin.package_filelist(by_name["fixture-libs-llvmir"]), [
                "/usr/lib/llvmir/asm/hash-trampoline.S.o",
                "/usr/lib/llvmir/libfixture.so.1",
                "/usr/lib/llvmir/libfixture.so.1_cmd",
                "/usr/lib/llvmir/libfixture.so.1_dynamiclist",
                "/usr/lib64/fixture/llvmir/plugin.so",
            ])
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
    def test_rpmbuild_removes_ir_for_deleted_native_files(self):
        with tempfile.TemporaryDirectory() as temp:
            topdir = Path(temp) / "rpmbuild"
            for directory in ("BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"):
                (topdir / directory).mkdir(parents=True)
            spec_path = topdir / "SPECS" / "fixture.spec"
            spec_path.write_text("""Name: fixture
Version: 1
Release: 1
Summary: Removed native files fixture
License: MIT

%description
fixture

%install
mkdir -p %{buildroot}/usr/lib64/llvmir
printf native > %{buildroot}/usr/lib64/libkeep.a
for name in libkeep.a libremoved.a libremoved.a.bc libremoved.a_cmd libremoved.a_log libremoved.a_missing; do
    printf ir > %{buildroot}/usr/lib64/llvmir/$name
done
printf native > %{buildroot}/usr/lib64/libremoved.a
rm %{buildroot}/usr/lib64/libremoved.a
for pair in 'bin tjbench' 'sbin removed-admin' 'libexec removed-helper'; do
    set -- $pair
    mkdir -p %{buildroot}/usr/$1 %{buildroot}/usr/lib/llvmir-$1/asm
    printf native > %{buildroot}/usr/$1/$2
    for suffix in '' .bc _cmd _verscript _dynamiclist .version.txt .soversion.txt _log _missing .bc_log .bc_missing _cmd_log; do
        printf ir > %{buildroot}/usr/lib/llvmir-$1/$2$suffix
    done
    printf './asm/removed.o' > %{buildroot}/usr/lib/llvmir-$1/$2_cmd
    printf assembly > %{buildroot}/usr/lib/llvmir-$1/asm/removed.o
    rm %{buildroot}/usr/$1/$2
done
for suffix in .bc _cmd _verscript _dynamiclist .version.txt .soversion.txt _log _missing; do
    printf ir > %{buildroot}/usr/lib64/llvmir/libremoved.so.1$suffix
done

%files
/usr/lib64/libkeep.a

%changelog
* Thu Jan 01 2026 Test <test@example.invalid> - 1-1
Initial fixture
""")
            build = subprocess.run(
                self.hook_build_command(topdir, spec_path),
                capture_output=True, text=True, timeout=60,
            )
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            ir_rpm = next(rpm for rpm in (topdir / "RPMS").rglob("*.rpm")
                          if plugin.parse_nevra(rpm)[0] == "fixture-llvmir")
            self.assertEqual(
                plugin.package_filelist(ir_rpm), ["/usr/lib64/llvmir/libkeep.a"],
            )

    def test_rpmbuild_omits_ir_for_excluded_static_library(self):
        with tempfile.TemporaryDirectory() as temp:
            topdir = Path(temp) / "rpmbuild"
            for directory in ("BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"):
                (topdir / directory).mkdir(parents=True)
            spec_path = topdir / "SPECS" / "fixture.spec"
            spec_path.write_text("""Name: fixture
Version: 1
Release: 1
Summary: Excluded static library fixture
License: MIT

%description
fixture

%install
mkdir -p %{buildroot}/usr/lib64/llvmir
printf native > %{buildroot}/usr/lib64/libfixture.a
printf ir > %{buildroot}/usr/lib64/llvmir/libfixture.a
printf native > %{buildroot}/usr/lib64/libfixture.so.1
printf ir > %{buildroot}/usr/lib64/llvmir/libfixture.so.1

%files
/usr/lib64/libfixture.so.1
%exclude /usr/lib64/libfixture.a

%changelog
* Thu Jan 01 2026 Test <test@example.invalid> - 1-1
Initial fixture
""")
            build = subprocess.run(
                self.hook_build_command(topdir, spec_path),
                capture_output=True, text=True, timeout=60,
            )
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            ir_rpm = next(rpm for rpm in (topdir / "RPMS").rglob("*.rpm")
                          if plugin.parse_nevra(rpm)[0] == "fixture-llvmir")
            self.assertEqual(plugin.package_filelist(ir_rpm),
                             ["/usr/lib64/llvmir/libfixture.so.1"])

    def test_brp_preserves_executable_installation_prefix(self):
        cases = [
            ("/usr/lib/llvmir-convert", "lib", "bin"),
            ("/usr/lib/llvmir-convert", "lib", "sbin"),
            ("/usr/lib/llvmir-convert", "lib", "libexec"),
            ("/usr/lib64/vendor", "lib64", "bin"),
            ("/usr", "lib64", "bin"),
            ("/usr", "lib", "bin"),
            ("/usr", "lib", "sbin"),
            ("/usr", "lib", "libexec"),
        ]
        for prefix, libdir, directory in cases:
            with self.subTest(prefix=prefix, libdir=libdir, directory=directory), \
                 tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                buildroot = root / "buildroot"
                specparts = root / "specparts"
                specparts.mkdir()
                native = prefix + "/" + directory + "/llvmir-converter-22"
                path = buildroot / native.lstrip("/")
                path.parent.mkdir(parents=True)
                path.write_text("native")
                ir_dir = prefix + "/" + libdir + "/llvmir-" + directory
                ir = buildroot / ir_dir.lstrip("/")
                ir.mkdir(parents=True)
                artifacts = ["llvmir-converter-22", "llvmir-converter-22.bc",
                             "llvmir-converter-22_cmd"]
                for name in artifacts:
                    (ir / name).write_text("bitcode")
                rules = [{"package": "fixture", "path": native, "manifest": False}]
                if prefix not in ("", "/usr"):
                    # A same-named system program must not claim private IR.
                    system = "/usr/" + directory + "/llvmir-converter-22"
                    path = buildroot / system.lstrip("/")
                    path.parent.mkdir(parents=True)
                    path.write_text("unrelated native")
                    rules.append({"package": "unrelated", "path": system,
                                  "manifest": False})
                config = root / "rules.json"
                config.write_text(json.dumps({"rules": rules, "package_suffix": "-llvmir"}))
                environment = {"RPM_BUILD_ROOT": str(buildroot),
                               "RPM_SPECPARTS_DIR": str(specparts),
                               "RPM_BUILD_DIR": str(root), "RPM_PACKAGE_NAME": "fixture"}
                with mock.patch.dict(os.environ, environment), \
                     mock.patch.object(sys, "argv", [str(BRP_HELPER_PATH), str(config)]), \
                     mock.patch.object(brp_helper, "sync_meson_llvmir"):
                    brp_helper.main()
                self.assertEqual(
                    (specparts / "llvmir-files-0.list").read_text().splitlines(),
                    sorted(ir_dir + "/" + name for name in artifacts),
                )
                self.assertFalse((specparts / "llvmir-files-1.list").exists())
                self.assertIn("%package -n fixture-llvmir",
                              (specparts / "llvmir.specpart").read_text())

    @unittest.skipUnless(shutil.which("rpmbuild"), "rpmbuild is required")
    def test_rpmbuild_maps_private_bin_ir_to_package(self):
        with tempfile.TemporaryDirectory() as temp:
            topdir = Path(temp) / "rpmbuild"
            for directory in ("BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"):
                (topdir / directory).mkdir(parents=True)
            spec_path = topdir / "SPECS" / "fixture.spec"
            spec_path.write_text("""Name: fixture
Version: 1
Release: 1
Summary: Private executable directory fixture
License: MIT

%description
fixture

%install
mkdir -p %{buildroot}/usr/lib/llvmir-convert/bin
mkdir -p %{buildroot}/usr/lib/llvmir-convert/lib/llvmir-bin
printf native > %{buildroot}/usr/lib/llvmir-convert/bin/llvmir-converter-22
printf bitcode > %{buildroot}/usr/lib/llvmir-convert/lib/llvmir-bin/llvmir-converter-22
printf command > %{buildroot}/usr/lib/llvmir-convert/lib/llvmir-bin/llvmir-converter-22_cmd

%files
/usr/lib/llvmir-convert/bin/llvmir-converter-*
""")
            build = subprocess.run(
                self.hook_build_command(topdir, spec_path),
                capture_output=True, text=True, timeout=60,
            )
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            ir_rpm = next(rpm for rpm in (topdir / "RPMS").rglob("*.rpm")
                          if plugin.parse_nevra(rpm)[0] == "fixture-llvmir")
            self.assertEqual(plugin.package_filelist(ir_rpm), [
                "/usr/lib/llvmir-convert/lib/llvmir-bin/llvmir-converter-22",
                "/usr/lib/llvmir-convert/lib/llvmir-bin/llvmir-converter-22_cmd",
            ])

    def test_assembly_inputs_follow_all_consuming_packages(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            buildroot = root / "buildroot"
            specparts = root / "specparts"
            specparts.mkdir()
            ir = buildroot / "usr/lib64/llvmir"
            (ir / "asm").mkdir(parents=True)
            (ir / "asm/shared-object.o").write_text("assembly")
            rules = []
            for name in ("first", "second"):
                (buildroot / ("usr/lib64/lib%s.so.1" % name)).write_text("native")
                (ir / ("lib%s.so.1.bc" % name)).write_text("bitcode")
                (ir / ("lib%s.so.1_cmd" % name)).write_text(
                    "# ./asm/comment-only.o\ncmd=(\n  './asm/shared-object.o'\n)\n"
                )
                rules.append({"package": name, "path": "/usr/lib64/lib%s.so.1" % name,
                              "manifest": False})
            config = root / "rules.json"
            config.write_text(json.dumps({"rules": rules, "package_suffix": "-llvmir"}))
            environment = {"RPM_BUILD_ROOT": str(buildroot),
                           "RPM_SPECPARTS_DIR": str(specparts),
                           "RPM_BUILD_DIR": str(root), "RPM_PACKAGE_NAME": "fixture"}
            with mock.patch.dict(os.environ, environment), \
                 mock.patch.object(sys, "argv", [str(BRP_HELPER_PATH), str(config)]), \
                 mock.patch.object(brp_helper, "sync_meson_llvmir"):
                brp_helper.main()
                for index in range(2):
                    self.assertIn("/usr/lib64/llvmir/asm/shared-object.o",
                                  (specparts / ("llvmir-files-%d.list" % index)).read_text())
                (ir / "asm/shared-object.o").unlink()
                with self.assertRaisesRegex(RuntimeError, "missing assembly input"):
                    brp_helper.main()
                (ir / "asm/shared-object.o").write_text("assembly")
                (ir / "asm/orphan.o").write_text("orphan")
                brp_helper.main()
                self.assertFalse((ir / "asm/orphan.o").exists())
                self.assertTrue((ir / "asm/shared-object.o").exists())

    @unittest.skipUnless(shutil.which("rpmbuild"), "rpmbuild is required")
    def test_rpmbuild_preserves_multiple_ir_owners(self):
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
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            by_name = {plugin.parse_nevra(rpm)[0]: rpm
                       for rpm in (topdir / "RPMS").rglob("*.rpm")}
            self.assertEqual(plugin.package_filelist(by_name["ambiguous-llvmir"]),
                             ["/usr/lib/llvmir-bin/app"])
            self.assertEqual(plugin.package_filelist(by_name["ambiguous-tools-llvmir"]),
                             ["/usr/lib/llvmir-bin/app"])

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