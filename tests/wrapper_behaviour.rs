use std::env;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use sha2::{Digest, Sha256};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after UNIX_EPOCH")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "clang-wrap-test-{}-{}-{}",
            std::process::id(),
            nanos,
            id
        ));
        fs::create_dir_all(&path).expect("failed to create temporary test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn bin(name: &str) -> &'static str {
    match name {
        "ar" => env!("CARGO_BIN_EXE_ar"),
        "clang" => env!("CARGO_BIN_EXE_clang"),
        "cp" => env!("CARGO_BIN_EXE_cp"),
        "install" => env!("CARGO_BIN_EXE_install"),
        "ln" => env!("CARGO_BIN_EXE_ln"),
        "meson-install" => env!("CARGO_BIN_EXE_meson-install"),
        "mv" => env!("CARGO_BIN_EXE_mv"),
        other => panic!("unknown test binary: {other}"),
    }
}

fn llvmir_path(llvmir_dir: &Path, original_path: &Path) -> PathBuf {
    llvmir_dir.join(
        original_path
            .strip_prefix("/")
            .expect("absolute path should have a root prefix"),
    )
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("failed to create parent directory");
    }
    fs::write(path, content).expect("failed to write test file");
}

fn asm_name_for(path: &Path) -> String {
    let hash = hex::encode(Sha256::digest(path.as_os_str().as_encoded_bytes()));
    format!(
        "{}-{}",
        &hash[..12],
        path.file_name()
            .and_then(|name| name.to_str())
            .expect("UTF-8 test path")
    )
}

fn run_wrapper(binary: &str, cwd: &Path, llvmir_dir: &Path, args: &[&str]) {
    let status = Command::new(bin(binary))
        .current_dir(cwd)
        .env("LLVM_IR_DIR", llvmir_dir)
        .args(args)
        .status()
        .expect("failed to execute wrapper binary");
    assert!(status.success(), "{binary} wrapper failed with {status}");
}

fn run_meson_install(cwd: &Path, llvmir_dir: &Path, manifest: &Path, destdir: &Path) {
    let status = Command::new(bin("meson-install"))
        .current_dir(cwd)
        .env("LLVM_IR_DIR", llvmir_dir)
        .arg(manifest)
        .arg(destdir)
        .status()
        .expect("failed to execute meson-install");
    assert!(status.success(), "meson-install failed with {status}");
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {command} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[test]
fn cp_mv_and_ln_sync_llvmir_into_destination_directory() {
    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");

    for (name, wrapper, dest_dir) in [
        ("copy.o", "cp", "copy-dst"),
        ("move.o", "mv", "move-dst"),
        ("link.o", "ln", "link-dst"),
    ] {
        let source = root.join(name);
        let destination = root.join(dest_dir);
        let ir_source = llvmir_path(&llvmir_dir, &source);

        fs::create_dir_all(&destination).expect("failed to create destination directory");
        write_file(&source, "object");
        write_file(&ir_source, "llvm-ir");

        run_wrapper(wrapper, root, &llvmir_dir, &[name, dest_dir]);

        assert!(
            llvmir_path(&llvmir_dir, &destination.join(name)).exists(),
            "{wrapper} should sync LLVM IR into destination directory"
        );
    }
}

#[test]
fn install_executable_to_bin_syncs_ir_into_lib_llvmir_bin() {
    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    let source = root.join("prog");
    let target = root.join("image/usr/bin/prog");

    write_file(&source, "program");
    write_file(
        &llvmir_path(&llvmir_dir, &source).with_extension("bc"),
        "bitcode",
    );
    fs::create_dir_all(target.parent().expect("target should have parent"))
        .expect("failed to create target parent");

    run_wrapper(
        "install",
        root,
        &llvmir_dir,
        &["-m", "755", "prog", "image/usr/bin/prog"],
    );

    assert!(root.join("image/usr/lib/llvmir-bin/prog.bc").exists());
    assert!(!root.join("image/usr/bin/llvmir/prog.bc").exists());
}

#[test]
fn install_sbin_program_without_execute_mode_syncs_ir_into_lib_llvmir_sbin() {
    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    let source = root.join("daemon");
    let target = root.join("image/usr/sbin/daemon");

    // Build systems can apply the executable mode after invoking install.
    // Treat sbin as an executable location even while this input is mode 0644.
    write_file(&source, "program");
    write_file(
        &llvmir_path(&llvmir_dir, &source).with_extension("bc"),
        "bitcode",
    );
    fs::create_dir_all(target.parent().expect("target should have parent"))
        .expect("failed to create target parent");

    run_wrapper(
        "install",
        root,
        &llvmir_dir,
        &["daemon", "image/usr/sbin/daemon"],
    );

    assert!(root.join("image/usr/lib/llvmir-sbin/daemon.bc").exists());
    assert!(!root.join("image/usr/sbin/llvmir/daemon.bc").exists());
}

#[test]
fn install_libexec_program_without_execute_mode_syncs_ir_into_lib_llvmir_libexec() {
    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    let source = root.join("helper");
    let target = root.join("image/usr/libexec/helper");

    write_file(&source, "program");
    write_file(
        &llvmir_path(&llvmir_dir, &source).with_extension("bc"),
        "bitcode",
    );
    fs::create_dir_all(target.parent().expect("target should have parent"))
        .expect("failed to create target parent");

    run_wrapper(
        "install",
        root,
        &llvmir_dir,
        &["helper", "image/usr/libexec/helper"],
    );

    assert!(root.join("image/usr/lib/llvmir-libexec/helper.bc").exists());
    assert!(!root.join("image/usr/libexec/llvmir/helper.bc").exists());
}

#[test]
fn multi_source_compile_generates_bitcode_for_each_object() {
    if !command_exists("clang") {
        eprintln!("skipping multi-source compile test: clang is unavailable");
        return;
    }

    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    write_file(&root.join("first.c"), "int first(void) { return 1; }\n");
    write_file(&root.join("second.c"), "int second(void) { return 2; }\n");

    let status = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["-c", "first.c", "second.c"])
        .status()
        .expect("failed to compile multiple sources with clang wrapper");
    assert!(status.success(), "clang wrapper failed with {status}");

    for object in [root.join("first.o"), root.join("second.o")] {
        assert!(
            object.is_file(),
            "normal compile did not create {}",
            object.display()
        );
        assert!(
            llvmir_path(&llvmir_dir, &object).is_file(),
            "LLVM IR was not generated for {}",
            object.display()
        );
    }
}

#[test]
fn install_static_library_embeds_missing_native_members_in_ir_archive() {
    if !command_exists("clang") || !command_exists("llvm-ar") {
        eprintln!("skipping installed static archive test: clang or llvm-ar is unavailable");
        return;
    }

    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    write_file(&root.join("part.c"), "int part(void) { return 1; }\n");
    write_file(
        &root.join("native.s"),
        ".text\n.globl native\nnative:\n  ret\n",
    );

    let compile_ir = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["part.c", "-c", "-o", "part.o"])
        .status()
        .expect("failed to compile bitcode-backed object");
    assert!(compile_ir.success());
    let compile_native = Command::new("clang")
        .current_dir(root)
        .args(["native.s", "-c", "-o", "native.o"])
        .status()
        .expect("failed to compile native object");
    assert!(compile_native.success());

    let archive = Command::new(bin("ar"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["csrD", "libpart.a", "part.o", "native.o"])
        .status()
        .expect("failed to create static archive");
    assert!(archive.success());

    fs::create_dir_all(root.join("image/usr/lib")).expect("failed to create install directory");
    run_wrapper(
        "install",
        root,
        &llvmir_dir,
        &["libpart.a", "image/usr/lib/libpart.a"],
    );

    let installed_ir = root.join("image/usr/lib/llvmir/libpart.a");
    let members = Command::new("llvm-ar")
        .args(["t", installed_ir.to_str().expect("UTF-8 test path")])
        .output()
        .expect("failed to list installed LLVM archive");
    assert!(members.status.success());
    let members = String::from_utf8_lossy(&members.stdout);
    assert!(members.lines().any(|member| member == "part.o"));
    assert!(members.lines().any(|member| member == "native.o"));
    assert!(
        !root.join("image/usr/lib/llvmir/libpart.a_missing").exists(),
        "embedded native archive members should not retain a sidecar"
    );
}

#[test]
fn install_nonstatic_ir_relocates_missing_objects_to_asm_directory() {
    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    let source = root.join("libdemo.so");
    let missing_object = root.join("build/native.o");
    let source_ir = llvmir_path(&llvmir_dir, &source);
    let source_cmd = PathBuf::from(format!("{}_cmd", source_ir.display()));

    write_file(&source, "shared-library-placeholder");
    write_file(&source_ir, "bitcode");
    write_file(&missing_object, "native-object");
    write_file(
        &PathBuf::from(format!("{}_missing", source_ir.display())),
        &format!("{}\n", missing_object.display()),
    );
    write_file(
        &source_cmd,
        &format!(
            "#!/bin/bash\ncmd=(\n  clang\n  ./libdemo.so\n  -Wl,{}\n  --output=output/libdemo.so\n)\n\"${{cmd[@]}}\"\n",
            missing_object.display()
        ),
    );
    fs::create_dir_all(root.join("image/usr/lib")).expect("failed to create install directory");

    run_wrapper(
        "install",
        root,
        &llvmir_dir,
        &["libdemo.so", "image/usr/lib/libdemo.so"],
    );

    let installed_dir = root.join("image/usr/lib/llvmir");
    assert!(!installed_dir.join("libdemo.so_missing").exists());
    let installed_cmd = fs::read_to_string(installed_dir.join("libdemo.so_cmd"))
        .expect("failed to read installed command");
    let asm_name = asm_name_for(&missing_object);
    assert!(installed_cmd.contains(&format!("./asm/{asm_name}")));
    assert!(!installed_cmd.contains(&missing_object.display().to_string()));
    assert!(installed_dir.join("asm").join(asm_name).is_file());
}

#[test]
fn install_shared_library_rewrites_installed_cmd_content() {
    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    let source = root.join("libdemo.so");
    let target = root.join("image/usr/lib/libdemo.so.1.2.3");
    let source_ir = llvmir_path(&llvmir_dir, &source);
    let source_cmd = PathBuf::from(format!("{}_cmd", source_ir.display()));

    write_file(&source, "shared-library-placeholder");
    write_file(&source_ir, "bitcode");
    write_file(
        &source_cmd,
        "#!/bin/bash\n# Original output: libdemo.so\ncmd=(\n  clang-21\n  -Wl,-soname,libdemo.so.1\n  ./libdemo.so\n  -Wl,--version-script=./libdemo.so_verscript\n  --output=output/libdemo.so\n)\n\"${cmd[@]}\"\n",
    );
    fs::create_dir_all(target.parent().expect("target should have parent"))
        .expect("failed to create target parent");

    run_wrapper(
        "install",
        root,
        &llvmir_dir,
        &["libdemo.so", "image/usr/lib/libdemo.so.1.2.3"],
    );

    let installed_cmd = root.join("image/usr/lib/llvmir/libdemo.so.1_cmd");
    let installed_ir = root.join("image/usr/lib/llvmir/libdemo.so.1");
    let script = fs::read_to_string(&installed_cmd).expect("failed to read installed _cmd");

    assert!(installed_ir.exists());
    assert!(!root.join("image/usr/lib/llvmir/libdemo.so.1.2.3").exists());
    assert!(script.contains("# Original output: libdemo.so.1"));
    assert!(script.contains("./libdemo.so.1"));
    assert!(script.contains("--output=output/libdemo.so.1"));
    assert!(script.contains("-Wl,--version-script=./libdemo.so.1_verscript"));
    assert!(!script.contains("# Original output: libdemo.so\n"));
    assert!(!script.contains("./libdemo.so\n"));
    assert!(!script.contains("output/libdemo.so\n"));
}

#[test]
fn install_shared_library_temp_t_uses_soname_for_installed_ir() {
    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    let source = root.join("libdemo.so.1.2.3T");
    let target = root.join("image/usr/lib/libdemo.so.1.2.3");
    let source_ir = llvmir_path(&llvmir_dir, &source);
    let source_cmd = source_ir
        .parent()
        .expect("source ir should have parent")
        .join("libdemo.so.1.2.3_cmd");

    write_file(&source, "shared-library-placeholder");
    write_file(&source_ir, "bitcode");
    write_file(
        &source_cmd,
        "#!/bin/bash\n# Original output: libdemo.so.1.2.3\ncmd=(\n  clang-21\n  -Wl,-soname,libdemo.so.1\n  ./libdemo.so.1.2.3\n  --output=output/libdemo.so.1.2.3\n)\n\"${cmd[@]}\"\n",
    );
    fs::create_dir_all(target.parent().expect("target should have parent"))
        .expect("failed to create target parent");

    run_wrapper(
        "install",
        root,
        &llvmir_dir,
        &["libdemo.so.1.2.3T", "image/usr/lib/libdemo.so.1.2.3"],
    );

    let installed_ir = root.join("image/usr/lib/llvmir/libdemo.so.1");
    let installed_cmd = root.join("image/usr/lib/llvmir/libdemo.so.1_cmd");
    let script = fs::read_to_string(&installed_cmd).expect("failed to read installed _cmd");

    assert!(installed_ir.exists());
    assert!(installed_cmd.exists());
    assert!(!root.join("image/usr/lib/llvmir/libdemo.so.1.2.3").exists());
    assert!(!root.join("image/usr/lib/llvmir/libdemo.so.1.2.3T").exists());
    assert!(script.contains("# Original output: libdemo.so.1"));
    assert!(script.contains("./libdemo.so.1"));
    assert!(script.contains("--output=output/libdemo.so.1"));
}

#[test]
fn meson_install_uses_manifest_and_destdir_to_sync_llvmir_files() {
    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    let destdir = root.join("image");
    let library = root.join("build/libdemo.so.1.2.3");
    let executable = root.join("build/demo");
    let header = root.join("include/demo.h");
    let library_ir = llvmir_path(&llvmir_dir, &library);

    write_file(&library, "shared-library-placeholder");
    write_file(&library_ir, "bitcode");
    write_file(
        &PathBuf::from(format!("{}_cmd", library_ir.display())),
        "#!/bin/bash\n# Original output: libdemo.so.1.2.3\nclang -Wl,-soname,libdemo.so.1 ./libdemo.so.1.2.3 -Wl,--version-script=./libdemo.so.1.2.3_verscript\n",
    );
    write_file(
        &PathBuf::from(format!("{}_verscript", library_ir.display())),
        "LIBDEMO_1 { global: demo; };\n",
    );
    write_file(
        &PathBuf::from(format!("{}_log", library_ir.display())),
        "llvm-link log\n",
    );
    write_file(&executable, "executable-placeholder");
    write_file(
        &llvmir_path(&llvmir_dir, &executable).with_extension("bc"),
        "executable-bitcode",
    );
    write_file(&header, "#pragma once\n");

    let manifest = root.join("intro-installed.json");
    write_file(
        &manifest,
        &json!({
            library.to_string_lossy(): "/usr/lib/libdemo.so.1.2.3",
            executable.to_string_lossy(): "/usr/bin/demo",
            header.to_string_lossy(): "/usr/include/demo.h",
            "libdemo.so.1": "/usr/lib/libdemo.so.1",
        })
        .to_string(),
    );

    run_meson_install(root, &llvmir_dir, &manifest, &destdir);

    let llvmir_lib_dir = destdir.join("usr/lib/llvmir");
    assert!(llvmir_lib_dir.join("libdemo.so.1").exists());
    assert!(llvmir_lib_dir.join("libdemo.so.1_cmd").exists());
    assert!(llvmir_lib_dir.join("libdemo.so.1_verscript").exists());
    assert!(!llvmir_lib_dir.join("libdemo.so.1_log").exists());
    assert!(!llvmir_lib_dir.join("libdemo.so.1.2.3").exists());
    let cmd = fs::read_to_string(llvmir_lib_dir.join("libdemo.so.1_cmd"))
        .expect("failed to read installed cmd file");
    assert!(cmd.contains("# Original output: libdemo.so.1"));
    assert!(cmd.contains("./libdemo.so.1"));
    assert!(cmd.contains("libdemo.so.1_verscript"));
    assert!(destdir.join("usr/lib/llvmir-bin/demo.bc").exists());
    assert!(!destdir.join("usr/include/llvmir/demo.h").exists());
}

#[test]
fn meson_install_handles_missing_native_members_like_install_wrap() {
    if !command_exists("clang") || !command_exists("llvm-ar") {
        eprintln!("skipping Meson missing-member test: clang or llvm-ar is unavailable");
        return;
    }

    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    let destdir = root.join("image");

    let archive = root.join("build/libmixed.a");
    let archive_ir = llvmir_path(&llvmir_dir, &archive);
    let bitcode_member = root.join("build/ir.o");
    let native_member = root.join("build/native.o");
    fs::create_dir_all(root.join("build")).expect("failed to create build directory");
    fs::create_dir_all(
        archive_ir
            .parent()
            .expect("LLVM archive fixture should have a parent directory"),
    )
    .expect("failed to create LLVM archive directory");
    write_file(&root.join("ir.c"), "int ir_part(void) { return 1; }\n");
    write_file(
        &root.join("native.c"),
        "int native_part(void) { return 2; }\n",
    );
    for (source, object) in [("ir.c", &bitcode_member), ("native.c", &native_member)] {
        let status = Command::new("clang")
            .current_dir(root)
            .args([source, "-c", "-o"])
            .arg(object)
            .status()
            .expect("failed to compile native test object");
        assert!(status.success());
    }
    let archive_status = Command::new("llvm-ar")
        .args(["rcsD"])
        .arg(&archive_ir)
        .arg(&bitcode_member)
        .status()
        .expect("failed to create LLVM archive fixture");
    assert!(archive_status.success());
    write_file(&archive, "native archive placeholder");
    write_file(
        &PathBuf::from(format!("{}_missing", archive_ir.display())),
        &format!("{}\n", native_member.display()),
    );

    let library = root.join("build/libdemo.so");
    let library_ir = llvmir_path(&llvmir_dir, &library);
    let missing_object = root.join("build/link-native.o");
    write_file(&library, "shared-library-placeholder");
    write_file(&library_ir, "bitcode");
    write_file(&missing_object, "native-object");
    write_file(
        &PathBuf::from(format!("{}_cmd", library_ir.display())),
        "#!/bin/bash\ncmd=(\n  clang\n  ./libdemo.so\n  -Wl,/build/link-native.o\n  --output=output/libdemo.so\n)\n\"${cmd[@]}\"\n",
    );
    write_file(
        &PathBuf::from(format!("{}_missing", library_ir.display())),
        &format!("{}\n", missing_object.display()),
    );

    let manifest = root.join("intro-installed.json");
    write_file(
        &manifest,
        &json!({
            archive.to_string_lossy(): "/usr/lib/libmixed.a",
            library.to_string_lossy(): "/usr/lib/libdemo.so",
        })
        .to_string(),
    );
    run_meson_install(root, &llvmir_dir, &manifest, &destdir);

    let installed_archive = destdir.join("usr/lib/llvmir/libmixed.a");
    let archive_members = Command::new("llvm-ar")
        .args(["t"])
        .arg(&installed_archive)
        .output()
        .expect("failed to list installed LLVM archive");
    assert!(archive_members.status.success());
    assert!(String::from_utf8_lossy(&archive_members.stdout)
        .lines()
        .any(|member| member == "native.o"));
    assert!(!destdir.join("usr/lib/llvmir/libmixed.a_missing").exists());

    let installed_dir = destdir.join("usr/lib/llvmir");
    assert!(!installed_dir.join("libdemo.so_missing").exists());
    assert!(fs::read_dir(installed_dir.join("asm"))
        .expect("Meson-installed asm directory should exist")
        .next()
        .is_some());
    let cmd = fs::read_to_string(installed_dir.join("libdemo.so_cmd"))
        .expect("failed to read Meson-installed command");
    let asm_name = asm_name_for(&missing_object);
    assert!(cmd.contains(&format!("./asm/{asm_name}")));
    assert!(installed_dir.join("asm").join(asm_name).is_file());
    assert!(!cmd.contains(&missing_object.display().to_string()));
}

#[test]
fn generated_cmd_script_handles_spaces_without_eval() {
    if !command_exists("clang") || !command_exists("llvm-link") {
        eprintln!("skipping _cmd script test: clang or llvm-link is unavailable");
        return;
    }

    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");

    write_file(
        &root.join("hello space.c"),
        "int main(void) { return 0; }\n",
    );

    let status = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["hello space.c", "-ffat-lto-objects", "-o", "hello space"])
        .status()
        .expect("failed to execute clang wrapper");
    assert!(status.success(), "clang wrapper failed with {status}");

    let cmd_file = llvmir_path(&llvmir_dir, &root.join("hello space_cmd"));
    let script = fs::read_to_string(&cmd_file).expect("failed to read generated _cmd script");
    assert!(!script.contains("eval "));
    assert!(!script.contains("newcmd=`"));
    assert!(!script.contains("cmd=\""));
    assert!(script.contains("cmd=("));
    assert!(script.contains("\"${cmd[@]}\""));
    assert!(!script.contains("_try_bc"));
    assert!(!script.contains("substituted_cmd"));
    assert!(!script.contains("-ffat-lto-objects"));
    assert!(script.lines().any(|line| line.trim() == "-O3"));
    assert!(script.contains("--output=output/hello space"));
    assert!(!script.lines().any(|line| line.trim() == "-o"));

    let syntax_status = Command::new("bash")
        .arg("-n")
        .arg(&cmd_file)
        .status()
        .expect("failed to run bash -n on generated _cmd script");
    assert!(
        syntax_status.success(),
        "generated _cmd script has invalid syntax"
    );

    let run_status = Command::new("bash")
        .current_dir(cmd_file.parent().expect("_cmd file should have parent"))
        .arg(
            cmd_file
                .file_name()
                .expect("_cmd file should have filename"),
        )
        .status()
        .expect("failed to execute generated _cmd script");
    assert!(
        run_status.success(),
        "generated _cmd script failed with {run_status}"
    );
    assert!(cmd_file
        .parent()
        .expect("_cmd file should have parent")
        .join("output/hello space")
        .exists());
}

#[test]
fn generated_cmd_script_expands_dash_l_to_real_library_path() {
    if !command_exists("clang") || !command_exists("llvm-link") {
        eprintln!("skipping -l expansion test: clang or llvm-link is unavailable");
        return;
    }

    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    let lib_dir = root.join("lib");
    fs::create_dir_all(&lib_dir).expect("failed to create library directory");

    write_file(&root.join("dep.c"), "int dep(void) { return 0; }\n");
    write_file(
        &root.join("cblas_dep.c"),
        "int cblas_dep(void) { return 0; }\n",
    );
    write_file(
        &root.join("main.c"),
        "int dep(void); int cblas_dep(void); int main(void) { return dep() + cblas_dep(); }\n",
    );

    let real_lib = lib_dir.join("libwrapdep.so.1.0.0");
    let symlink_lib = lib_dir.join("libwrapdep.so");
    let lib_status = Command::new("clang")
        .current_dir(root)
        .args([
            "-shared",
            "-fPIC",
            "dep.c",
            "-Wl,-soname,libwrapdep.so.1",
            "-o",
        ])
        .arg(&real_lib)
        .status()
        .expect("failed to build test shared library");
    assert!(
        lib_status.success(),
        "test shared library build failed with {lib_status}"
    );
    unix_fs::symlink(&real_lib, &symlink_lib)
        .expect("failed to create development library symlink");

    let cblas_lib_dir = root.join("cblas/.libs");
    fs::create_dir_all(&cblas_lib_dir).expect("failed to create cblas library directory");
    let real_cblas_lib = cblas_lib_dir.join("libgslcblas.so.0.0.0");
    let symlink_cblas_lib = cblas_lib_dir.join("libgslcblas.so");
    let cblas_status = Command::new("clang")
        .current_dir(root)
        .args([
            "-shared",
            "-fPIC",
            "cblas_dep.c",
            "-Wl,-soname,libgslcblas.so.0",
            "-o",
        ])
        .arg(&real_cblas_lib)
        .status()
        .expect("failed to build explicit test shared library");
    assert!(
        cblas_status.success(),
        "explicit test shared library build failed with {cblas_status}"
    );
    unix_fs::symlink(&real_cblas_lib, &symlink_cblas_lib)
        .expect("failed to create explicit development library symlink");

    let compile_status = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["main.c", "-c", "-o", "main.o"])
        .status()
        .expect("failed to compile test object with clang wrapper");
    assert!(
        compile_status.success(),
        "clang wrapper compile failed with {compile_status}"
    );

    let status = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .arg("main.o")
        .arg("-L")
        .arg(&lib_dir)
        .arg("-lwrapdep")
        .arg(&symlink_cblas_lib)
        .arg("-O1")
        .arg("-Wl,--as-needed")
        .arg("-Wl,--no-undefined")
        .arg("-Wl,-rpath,$ORIGIN/../lib")
        .args(["-o", "usesdep"])
        .status()
        .expect("failed to execute clang wrapper");
    assert!(status.success(), "clang wrapper failed with {status}");

    let cmd_file = llvmir_path(&llvmir_dir, &root.join("usesdep_cmd"));
    let script = fs::read_to_string(&cmd_file).expect("failed to read generated _cmd script");
    let exact_link_arg = "-l:libwrapdep.so.1";
    let exact_cblas_link_arg = "-l:libgslcblas.so.0";
    let symlink_lib_path = symlink_lib.display().to_string();
    let symlink_cblas_lib_path = symlink_cblas_lib.display().to_string();
    let real_cblas_lib_dir = real_cblas_lib
        .canonicalize()
        .expect("real explicit test shared library should canonicalize")
        .parent()
        .expect("real explicit test shared library should have parent")
        .display()
        .to_string();

    assert!(
        script.lines().any(|line| line.trim() == exact_link_arg),
        "generated _cmd should contain an exact -l: argument for the real shared library filename"
    );
    assert!(
        script
            .lines()
            .any(|line| line.trim() == "--output=output/usesdep"),
        "generated _cmd should use --output=output/usesdep"
    );
    assert!(
        script
            .lines()
            .any(|line| line.trim() == exact_cblas_link_arg),
        "generated _cmd should contain an exact -l: argument for the explicit shared library filename"
    );
    assert!(
        !script
            .lines()
            .any(|line| line.trim() == format!("-L{}", real_cblas_lib_dir)),
        "generated _cmd should not add -L for explicit shared library paths"
    );
    assert!(
        !script.lines().any(|line| line.trim() == "-o"),
        "generated _cmd should not use split -o output syntax"
    );
    assert!(
        script.lines().any(|line| line.trim() == "-O1"),
        "generated _cmd should retain the requested optimization level"
    );
    assert!(
        !script.lines().any(|line| line.trim() == "-O3"),
        "generated _cmd should not add -O3 when an optimization level is specified"
    );
    assert!(
        !script.lines().any(|line| line.trim() == "-lwrapdep"),
        "generated _cmd should not keep -lwrapdep when the library can be resolved"
    );
    assert!(
        !script.contains(&real_lib.display().to_string()),
        "generated _cmd should not record the full real shared library path"
    );
    assert!(
        !script.lines().any(|line| line.trim() == symlink_lib_path),
        "generated _cmd should not record the development symlink path"
    );
    assert!(
        !script
            .lines()
            .any(|line| line.trim() == symlink_cblas_lib_path),
        "generated _cmd should not record the explicit development symlink path"
    );
    let command_args: Vec<&str> = script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let library_position = command_args
        .iter()
        .position(|argument| *argument == exact_link_arg)
        .expect("generated _cmd should contain the resolved library");
    assert!(library_position > 0);
    assert!(
        !script.contains(&lib_dir.display().to_string()),
        "generated _cmd should not retain the original build-only -L directory"
    );
    for linker_option in [
        "-Wl,--as-needed",
        "-Wl,--no-undefined",
        "-Wl,-rpath,$ORIGIN/../lib",
    ] {
        assert!(
            script.contains(linker_option),
            "generated _cmd should preserve linker option {linker_option}"
        );
    }
}

#[test]
fn static_library_ir_is_llvm_archive_and_records_missing_members() {
    if !command_exists("clang") || !command_exists("llvm-ar") {
        eprintln!("skipping static library archive test: clang or llvm-ar is unavailable");
        return;
    }

    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");

    write_file(&root.join("util.c"), "int util(void) { return 7; }\n");
    write_file(
        &root.join("asm-only.s"),
        ".text\n.globl asm_only\nasm_only:\n  ret\n",
    );

    let compile_util = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["util.c", "-c", "-o", "util.o"])
        .status()
        .expect("failed to compile util.o with clang wrapper");
    assert!(
        compile_util.success(),
        "clang wrapper util compile failed with {compile_util}"
    );

    let compile_asm = Command::new("clang")
        .current_dir(root)
        .args(["asm-only.s", "-c", "-o", "asm-only.o"])
        .status()
        .expect("failed to compile asm-only.o");
    assert!(
        compile_asm.success(),
        "assembly compile failed with {compile_asm}"
    );

    let archive = Command::new(bin("ar"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["csrD", "libutil.a", "util.o", "asm-only.o"])
        .status()
        .expect("failed to archive libutil.a with ar wrapper");
    assert!(archive.success(), "ar wrapper failed with {archive}");

    let archive_ir = llvmir_path(&llvmir_dir, &root.join("libutil.a"));
    let missing_sidecar = PathBuf::from(format!("{}_missing", archive_ir.display()));
    let archive_members = Command::new("llvm-ar")
        .arg("t")
        .arg(&archive_ir)
        .output()
        .expect("failed to list LLVM archive members");
    assert!(
        archive_members.status.success(),
        "llvm-ar failed to read generated archive: {}",
        String::from_utf8_lossy(&archive_members.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&archive_members.stdout)
            .lines()
            .collect::<Vec<_>>(),
        vec!["util.o"],
        "LLVM archive should contain only bitcode-backed members"
    );
    assert_eq!(
        fs::read_to_string(&missing_sidecar).expect("failed to read missing-member sidecar"),
        format!("{}\n", root.join("asm-only.o").display()),
        "sidecar should contain the missing member's full absolute path"
    );

    let clean_archive_ir = llvmir_path(&llvmir_dir, &root.join("libclean.a"));
    let clean_missing_sidecar = PathBuf::from(format!("{}_missing", clean_archive_ir.display()));
    write_file(&clean_missing_sidecar, "/stale/member.o\n");

    let archive_without_missing = Command::new(bin("ar"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["csrD", "libclean.a", "util.o"])
        .status()
        .expect("failed to archive libclean.a with ar wrapper");
    assert!(
        archive_without_missing.success(),
        "ar wrapper failed with {archive_without_missing}"
    );
    assert!(
        !clean_missing_sidecar.exists(),
        "archive without missing members should not leave a sidecar"
    );
}

#[test]
fn final_link_missing_sidecar_merges_direct_and_static_library_entries() {
    if !command_exists("clang") || !command_exists("llvm-link") || !command_exists("llvm-ar") {
        eprintln!("skipping final missing-sidecar test: LLVM tools are unavailable");
        return;
    }

    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");

    write_file(&root.join("libpart.c"), "int libpart(void) { return 7; }\n");
    write_file(
        &root.join("main.c"),
        "int libpart(void); int native_only(void); int main(void) { return libpart() + native_only() != 10; }\n",
    );
    write_file(
        &root.join("native-only.c"),
        "int native_only(void) { return 3; }\n",
    );
    write_file(&root.join("clean-main.c"), "int main(void) { return 0; }\n");
    write_file(
        &root.join("archive-missing.s"),
        ".text\n.globl archive_missing\narchive_missing:\n  ret\n",
    );

    let compile_libpart = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["libpart.c", "-c", "-o", "libpart.o"])
        .status()
        .expect("failed to compile libpart.o with clang wrapper");
    assert!(compile_libpart.success());

    for (source, object) in [
        ("native-only.c", "native-only.o"),
        ("archive-missing.s", "archive-missing.o"),
    ] {
        let status = Command::new("clang")
            .current_dir(root)
            .args([source, "-c", "-o", object])
            .status()
            .expect("failed to compile native test object");
        assert!(status.success());
    }

    let archive = Command::new(bin("ar"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["csrD", "libpart.a", "libpart.o", "archive-missing.o"])
        .status()
        .expect("failed to create static library with ar wrapper");
    assert!(archive.success());

    let link = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["main.c", "native-only.o", "libpart.a", "-o", "app"])
        .status()
        .expect("failed to link app with clang wrapper");
    assert!(link.success(), "clang wrapper link failed with {link}");

    let app_ir = llvmir_path(&llvmir_dir, &root.join("app"));
    let app_missing = PathBuf::from(format!("{}_missing", app_ir.display()));
    assert_eq!(
        fs::read_to_string(&app_missing).expect("failed to read final missing sidecar"),
        format!(
            "{}\n{}\n",
            root.join("native-only.o").display(),
            root.join("archive-missing.o").display(),
        ),
        "final sidecar should contain direct and static-library missing members"
    );

    let clean_link = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["clean-main.c", "-o", "app"])
        .status()
        .expect("failed to relink app without missing direct objects");
    assert!(clean_link.success());
    assert!(
        !app_missing.exists(),
        "relink without missing members should remove a stale final sidecar"
    );
}

#[test]
fn generated_cmd_expands_linker_script_input_dependencies() {
    if !command_exists("clang") || !command_exists("llvm-link") {
        eprintln!("skipping linker script expansion test: clang or llvm-link is unavailable");
        return;
    }

    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");
    let lib_dir = root.join("lib");
    fs::create_dir_all(&lib_dir).expect("failed to create library directory");

    write_file(&root.join("primary.c"), "int primary(void) { return 1; }\n");
    write_file(&root.join("dep.c"), "int dep(void) { return 2; }\n");
    write_file(
        &root.join("main.c"),
        "int primary(void); int dep(void); int main(void) { return primary() + dep() == 3 ? 0 : 1; }\n",
    );

    let primary_real = lib_dir.join("libprimary.so.1.0.0");
    let dep_real = lib_dir.join("libscriptdep.so.2.0.0");

    let primary_status = Command::new("clang")
        .current_dir(root)
        .args([
            "-shared",
            "-fPIC",
            "primary.c",
            "-Wl,-soname,libprimary.so.1",
            "-o",
        ])
        .arg(&primary_real)
        .status()
        .expect("failed to build primary shared library");
    assert!(primary_status.success());

    let dep_status = Command::new("clang")
        .current_dir(root)
        .args([
            "-shared",
            "-fPIC",
            "dep.c",
            "-Wl,-soname,libscriptdep.so.2",
            "-o",
        ])
        .arg(&dep_real)
        .status()
        .expect("failed to build dependency shared library");
    assert!(dep_status.success());

    unix_fs::symlink(&dep_real, lib_dir.join("libscriptdep.so"))
        .expect("failed to create dependency development symlink");
    write_file(
        &lib_dir.join("libwrapscript.so"),
        "INPUT(libprimary.so.1.0.0 -lscriptdep)\n",
    );

    let compile_status = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["main.c", "-c", "-o", "main.o"])
        .status()
        .expect("failed to compile main.o with clang wrapper");
    assert!(compile_status.success());

    let link_status = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .arg("main.o")
        .arg("-L")
        .arg(&lib_dir)
        .arg("-lwrapscript")
        .args(["-o", "uses-script"])
        .status()
        .expect("failed to link executable with linker script library");
    assert!(link_status.success());

    let cmd_file = llvmir_path(&llvmir_dir, &root.join("uses-script_cmd"));
    let script = fs::read_to_string(&cmd_file).expect("failed to read generated _cmd script");

    assert!(script
        .lines()
        .any(|line| line.trim() == "-l:libprimary.so.1"));
    assert!(script
        .lines()
        .any(|line| line.trim() == "-l:libscriptdep.so.2"));
    assert!(!script.lines().any(|line| line.trim() == "-lwrapscript"));
    assert!(!script.lines().any(|line| line.trim() == "-lscriptdep"));
}
