use std::env;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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

fn run_wrapper(binary: &str, cwd: &Path, llvmir_dir: &Path, args: &[&str]) {
    let status = Command::new(bin(binary))
        .current_dir(cwd)
        .env("LLVM_IR_DIR", llvmir_dir)
        .args(args)
        .status()
        .expect("failed to execute wrapper binary");
    assert!(status.success(), "{binary} wrapper failed with {status}");
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
        .arg("-Wl,-rpath,/tmp/drop-rpath-comma")
        .arg("-Wl,-rpath=/tmp/drop-rpath-equals")
        .arg("-Wl,--rpath,/tmp/drop-rpath-long")
        .arg("-Wl,-rpath")
        .arg("-Wl,/tmp/drop-rpath-split")
        .args(["-o", "usesdep"])
        .status()
        .expect("failed to execute clang wrapper");
    assert!(status.success(), "clang wrapper failed with {status}");

    let cmd_file = llvmir_path(&llvmir_dir, &root.join("usesdep_cmd"));
    let script = fs::read_to_string(&cmd_file).expect("failed to read generated _cmd script");
    let exact_link_arg = "-l:libwrapdep.so.1.0.0";
    let exact_cblas_link_arg = "-l:libgslcblas.so.0.0.0";
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
    for rpath_fragment in [
        "-rpath",
        "--rpath",
        "/tmp/drop-rpath-comma",
        "/tmp/drop-rpath-equals",
        "/tmp/drop-rpath-long",
        "/tmp/drop-rpath-split",
    ] {
        assert!(
            !script.contains(rpath_fragment),
            "generated _cmd should not contain rpath fragment {rpath_fragment}"
        );
    }
}

#[test]
fn link_executable_merges_explicit_static_library_ir() {
    if !command_exists("clang") || !command_exists("llvm-link") {
        eprintln!("skipping static library merge test: clang or llvm-link is unavailable");
        return;
    }

    let test_dir = TestDir::new();
    let root = test_dir.path();
    let llvmir_dir = root.join("llvmir");

    write_file(&root.join("util.c"), "int util(void) { return 7; }\n");
    write_file(
        &root.join("main.c"),
        "int util(void); int main(void) { return util() == 7 ? 0 : 1; }\n",
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

    let archive = Command::new(bin("ar"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["csrDT", "libutil.a", "util.o"])
        .status()
        .expect("failed to archive libutil.a with ar wrapper");
    assert!(archive.success(), "ar wrapper failed with {archive}");

    let compile_main = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .args(["main.c", "-c", "-o", "main.o"])
        .status()
        .expect("failed to compile main.o with clang wrapper");
    assert!(
        compile_main.success(),
        "clang wrapper main compile failed with {compile_main}"
    );

    let libutil = root.join("libutil.a");
    let link = Command::new(bin("clang"))
        .current_dir(root)
        .env("EMIT_LLVMIR", "1")
        .env("LLVM_IR_DIR", &llvmir_dir)
        .arg("main.o")
        .arg(&libutil)
        .args(["-o", "app"])
        .status()
        .expect("failed to link app with clang wrapper");
    assert!(link.success(), "clang wrapper link failed with {link}");

    let app_ir = llvmir_path(&llvmir_dir, &root.join("app"));
    let app_log = PathBuf::from(format!("{}_log", app_ir.display()));
    let archive_ir = llvmir_path(&llvmir_dir, &libutil);
    let log = fs::read_to_string(&app_log).expect("failed to read app llvm-link log");

    assert!(app_ir.exists());
    assert!(archive_ir.exists());
    assert!(
        log.contains(&archive_ir.display().to_string()),
        "executable llvm-link log should include static library IR {}",
        archive_ir.display()
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
        .any(|line| line.trim() == "-l:libprimary.so.1.0.0"));
    assert!(script
        .lines()
        .any(|line| line.trim() == "-l:libscriptdep.so.2.0.0"));
    assert!(!script.lines().any(|line| line.trim() == "-lwrapscript"));
    assert!(!script.lines().any(|line| line.trim() == "-lscriptdep"));
}
