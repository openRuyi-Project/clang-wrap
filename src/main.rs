// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! clang/clang++ wrapper
//!
//! Generate LLVM IR during compilation, use llvm-link to merge LLVM IR files during linking

use std::collections::HashSet;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Stdio};

use goblin::elf::Elf;

use clang_wrap::{
    append_tmp_suffix, debug_log, ensure_dir_exists, expand_at_file_args, find_llvm_tool,
    find_llvmir_file, generate_tmp_suffix, get_absolute_path, get_emit_llvmir_opt, get_exe_path,
    get_llvm_ir_dir, get_program_name, init_debug_log, is_debug_mode, shell_escape,
};

// ============================================================================
// Constant definitions
// ============================================================================

/// Source file extensions
const SOURCE_EXTENSIONS: &[&str] = &[
    ".c", ".cpp", ".cc", ".cxx", ".c++", ".m", ".mm", ".S", ".s", ".asm",
];

/// Get clang version information
fn get_clang_version(clang_path: &Path) -> Option<String> {
    let output = Command::new(clang_path).arg("--version").output().ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(first_line) = stdout.lines().next() {
            return Some(first_line.to_string());
        }
    }
    None
}

fn clang_major_version(version: &str) -> Option<&str> {
    let version_pos = version.find("version")?;
    let after_version = &version[version_pos + "version".len()..];
    let start = after_version.find(|ch: char| ch.is_ascii_digit())?;
    let major = after_version[start..]
        .split(|ch: char| !ch.is_ascii_digit())
        .next()?;

    if major.is_empty() {
        None
    } else {
        Some(major)
    }
}

fn precise_usr_bin_clang_path(clang_path: &Path, clang_version: &str) -> PathBuf {
    let Some(major) = clang_major_version(clang_version) else {
        return clang_path.to_path_buf();
    };

    if clang_path == Path::new("/usr/bin/clang") {
        PathBuf::from(format!("clang-{}", major))
    } else if clang_path == Path::new("/usr/bin/clang++") {
        PathBuf::from(format!("clang++-{}", major))
    } else {
        clang_path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("clang-wrap-{prefix}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn clang_major_version_parses_common_version_lines() {
        assert_eq!(
            clang_major_version("Ubuntu clang version 21.1.8 (6ubuntu1)"),
            Some("21")
        );
        assert_eq!(
            clang_major_version("Debian clang version 19.1.7 (++20250114073446+cd708029e0b2-1)"),
            Some("19")
        );
        assert_eq!(clang_major_version("not a clang banner"), None);
    }

    #[test]
    fn ir_generation_drops_fat_lto_objects_option() {
        let args = vec![
            "-O2".to_string(),
            "-ffat-lto-objects".to_string(),
            "-g".to_string(),
        ];

        assert_eq!(
            args.into_iter()
                .filter(|arg| !is_ir_incompatible_option(arg))
                .collect::<Vec<_>>(),
            vec!["-O2", "-g"]
        );
    }

    #[test]
    fn linker_argument_forms_are_normalized_before_classification() {
        let args = [
            "-Wl,--dynamic-list".to_string(),
            "-Wl,/build/dynamic.list".to_string(),
            "-Xlinker".to_string(),
            "--version-script".to_string(),
            "-Xlinker".to_string(),
            "/build/exports.map".to_string(),
            "-Wl,-rpath".to_string(),
            "-Wl,$ORIGIN/../lib".to_string(),
            "-Xlinker".to_string(),
            "-rpath-link".to_string(),
            "-Xlinker".to_string(),
            "/build/lib".to_string(),
        ];
        assert_eq!(
            normalize_linker_arguments(&args),
            [
                "-Wl,--dynamic-list=/build/dynamic.list",
                "-Wl,--version-script=/build/exports.map",
                "-Wl,-rpath,$ORIGIN/../lib",
                "-Wl,-rpath-link,/build/lib",
            ]
        );
    }

    #[test]
    fn precise_usr_bin_clang_path_uses_versioned_command_name() {
        assert_eq!(
            precise_usr_bin_clang_path(
                Path::new("/usr/bin/clang"),
                "Ubuntu clang version 21.1.8 (6ubuntu1)",
            ),
            PathBuf::from("clang-21")
        );
        assert_eq!(
            precise_usr_bin_clang_path(
                Path::new("/usr/bin/clang++"),
                "Ubuntu clang version 21.1.8 (6ubuntu1)",
            ),
            PathBuf::from("clang++-21")
        );
        assert_eq!(
            precise_usr_bin_clang_path(
                Path::new("/opt/llvm/bin/clang"),
                "Ubuntu clang version 21.1.8 (6ubuntu1)",
            ),
            PathBuf::from("/opt/llvm/bin/clang")
        );
    }

    #[test]
    fn nonportable_linker_path_options_are_detected_in_all_supported_spellings() {
        for args in [
            vec!["--sysroot", "/build/sysroot"],
            vec!["-Wl,-T,/build/layout.ld"],
            vec!["-Wl,/build/opaque-linker-input.o"],
            vec!["-Xlinker", "--sysroot=/build/sysroot"],
            vec!["-Xlinker", "-T/build/layout.ld"],
            vec!["-Xlinker", "/build/opaque-linker-input.o"],
        ] {
            let args = args.into_iter().map(str::to_string).collect::<Vec<_>>();
            assert!(has_nonportable_linker_path_option(&args), "{args:?}");
        }

        let frontend_only = vec!["-isysroot".to_string(), "/build/sysroot".to_string()];
        assert!(!has_nonportable_linker_path_option(&frontend_only));

        for args in [
            vec!["-Wl,--start-group", "-lfoo", "-Wl,--end-group"],
            vec!["-Wl,--whole-archive", "-lfoo", "-Wl,--no-whole-archive"],
            vec!["-Xlinker", "--as-needed"],
            vec!["-Wl,-rpath", "-Wl,/private/build/.libs"],
            vec!["-Wl,-rpath,/private/build/.libs"],
            vec!["-Wl,-rpath-link,/build/lib"],
            vec!["-rpath-link", "/build/lib"],
            vec!["-Xlinker", "-rpath-link", "-Xlinker", "/build/lib"],
            vec!["-Xlinker", "--rpath-link=/build/lib"],
        ] {
            let args = args.into_iter().map(str::to_string).collect::<Vec<_>>();
            assert!(!has_nonportable_linker_path_option(&args), "{args:?}");
        }
    }

    #[test]
    fn rejected_linker_paths_fail_without_creating_cmd_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("clang-wrap-cmd-reject-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let bitcode = dir.join("program.bc");
        let args = vec![
            "program.o".to_string(),
            "-Xlinker".to_string(),
            "-T".to_string(),
            "-Xlinker".to_string(),
            "/private/build/linker-script.ld".to_string(),
        ];

        let error = generate_link_cmd_file(
            Path::new("/bin/false"),
            &bitcode,
            Path::new("program"),
            &args,
            &[],
            false,
            None,
            None,
            None,
            &[],
        )
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!PathBuf::from(format!("{}_cmd", bitcode.display())).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn compiler_only_options_are_omitted_from_cmd_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("clang-wrap-cmd-frontend-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let bitcode = dir.join("program.bc");
        let args = vec![
            "program.o".to_string(),
            "-isysroot".to_string(),
            "/private/build/sysroot".to_string(),
            "-I/private/build/include".to_string(),
            "-Xclang".to_string(),
            "-ast-dump".to_string(),
        ];

        generate_link_cmd_file(
            Path::new("/bin/false"),
            &bitcode,
            Path::new("program"),
            &args,
            &[],
            false,
            None,
            None,
            None,
            &[],
        )
        .unwrap();

        let script = fs::read_to_string(format!("{}_cmd", bitcode.display())).unwrap();
        for value in [
            "-isysroot",
            "/private/build/sysroot",
            "-I/private/build/include",
            "-Xclang",
            "-ast-dump",
        ] {
            assert!(!script.contains(value), "script leaked {value}");
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn absolute_rpath_is_omitted_without_rejecting_cmd_generation() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("clang-wrap-cmd-rpath-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let bitcode = dir.join("program.bc");
        let rpath = "/private/build/.libs";
        let args = vec![
            "program.o".to_string(),
            "-Wl,-rpath".to_string(),
            format!("-Wl,{rpath}"),
        ];

        generate_link_cmd_file(
            Path::new("/bin/false"),
            &bitcode,
            Path::new("program"),
            &args,
            &[],
            false,
            None,
            None,
            None,
            &[],
        )
        .unwrap();

        let script = fs::read_to_string(format!("{}_cmd", bitcode.display())).unwrap();
        assert!(!script.contains(rpath));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn split_version_script_option_is_rewritten_to_installed_sidecar() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("clang-wrap-cmd-verscript-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let bitcode = dir.join("program.bc");
        let args = vec![
            "program.o".to_string(),
            "-Wl,-version-script".to_string(),
            "-Wl,.libs/program.ver".to_string(),
        ];

        generate_link_cmd_file(
            Path::new("/bin/false"),
            &bitcode,
            Path::new("program"),
            &args,
            &[],
            false,
            Some(Path::new("/artifact/program_verscript")),
            None,
            None,
            &[],
        )
        .unwrap();

        let script = fs::read_to_string(format!("{}_cmd", bitcode.display())).unwrap();
        assert!(script.contains("-Wl,--version-script=./program_verscript"));
        assert!(!script.contains(".libs/program.ver"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn split_long_version_script_option_is_rewritten_to_installed_sidecar() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("clang-wrap-cmd-long-verscript-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let bitcode = dir.join("program.bc");
        let args = vec![
            "program.o".to_string(),
            "-Wl,--version-script".to_string(),
            "-Wl,../src/libpcre2-8.sym".to_string(),
        ];

        generate_link_cmd_file(
            Path::new("/bin/false"),
            &bitcode,
            Path::new("program"),
            &args,
            &[],
            false,
            Some(Path::new("/artifact/program_verscript")),
            None,
            None,
            &[],
        )
        .unwrap();

        let script = fs::read_to_string(format!("{}_cmd", bitcode.display())).unwrap();
        assert!(script.contains("-Wl,--version-script=./program_verscript"));
        assert!(!script.contains("../src/libpcre2-8.sym"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dynamic_list_option_is_rewritten_to_installed_sidecar() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("clang-wrap-cmd-dynamic-list-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let bitcode = dir.join("program.bc");
        let args = vec![
            "program.o".to_string(),
            "-Wl,--dynamic-list=/private/build/dynamic.list".to_string(),
        ];

        generate_link_cmd_file(
            Path::new("/bin/false"),
            &bitcode,
            Path::new("program"),
            &args,
            &[],
            false,
            None,
            Some(Path::new("/artifact/program_dynamiclist")),
            None,
            &[],
        )
        .unwrap();

        let script = fs::read_to_string(format!("{}_cmd", bitcode.display())).unwrap();
        assert!(script.contains("-Wl,--dynamic-list=./program_dynamiclist"));
        assert!(!script.contains("/private/build/dynamic.list"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn cmd_searches_parent_install_library_directory() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("clang-wrap-cmd-parent-lib-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let bitcode = dir.join("program.bc");

        generate_link_cmd_file(
            Path::new("/bin/false"),
            &bitcode,
            Path::new("program"),
            &["program.o".to_string(), "-l:libproject.so.0".to_string()],
            &[],
            false,
            None,
            None,
            None,
            &[],
        )
        .unwrap();

        let script = fs::read_to_string(format!("{}_cmd", bitcode.display())).unwrap();
        assert!(script.contains("  -L..\n"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dependency_file_options_are_omitted_from_cmd_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("clang-wrap-cmd-dependency-file-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        let bitcode = dir.join("program.bc");
        let args = vec![
            "program.o".to_string(),
            "-Xlinker".to_string(),
            "--dependency-file=CMakeFiles/program.dir/link.d".to_string(),
            "-Xlinker".to_string(),
            "--dependency-file".to_string(),
            "-Xlinker".to_string(),
            "CMakeFiles/program.dir/split-link.d".to_string(),
            "-Wl,--dependency-file,CMakeFiles/program.dir/wl-link.d".to_string(),
        ];

        generate_link_cmd_file(
            Path::new("/bin/false"),
            &bitcode,
            Path::new("program"),
            &args,
            &[],
            false,
            None,
            None,
            None,
            &[],
        )
        .unwrap();

        let script = fs::read_to_string(format!("{}_cmd", bitcode.display())).unwrap();
        assert!(!script.contains("dependency-file"));
        assert!(!script.contains("link.d"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn static_archives_select_members_before_one_link() {
        let dir = unique_temp_dir("llvm-link-archive-selection");
        let linker = dir.join("fake-llvm-link");
        let invocation_log = dir.join("invocations");
        let output = dir.join("merged.bc");
        let direct = dir.join("main.bc");
        let archive_a = dir.join("liba.a");
        let archive_b = dir.join("libb.a");
        let log_file = File::create(dir.join("merge.log")).unwrap();
        let source_main = dir.join("main.c");
        let source_a = dir.join("a.c");
        let source_b = dir.join("b.c");
        fs::write(
            &source_main,
            "extern int archive_a(void); int main(void) { return archive_a(); }\n",
        )
        .unwrap();
        fs::write(&source_a, "int archive_a(void) { return 1; }\n").unwrap();
        fs::write(&source_b, "int archive_b(void) { return 2; }\n").unwrap();
        let clang = find_llvm_tool("clang", "clang");
        let archive_a_member = dir.join("archive_a.bc");
        for (source, bitcode) in [(&source_main, &direct), (&source_a, &archive_a_member)] {
            assert!(Command::new(&clang)
                .args(["-emit-llvm", "-c"])
                .arg(source)
                .arg("-o")
                .arg(bitcode)
                .status()
                .unwrap()
                .success());
        }
        assert!(Command::new(find_llvm_tool("llvm-ar", "clang"))
            .args(["rcsD"])
            .arg(&archive_a)
            .arg(&archive_a_member)
            .status()
            .unwrap()
            .success());
        let archive_b_member = dir.join("archive_b.bc");
        assert!(Command::new(find_llvm_tool("clang", "clang"))
            .args(["-emit-llvm", "-c"])
            .arg(&source_b)
            .arg("-o")
            .arg(&archive_b_member)
            .status()
            .unwrap()
            .success());
        assert!(Command::new(find_llvm_tool("llvm-ar", "clang"))
            .args(["rcsD"])
            .arg(&archive_b)
            .arg(&archive_b_member)
            .status()
            .unwrap()
            .success());
        fs::write(
            &linker,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nout=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '-o' ]; then out=\"$2\"; shift 2; continue; fi\n  shift\ndone\nprintf merged > \"$out\"\n",
                invocation_log.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&linker).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&linker, permissions).unwrap();

        merge_llvm_ir(
            &linker,
            &output,
            std::slice::from_ref(&direct),
            &[archive_a.clone(), archive_b.clone()],
            &[],
            &log_file,
        )
        .unwrap();

        let invocations = fs::read_to_string(&invocation_log).unwrap();
        let lines = invocations.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "{invocations}");
        assert!(!lines[0].contains("--only-needed"));
        assert!(lines[0].contains("archive-0"), "{invocations}");
        assert!(output.exists());
        assert!(!PathBuf::from(format!("{}.direct-input", output.display())).exists());
        assert!(!PathBuf::from(format!("{}.static-archive-current", output.display())).exists());
        assert!(!PathBuf::from(format!("{}.static-archive-next", output.display())).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn whole_archive_bitcode_is_merged_without_symbol_selection() {
        let dir = unique_temp_dir("llvm-link-whole-archive");
        let linker = dir.join("fake-llvm-link");
        let invocation_log = dir.join("invocations");
        let output = dir.join("merged.bc");
        let direct = dir.join("version.bc");
        let archive = dir.join("libconvenience.a");
        let member = dir.join("convenience.bc");
        let log_file = File::create(dir.join("merge.log")).unwrap();
        let source = dir.join("whole.c");
        fs::write(&source, "int convenience(void) { return 3; }\n").unwrap();
        assert!(Command::new(find_llvm_tool("clang", "clang"))
            .args(["-emit-llvm", "-c"])
            .arg(&source)
            .arg("-o")
            .arg(&direct)
            .status()
            .unwrap()
            .success());
        fs::copy(&direct, &member).unwrap();
        let status = Command::new(find_llvm_tool("llvm-ar", "clang"))
            .args(["rcsD"])
            .arg(&archive)
            .arg(&member)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(
            &linker,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nout=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '-o' ]; then out=\"$2\"; shift 2; continue; fi\n  shift\ndone\nprintf merged > \"$out\"\n",
                invocation_log.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&linker).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&linker, permissions).unwrap();

        merge_llvm_ir(
            &linker,
            &output,
            std::slice::from_ref(&direct),
            &[],
            std::slice::from_ref(&archive),
            &log_file,
        )
        .unwrap();

        let invocations = fs::read_to_string(&invocation_log).unwrap();
        let lines = invocations.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "{invocations}");
        assert!(!lines[0].contains("--only-needed"), "{invocations}");
        assert!(lines[0].contains("whole-0"), "{invocations}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn whole_archive_option_is_detected_when_forwarded_to_the_linker() {
        for args in [
            vec!["-Wl,--whole-archive"],
            vec!["-Wl,--as-needed,--whole-archive"],
            vec!["-Xlinker", "--whole-archive"],
        ] {
            let args = args.into_iter().map(str::to_string).collect::<Vec<_>>();
            assert!(has_whole_archive_option(&args), "{args:?}");
        }

        assert!(!has_whole_archive_option(&[
            "-Wl,--no-whole-archive".to_string()
        ]));
    }

    #[test]
    fn archives_embedded_in_wl_whole_archive_options_are_collected() {
        let paths = whole_archives_in_wl_arg(
            "-Wl,--as-needed,--whole-archive,/tmp/libone.a,relative/libtwo.a,--no-whole-archive",
        );
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/tmp/libone.a"),
                PathBuf::from("relative/libtwo.a")
            ]
        );
        assert!(whole_archives_in_wl_arg("-Wl,--whole-archive,--no-whole-archive").is_empty());
    }

    #[test]
    fn symlinked_object_uses_its_canonical_path_for_ir_lookup() {
        let dir = unique_temp_dir("symlinked-ir-input");
        let target = dir.join("source.o");
        let link = dir.join(".libs/libfoo.lax/source.o");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        fs::write(&target, "native object").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert_eq!(ir_lookup_paths(&link), vec![link.clone(), target]);
        let _ = fs::remove_dir_all(dir);
    }
}

fn canonical_or_absolute_path(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|_| get_absolute_path(path))
}

fn resolve_library_in_dir(dir: &Path, lib: &str) -> Option<PathBuf> {
    if let Some(exact_name) = lib.strip_prefix(':') {
        let candidate = dir.join(exact_name);
        if candidate.exists() {
            return Some(canonical_or_absolute_path(&candidate));
        }
        return None;
    }

    for filename in [format!("lib{}.so", lib), format!("lib{}.a", lib)] {
        let candidate = dir.join(filename);
        if candidate.exists() {
            return Some(canonical_or_absolute_path(&candidate));
        }
    }

    None
}

fn configured_library_dirs(library_dirs: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for dir in library_dirs {
        dirs.push(get_absolute_path(Path::new(dir)));
    }

    if let Ok(library_path) = env::var("LIBRARY_PATH") {
        for dir in library_path.split(':').filter(|dir| !dir.is_empty()) {
            dirs.push(get_absolute_path(Path::new(dir)));
        }
    }

    dirs
}

fn ldconfig_library_paths(lib: &str) -> Vec<PathBuf> {
    let output = match Command::new("ldconfig").arg("-p").output() {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    let exact_name = lib.strip_prefix(':').unwrap_or(lib);
    let shared_prefix = format!("lib{}.so.", lib);
    let shared_name = format!("lib{}.so", lib);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut paths = Vec::new();

    for line in stdout.lines() {
        let Some((name_part, path_part)) = line.split_once("=>") else {
            continue;
        };
        let Some(name) = name_part.split_whitespace().next() else {
            continue;
        };

        let matches = if lib.starts_with(':') {
            name == exact_name
        } else {
            name == shared_name || name.starts_with(&shared_prefix)
        };

        if matches {
            paths.push(canonical_or_absolute_path(Path::new(path_part.trim())));
        }
    }

    paths
}

fn default_library_dirs(clang_path: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(output) = Command::new(clang_path).arg("-print-search-dirs").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(paths) = line.strip_prefix("libraries: =") {
                    for dir in paths.split(':').filter(|dir| !dir.is_empty()) {
                        dirs.push(get_absolute_path(Path::new(dir)));
                    }
                }
            }
        }
    }

    for dir in [
        "/lib",
        "/lib64",
        "/usr/lib",
        "/usr/lib64",
        "/usr/local/lib",
        "/usr/lib/x86_64-linux-gnu",
        "/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/lib/aarch64-linux-gnu",
        "/usr/lib/riscv64-linux-gnu",
        "/lib/riscv64-linux-gnu",
    ] {
        dirs.push(PathBuf::from(dir));
    }

    dirs
}

fn resolve_link_library(lib: &str, library_dirs: &[String], clang_path: &Path) -> Option<PathBuf> {
    for dir in configured_library_dirs(library_dirs) {
        if let Some(path) = resolve_library_in_dir(&dir, lib) {
            return Some(path);
        }
    }

    for dir in default_library_dirs(clang_path) {
        if let Some(path) = resolve_library_in_dir(&dir, lib) {
            return Some(path);
        }
    }

    if let Some(path) = ldconfig_library_paths(lib).into_iter().next() {
        return Some(path);
    }

    None
}

fn shared_library_soname(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Elf::parse(&bytes).ok()?.soname.map(ToOwned::to_owned)
}

fn exact_link_arg_for_library_path(path: &Path) -> Option<String> {
    path.file_name().and_then(|name| name.to_str()).map(|name| {
        let name = shared_library_soname(path).unwrap_or_else(|| name.to_string());
        format!("-l:{}", name)
    })
}

fn push_array_arg(script: &mut String, arg: &str) {
    script.push_str("  ");
    script.push_str(&shell_escape(arg));
    script.push('\n');
}

fn linker_script_members(path: &Path) -> Option<Vec<String>> {
    let content = fs::read_to_string(path).ok()?;
    let mut members = Vec::new();
    let mut rest = content.as_str();

    while let Some(start) = rest.find("INPUT(").or_else(|| rest.find("GROUP(")) {
        let after_keyword = &rest[start..];
        let open = after_keyword.find('(')?;
        let after_open = &after_keyword[open + 1..];
        let Some(close) = after_open.find(')') else {
            break;
        };

        for token in after_open[..close].split_whitespace() {
            let member = token.trim_matches(|c| c == ',' || c == '"' || c == '\'');
            if !member.is_empty() {
                members.push(member.to_string());
            }
        }

        rest = &after_open[close + 1..];
    }

    if members.is_empty() {
        None
    } else {
        Some(members)
    }
}

fn library_path_link_args(
    lib_file: &Path,
    library_dirs: &[String],
    clang_path: &Path,
    visited: &mut Vec<PathBuf>,
) -> Option<Vec<String>> {
    let canonical_path = canonical_or_absolute_path(lib_file);

    if visited.contains(&canonical_path) {
        return Some(Vec::new());
    }

    if let Some(members) = linker_script_members(&canonical_path) {
        visited.push(canonical_path.clone());
        let script_dir = canonical_path.parent().unwrap_or(Path::new("."));
        let mut expanded = Vec::new();

        for member in members {
            if let Some(lib) = member.strip_prefix("-l") {
                if let Some(args) =
                    resolved_library_link_args(lib, library_dirs, clang_path, visited)
                {
                    expanded.extend(args);
                } else {
                    expanded.push(member);
                }
                continue;
            }

            let member_path = Path::new(&member);
            let candidate = if member_path.is_absolute() {
                member_path.to_path_buf()
            } else {
                script_dir.join(member_path)
            };

            if candidate.exists() {
                expanded.extend(library_path_link_args(
                    &candidate,
                    library_dirs,
                    clang_path,
                    visited,
                )?);
            } else {
                expanded.push(member);
            }
        }

        return Some(expanded);
    }

    exact_link_arg_for_library_path(&canonical_path).map(|arg| vec![arg])
}

fn resolved_library_link_args(
    lib: &str,
    library_dirs: &[String],
    clang_path: &Path,
    visited: &mut Vec<PathBuf>,
) -> Option<Vec<String>> {
    resolve_link_library(lib, library_dirs, clang_path)
        .and_then(|path| library_path_link_args(&path, library_dirs, clang_path, visited))
}

fn is_rpath_arg(arg: &str) -> bool {
    matches!(arg, "-rpath" | "--rpath" | "-Wl,-rpath" | "-Wl,--rpath")
        || arg.starts_with("-rpath=")
        || arg.starts_with("--rpath=")
        || arg.starts_with("-Wl,-rpath,")
        || arg.starts_with("-Wl,--rpath,")
        || arg.starts_with("-Wl,-rpath=")
        || arg.starts_with("-Wl,--rpath=")
}

/// Convert the several compiler-driver spellings of linker arguments into one
/// representation before any link-command classification takes place.
/// Values are deliberately folded into the same argument: this makes
/// `-Xlinker --dynamic-list -Xlinker file`, `-Wl,--dynamic-list -Wl,file`,
/// and `-Wl,--dynamic-list,file` equivalent.
fn normalize_linker_arguments(args: &[String]) -> Vec<String> {
    let mut normalized = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        let (mut linker, mut consumed) = if argument == "-Xlinker" {
            let Some(value) = args.get(index + 1) else {
                normalized.push(argument.clone());
                index += 1;
                continue;
            };
            (value.clone(), 2)
        } else if let Some(value) = argument.strip_prefix("-Wl,") {
            (value.to_string(), 1)
        } else if matches!(
            argument.as_str(),
            "-rpath" | "--rpath" | "-rpath-link" | "--rpath-link"
        ) {
            (argument.clone(), 1)
        } else {
            normalized.push(argument.clone());
            index += 1;
            continue;
        };

        let (option, embedded_value) = linker
            .split_once([',', '='])
            .map_or((linker.as_str(), None), |(option, value)| {
                (option, Some(value))
            });
        let takes_value = matches!(
            option,
            "-version-script"
                | "--version-script"
                | "--dynamic-list"
                | "--dependency-file"
                | "-rpath"
                | "--rpath"
                | "-rpath-link"
                | "--rpath-link"
        );
        let mut value = embedded_value.map(str::to_owned);
        if takes_value && value.is_none() {
            if let Some(next) = args.get(index + consumed) {
                let mut next_value = next.as_str();
                let mut extra = 1;
                if next_value == "-Xlinker" {
                    if let Some(payload) = args.get(index + consumed + 1) {
                        next_value = payload;
                        extra = 2;
                    }
                } else if let Some(payload) = next_value.strip_prefix("-Wl,") {
                    next_value = payload;
                }
                value = Some(next_value.to_string());
                consumed += extra;
            }
        }
        if let Some(value) = value {
            linker = if matches!(
                option,
                "-version-script" | "--version-script" | "--dynamic-list" | "--dependency-file"
            ) {
                format!("{option}={value}")
            } else {
                format!("{option},{value}")
            };
        }
        normalized.push(format!("-Wl,{linker}"));
        index += consumed;
    }
    normalized
}

fn rpath_arg_consumes_next(arg: &str) -> bool {
    matches!(arg, "-rpath" | "--rpath" | "-Wl,-rpath" | "-Wl,--rpath")
}

fn is_rpath_link_arg(arg: &str) -> bool {
    matches!(arg, "-rpath-link" | "--rpath-link")
        || arg.starts_with("-rpath-link=")
        || arg.starts_with("--rpath-link=")
        || arg.starts_with("-Wl,-rpath-link,")
        || arg.starts_with("-Wl,--rpath-link,")
        || arg.starts_with("-Wl,-rpath-link=")
        || arg.starts_with("-Wl,--rpath-link=")
}

fn compiler_only_option_consumes_next(arg: &str) -> bool {
    matches!(
        arg,
        "-D" | "-U"
            | "-I"
            | "-F"
            | "-include"
            | "-imacros"
            | "-include-pch"
            | "-idirafter"
            | "-iprefix"
            | "-iwithprefix"
            | "-iwithprefixbefore"
            | "-isystem"
            | "-iquote"
            | "-isysroot"
            | "-resource-dir"
            | "-Xclang"
            | "-Xpreprocessor"
    )
}

/// Options consumed by Clang's front end while compiling sources.  `_cmd`
/// links already-produced bitcode, so retaining them is unnecessary and may
/// disclose source/build-tree paths.
fn is_compiler_only_option(arg: &str) -> bool {
    matches!(arg, "-nostdinc" | "-nostdinc++" | "-fsyntax-only")
        || arg.starts_with("-D")
        || arg.starts_with("-U")
        || (arg.starts_with("-I") && arg.len() > 2)
        || (arg.starts_with("-F") && arg.len() > 2)
        || arg.starts_with("-include=")
        || arg.starts_with("-imacros=")
        || arg.starts_with("-include-pch=")
        || arg.starts_with("-idirafter=")
        || arg.starts_with("-iprefix=")
        || arg.starts_with("-iwithprefix=")
        || arg.starts_with("-iwithprefixbefore=")
        || arg.starts_with("-isystem=")
        || arg.starts_with("-iquote=")
        || arg.starts_with("-isysroot=")
        || (arg.starts_with("-isysroot") && arg.len() > "-isysroot".len())
        || arg.starts_with("-resource-dir=")
        || arg.starts_with("-fdebug-prefix-map=")
        || arg.starts_with("-ffile-prefix-map=")
        || arg.starts_with("-fmacro-prefix-map=")
}

fn is_ir_incompatible_option(arg: &str) -> bool {
    arg == "-ffat-lto-objects"
}

/// Return whether an argument is a linker option whose value makes a generated
/// `_cmd` dependent on the original build root.  These options cannot be made
/// relocatable merely by changing their spelling: a sysroot selects an entire
/// toolchain and a general linker script can itself contain arbitrary paths.
/// Build-time-only `rpath-link` options are removed before this validation.
fn has_nonportable_linker_path_option(args: &[String]) -> bool {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];

        // An rpath is deliberately removed from generated `_cmd` files unless
        // it uses $ORIGIN (see generate_link_cmd_file).  Therefore temporary
        // build-directory rpaths, such as libtool relink rpaths, must not make
        // the original link fail. Skip both the option and its value here.
        if is_rpath_arg(arg) {
            index += if rpath_arg_consumes_next(arg) { 2 } else { 1 };
            continue;
        }
        if is_rpath_link_arg(arg) {
            index += if matches!(arg.as_str(), "-rpath-link" | "--rpath-link") {
                2
            } else {
                1
            };
            continue;
        }

        let direct_option = matches!(arg.as_str(), "--sysroot" | "-T" | "--script")
            || arg.starts_with("--sysroot=")
            || (arg.starts_with("-T") && arg.len() > 2)
            || arg.starts_with("--script=");
        if direct_option {
            return true;
        }

        if let Some(payload) = arg.strip_prefix("-Wl,") {
            // `-Wl,` accepts a comma-separated list.  An absolute linker
            // input is just as build-root dependent as an explicit `-T`.
            if payload.split(',').any(|value| value.starts_with('/')) {
                return true;
            }
            if payload == "--sysroot"
                || payload.starts_with("--sysroot=")
                || payload == "-T"
                || payload.starts_with("-T,")
                || payload.starts_with("-T=")
                || payload == "--script"
                || payload.starts_with("--script,")
                || payload.starts_with("--script=")
            {
                return true;
            }
        }

        // Clang's -Xlinker consumes exactly one linker argument.  A following
        // -Xlinker is commonly used to pass the value of a two-token option.
        if arg == "-Xlinker" {
            let Some(payload) = args.get(index + 1).map(String::as_str) else {
                return true;
            };
            if matches!(payload, "-rpath-link" | "--rpath-link") {
                index += if args.get(index + 2).is_some_and(|arg| arg == "-Xlinker") {
                    4
                } else {
                    3
                };
                continue;
            }
            if payload.starts_with("-rpath-link=") || payload.starts_with("--rpath-link=") {
                index += 2;
                continue;
            }
            if payload.starts_with('/')
                || matches!(payload, "--sysroot" | "-T" | "--script")
                || payload.starts_with("--sysroot=")
                || (payload.starts_with("-T") && payload.len() > 2)
                || payload.starts_with("--script=")
            {
                return true;
            }
            index += 2;
            continue;
        }
        index += 1;
    }
    false
}

fn is_optimization_level_arg(arg: &str) -> bool {
    matches!(arg, "-O" | "-Ofast" | "-Og" | "-Os" | "-Oz")
        || arg.strip_prefix("-O").is_some_and(|level| {
            !level.is_empty() && level.chars().all(|character| character.is_ascii_digit())
        })
}

fn has_whole_archive_option(args: &[String]) -> bool {
    args.iter().enumerate().any(|(index, arg)| {
        arg.strip_prefix("-Wl,")
            .is_some_and(|options| options.split(',').any(|option| option == "--whole-archive"))
            || (arg == "-Xlinker"
                && args
                    .get(index + 1)
                    .is_some_and(|option| option == "--whole-archive"))
    })
}

/// Extract archive paths embedded in one GCC `-Wl,` argument while GNU ld's
/// `--whole-archive` mode is active.  Libtool commonly emits its convenience
/// archives this way rather than as separate argv entries, e.g.
/// `-Wl,--whole-archive,a.a,b.a,--no-whole-archive`.
fn whole_archives_in_wl_arg(arg: &str) -> Vec<PathBuf> {
    let Some(options) = arg.strip_prefix("-Wl,") else {
        return Vec::new();
    };

    let mut whole_archive_active = false;
    let mut archives = Vec::new();
    for option in options.split(',') {
        match option {
            "--whole-archive" => whole_archive_active = true,
            "--no-whole-archive" => whole_archive_active = false,
            _ if whole_archive_active && option.ends_with(".a") => {
                archives.push(PathBuf::from(option));
            }
            _ => {}
        }
    }
    archives
}

/// Return the literal input path followed by its resolved target, if the
/// input is a symlink. Libtool expands convenience archives into temporary
/// `.lax` symlinks at final-link time, whereas their bitcode remains beside
/// the original object.
fn ir_lookup_paths(path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![path.to_path_buf()];
    if let Ok(canonical) = path.canonicalize() {
        if canonical != path {
            paths.push(canonical);
        }
    }
    paths
}

fn add_missing_ir_file(files: &mut Vec<PathBuf>, path: PathBuf) {
    if !files.contains(&path) {
        files.push(path);
    }
}

fn merge_missing_ir_file(files: &mut Vec<PathBuf>, sidecar_path: &Path) {
    let content = match fs::read_to_string(sidecar_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            eprintln!(
                "Warning: Failed to read LLVM IR missing-member file {}: {}",
                sidecar_path.display(),
                error
            );
            return;
        }
    };

    for path in content.lines().filter(|path| !path.is_empty()) {
        add_missing_ir_file(files, PathBuf::from(path));
    }
}

fn write_missing_ir_file(output: &Path, files: &[PathBuf]) -> std::io::Result<()> {
    let sidecar_path = PathBuf::from(format!("{}_missing", output.display()));
    if files.is_empty() {
        match fs::remove_file(sidecar_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    } else {
        let mut content = String::new();
        for file in files {
            content.push_str(&file.display().to_string());
            content.push('\n');
        }
        fs::write(&sidecar_path, content)
    }
}

fn run_llvm_link(
    llvm_link: &Path,
    output: &Path,
    inputs: &[PathBuf],
    only_needed: bool,
    log_file: &File,
) -> std::io::Result<()> {
    let mut command = Command::new(llvm_link);
    if only_needed {
        command.arg("--only-needed");
    }
    command.arg("-o").arg(output).args(inputs);

    let mut command_parts = vec![llvm_link.display().to_string()];
    if only_needed {
        command_parts.push("--only-needed".to_string());
    }
    command_parts.push("-o".to_string());
    command_parts.push(output.display().to_string());
    command_parts.extend(inputs.iter().map(|input| input.display().to_string()));
    writeln!(
        &mut log_file.try_clone()?,
        "Command: {}",
        command_parts.join(" ")
    )?;

    command.stdout(Stdio::from(log_file.try_clone()?));
    command.stderr(Stdio::from(log_file.try_clone()?));
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "llvm-link exited with status {}",
            status
        )))
    }
}

/// Copy whole archives that duplicate a direct input and remove only byte-for-
/// byte identical members.  Libtool may put an object's IR in both the direct
/// link inputs and a convenience archive (GSL's `version.o` does this).  GNU
/// ld avoids the duplicate while expanding the archive; llvm-link reports a
/// multiply-defined global if fed both copies.
fn filtered_whole_archives(
    llvm_ar: &Path,
    output: &Path,
    direct_inputs: &[PathBuf],
    whole_archives: &[PathBuf],
) -> std::io::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let direct_members: Vec<(String, Vec<u8>)> = direct_inputs
        .iter()
        .filter_map(|input| {
            Some((
                input.file_name()?.to_str()?.to_string(),
                fs::read(input).ok()?,
            ))
        })
        .collect();
    let mut filtered = Vec::with_capacity(whole_archives.len());
    let mut temporary_archives = Vec::new();

    for (index, archive) in whole_archives.iter().enumerate() {
        let members = Command::new(llvm_ar).arg("t").arg(archive).output()?;
        if !members.status.success() {
            return Err(std::io::Error::other(format!(
                "{} failed to list {}",
                llvm_ar.display(),
                archive.display()
            )));
        }

        let mut duplicates = Vec::new();
        for member in String::from_utf8_lossy(&members.stdout).lines() {
            for (direct_name, direct_bytes) in &direct_members {
                if member != direct_name {
                    continue;
                }
                let member_bytes = Command::new(llvm_ar)
                    .arg("p")
                    .arg(archive)
                    .arg(member)
                    .output()?;
                if member_bytes.status.success() && member_bytes.stdout == *direct_bytes {
                    duplicates.push(member.to_string());
                }
            }
        }

        if duplicates.is_empty() {
            filtered.push(archive.clone());
            continue;
        }

        let temporary = PathBuf::from(format!("{}.whole-archive-{index}", output.display()));
        fs::copy(archive, &temporary)?;
        let status = Command::new(llvm_ar)
            .arg("d")
            .arg(&temporary)
            .args(&duplicates)
            .status()?;
        if !status.success() {
            let _ = fs::remove_file(&temporary);
            return Err(std::io::Error::other(format!(
                "{} failed to remove duplicate members from {}",
                llvm_ar.display(),
                archive.display()
            )));
        }
        filtered.push(temporary.clone());
        temporary_archives.push(temporary);
    }

    Ok((filtered, temporary_archives))
}

#[derive(Default)]
struct BitcodeSymbols {
    defined: HashSet<String>,
    undefined: HashSet<String>,
}

struct ArchiveMember {
    path: PathBuf,
    symbols: BitcodeSymbols,
}

fn read_bitcode_symbols(nm: &Path, input: &Path) -> std::io::Result<BitcodeSymbols> {
    let output = Command::new(nm)
        .args(["--extern-only", "--format=posix"])
        .arg(input)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "{} failed to inspect {}",
            nm.display(),
            input.display()
        )));
    }

    let mut symbols = BitcodeSymbols::default();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else { continue };
        let Some(kind) = fields.next() else { continue };
        if kind.eq_ignore_ascii_case("U") {
            symbols.undefined.insert(name.to_string());
        } else {
            symbols.defined.insert(name.to_string());
        }
    }
    Ok(symbols)
}

fn extract_archive_members(
    llvm_ar: &Path,
    nm: &Path,
    archive: &Path,
    directory: &Path,
) -> std::io::Result<Vec<ArchiveMember>> {
    fs::create_dir_all(directory)?;
    let listed = Command::new(llvm_ar).arg("t").arg(archive).output()?;
    if !listed.status.success() {
        return Err(std::io::Error::other(format!(
            "{} failed to list {}",
            llvm_ar.display(),
            archive.display()
        )));
    }

    let mut members = Vec::new();
    for (index, name) in String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter(|name| !name.is_empty())
        .enumerate()
    {
        let path = directory.join(format!("{index}-{}", Path::new(name).display()));
        let extracted = Command::new(llvm_ar)
            .arg("p")
            .arg(archive)
            .arg(name)
            .output()?;
        if !extracted.status.success() {
            return Err(std::io::Error::other(format!(
                "{} failed to extract {} from {}",
                llvm_ar.display(),
                name,
                archive.display()
            )));
        }
        fs::write(&path, extracted.stdout)?;
        members.push(ArchiveMember {
            symbols: read_bitcode_symbols(nm, &path)?,
            path,
        });
    }
    Ok(members)
}

fn add_module_symbols(symbols: &mut BitcodeSymbols, module: BitcodeSymbols) {
    for name in module.defined {
        symbols.undefined.remove(&name);
        symbols.defined.insert(name);
    }
    for name in module.undefined {
        if !symbols.defined.contains(&name) {
            symbols.undefined.insert(name);
        }
    }
}

/// Select archive members using native-linker-like unresolved-symbol state.
/// Archives may be rescanned for dependency closure, but a selected member is
/// emitted only once. This prevents repeated llvm-link passes from duplicating
/// C++ global constructors while preserving cross-archive dependencies.
fn select_archive_members(
    llvm_ar: &Path,
    nm: &Path,
    archives: &[PathBuf],
    direct_inputs: &[PathBuf],
    work_directory: &Path,
) -> std::io::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut symbols = BitcodeSymbols::default();
    for input in direct_inputs {
        add_module_symbols(&mut symbols, read_bitcode_symbols(nm, input)?);
    }

    let mut archive_members = Vec::new();
    let mut extracted_directories = Vec::new();
    for (index, archive) in archives.iter().enumerate() {
        let directory = work_directory.join(format!("archive-{index}"));
        archive_members.push(extract_archive_members(llvm_ar, nm, archive, &directory)?);
        extracted_directories.push(directory);
    }

    let mut selected = HashSet::new();
    let mut selected_paths = Vec::new();
    loop {
        let mut changed = false;
        for members in &archive_members {
            for member in members {
                let member_id = member.path.clone();
                if selected.contains(&member_id)
                    || member.symbols.defined.is_disjoint(&symbols.undefined)
                {
                    continue;
                }
                selected.insert(member_id.clone());
                selected_paths.push(member_id);
                add_module_symbols(
                    &mut symbols,
                    BitcodeSymbols {
                        defined: member.symbols.defined.clone(),
                        undefined: member.symbols.undefined.clone(),
                    },
                );
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    Ok((selected_paths, extracted_directories))
}

/// Merge direct bitcode and LLVM static archives with ELF-like archive member
/// selection. llvm-link only performs one left-to-right archive scan, so repeat
/// it once per archive to resolve dependencies introduced by later archives.
fn merge_llvm_ir(
    llvm_link: &Path,
    output: &Path,
    direct_inputs: &[PathBuf],
    static_archives: &[PathBuf],
    whole_archives: &[PathBuf],
    log_file: &File,
) -> std::io::Result<()> {
    if static_archives.is_empty() && whole_archives.is_empty() {
        return run_llvm_link(llvm_link, output, direct_inputs, false, log_file);
    }

    let llvm_ar = find_llvm_tool(
        "llvm-ar",
        llvm_link
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("llvm-link"),
    );
    let (whole_archives, temporary_archives) =
        filtered_whole_archives(&llvm_ar, output, direct_inputs, whole_archives)?;
    let work_directory = PathBuf::from(format!("{}.archive-members", output.display()));
    let cleanup = || {
        let _ = fs::remove_dir_all(&work_directory);
        for archive in &temporary_archives {
            let _ = fs::remove_file(archive);
        }
    };
    cleanup();

    let result = (|| {
        // A link target normally has direct object/source input. If it does
        // not, retain llvm-link's old behavior rather than manufacturing an
        // empty seed that could change shared-library semantics.
        let nm = find_llvm_tool(
            "llvm-nm",
            llvm_link
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("llvm-link"),
        );
        let (selected_members, _) = select_archive_members(
            &llvm_ar,
            &nm,
            static_archives,
            direct_inputs,
            &work_directory,
        )?;
        let mut inputs = direct_inputs.to_vec();
        inputs.extend(selected_members);
        if !whole_archives.is_empty() {
            let mut whole_members = Vec::new();
            for (index, archive) in whole_archives.iter().enumerate() {
                let members = extract_archive_members(
                    &llvm_ar,
                    &nm,
                    archive,
                    &work_directory.join(format!("whole-{index}")),
                )?;
                whole_members.extend(members.into_iter().map(|member| member.path));
            }
            inputs.extend(whole_members);
        }
        run_llvm_link(llvm_link, output, &inputs, false, log_file)?;
        Ok(())
    })();
    cleanup();
    result
}

/// Handle link command (generate executable or shared library)
fn handle_link_command(
    clang_path: &Path,
    args: &[String],
    llvm_ir_dir: &str,
    _emit_llvmir_opt: Option<&str>,
    debug_mode: bool,
) -> ! {
    // Parse arguments
    let mut object_files: Vec<PathBuf> = Vec::new();
    let mut source_files: Vec<PathBuf> = Vec::new();
    let mut library_files: Vec<PathBuf> = Vec::new();
    let mut whole_archive_files: Vec<PathBuf> = Vec::new();
    let mut output_file: Option<PathBuf> = None;
    let mut other_args: Vec<String> = Vec::new();
    let mut library_dirs: Vec<String> = Vec::new();
    let mut is_shared = false;
    let mut version_script: Option<PathBuf> = None;
    let mut dynamic_list: Option<PathBuf> = None;
    let mut soversion: Option<String> = None;
    let mut whole_archive_active = false;
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];

        if arg == "-o" {
            if i + 1 < args.len() {
                output_file = Some(PathBuf::from(&args[i + 1]));
                i += 2;
                continue;
            }
        } else if arg.starts_with("-o") && arg.len() > 2 {
            output_file = Some(PathBuf::from(&arg[2..]));
        } else if arg == "-shared" {
            is_shared = true;
        } else if arg == "-L" {
            if i + 1 < args.len() {
                library_dirs.push(args[i + 1].clone());
                i += 2;
                continue;
            }
        } else if arg.starts_with("-L") && arg.len() > 2 {
            let dir = &arg[2..];
            library_dirs.push(dir.to_string());
        } else if arg == "-l" {
            if i + 1 < args.len() {
                i += 2;
                continue;
            }
        } else if arg.starts_with("-l") && arg.len() > 2 {
        } else if arg.ends_with(".o") && !arg.starts_with('-') {
            object_files.push(PathBuf::from(arg));
        } else if !arg.starts_with('-') && SOURCE_EXTENSIONS.iter().any(|ext| arg.ends_with(ext)) {
            source_files.push(PathBuf::from(arg));
        } else if !arg.starts_with('-')
            && (arg.ends_with(".so") || arg.contains(".so.") || arg.ends_with(".a"))
        {
            let library_file = PathBuf::from(arg);
            if whole_archive_active && library_file.extension().is_some_and(|ext| ext == "a") {
                whole_archive_files.push(library_file);
            } else {
                library_files.push(library_file);
            }
        } else if arg.starts_with("-Wl,") || arg.starts_with("-framework") {
            let normalized_arg = if let Some(path) = arg.strip_prefix("-Wl,-version-script,") {
                format!("-Wl,--version-script={}", path)
            } else if let Some(path) = arg.strip_prefix("-Wl,--version-script,") {
                format!("-Wl,--version-script={}", path)
            } else if arg == "-Wl,-version-script" || arg == "-Wl,--version-script" {
                if i + 1 < args.len() && args[i + 1].starts_with("-Wl,") {
                    let next_arg = &args[i + 1];
                    let path = next_arg.strip_prefix("-Wl,").unwrap_or(next_arg);
                    i += 1;
                    format!("-Wl,--version-script={}", path)
                } else {
                    arg.clone()
                }
            } else if arg == "-Wl,-soname" {
                if i + 1 < args.len() && args[i + 1].starts_with("-Wl,") {
                    let next_arg = &args[i + 1];
                    let soname = next_arg.strip_prefix("-Wl,").unwrap_or(next_arg);
                    i += 1;
                    format!("-Wl,-soname,{}", soname)
                } else {
                    arg.clone()
                }
            } else {
                arg.clone()
            };
            other_args.push(normalized_arg.clone());

            // `-Wl,` splits its comma-separated payload into individual ld
            // argv entries. Libtool uses that facility to place convenience
            // archives *inside* the whole-archive span, leaving no standalone
            // `.a` token for the normal parser to see.
            whole_archive_files.extend(whole_archives_in_wl_arg(&normalized_arg));

            if normalized_arg == "-Wl,--whole-archive" {
                whole_archive_active = true;
            } else if normalized_arg == "-Wl,--no-whole-archive" {
                whole_archive_active = false;
            }

            if let Some(path) = normalized_arg.strip_prefix("-Wl,--version-script=") {
                version_script = Some(PathBuf::from(path));
            }
            if let Some(path) = normalized_arg.strip_prefix("-Wl,--dynamic-list=") {
                dynamic_list = Some(PathBuf::from(path));
            }

            if let Some(soname) = normalized_arg.strip_prefix("-Wl,-soname,") {
                if let Some(idx) = soname.rfind(".so.") {
                    let sover = &soname[idx + 4..];
                    soversion = Some(sover.to_string());
                }
            }
        } else {
            other_args.push(arg.clone());
        }

        i += 1;
    }

    let output_path = match &output_file {
        Some(path) => path.clone(),
        None => PathBuf::from("a.out"),
    };

    // First pass: normal linking
    debug_log(
        debug_mode,
        &format!(
            "[DEBUG] Normal linking: {} {}",
            clang_path.display(),
            args[1..].join(" ")
        ),
    );

    let status = Command::new(clang_path)
        .args(&args[1..])
        .status()
        .unwrap_or_else(|_| panic!("Failed to execute {}", clang_path.display()));

    if !status.success() {
        exit(status.code().unwrap_or(1));
    }

    // Second pass: use llvm-link to merge LLVM IR files
    let mut direct_llvm_ir_files: Vec<PathBuf> = Vec::new();
    let mut static_llvm_archives: Vec<PathBuf> = Vec::new();
    let mut whole_llvm_archives: Vec<PathBuf> = Vec::new();
    let mut temp_llvm_ir_files: Vec<PathBuf> = Vec::new();
    let mut missing_ir_files: Vec<PathBuf> = Vec::new();

    // 1. Find LLVM IR files corresponding to .o files
    for obj_file in &object_files {
        let abs_obj = get_absolute_path(obj_file);
        let ir_path = ir_lookup_paths(&abs_obj).into_iter().find_map(|candidate| {
            let mut ir_path = PathBuf::from(llvm_ir_dir);
            let rel_path = candidate.strip_prefix("/").unwrap_or(&candidate);
            ir_path.push(rel_path);
            ir_path.exists().then_some(ir_path)
        });

        if let Some(ir_path) = ir_path {
            direct_llvm_ir_files.push(ir_path);
        } else {
            add_missing_ir_file(&mut missing_ir_files, abs_obj.clone());
            debug_log(
                debug_mode,
                &format!(
                    "[DEBUG] Warning: LLVM IR file not found: {}",
                    abs_obj.display()
                ),
            );
        }
    }

    // 1.5 Find LLVM IR files corresponding to static libraries
    for lib_file in &library_files {
        if !lib_file.extension().map(|e| e == "a").unwrap_or(false) {
            continue;
        }

        if let Some(ir_path) = find_llvmir_file(lib_file, llvm_ir_dir) {
            debug_log(
                debug_mode,
                &format!(
                    "[DEBUG] Found LLVM IR for static library: {} -> {}",
                    lib_file.display(),
                    ir_path.display()
                ),
            );
            let sidecar_path = PathBuf::from(format!("{}_missing", ir_path.display()));
            static_llvm_archives.push(ir_path);
            merge_missing_ir_file(&mut missing_ir_files, &sidecar_path);
        } else {
            debug_log(
                debug_mode,
                &format!(
                    "[DEBUG] Warning: LLVM IR file not found for static library: {}",
                    lib_file.display()
                ),
            );
        }
    }

    // 2. Generate LLVM IR for source files
    for source_file in &source_files {
        let abs_source = get_absolute_path(source_file);

        let mut ir_path = PathBuf::from(llvm_ir_dir);
        let rel_source = abs_source.strip_prefix("/").unwrap_or(&abs_source);
        ir_path.push(rel_source);
        ir_path.set_extension("bc");

        if let Err(e) = ensure_dir_exists(&ir_path) {
            eprintln!("Failed to create LLVM IR output directory: {}", e);
            exit(1);
        }

        let tmp_suffix = generate_tmp_suffix();
        let tmp_ir_path = append_tmp_suffix(&ir_path, &tmp_suffix);

        let mut llvm_gen_cmd = Command::new(clang_path);
        llvm_gen_cmd.args(&other_args);
        llvm_gen_cmd.arg("-emit-llvm");
        llvm_gen_cmd.arg("-c");
        llvm_gen_cmd.arg(source_file);
        llvm_gen_cmd.arg("-o").arg(&tmp_ir_path);

        {
            let mut cmd_parts = vec![clang_path.display().to_string()];
            cmd_parts.extend(other_args.clone());
            cmd_parts.push("-emit-llvm".to_string());
            cmd_parts.push("-c".to_string());
            cmd_parts.push(source_file.display().to_string());
            cmd_parts.push("-o".to_string());
            cmd_parts.push(tmp_ir_path.display().to_string());
            debug_log(
                debug_mode,
                &format!(
                    "[DEBUG] Generating LLVM IR for source: {}",
                    cmd_parts.join(" ")
                ),
            );
        }

        let llvm_gen_result = llvm_gen_cmd.status();

        match llvm_gen_result {
            Ok(status) if status.success() => {
                direct_llvm_ir_files.push(tmp_ir_path.clone());
                temp_llvm_ir_files.push(tmp_ir_path);
            }
            Ok(status) => {
                eprintln!("Failed to generate LLVM IR for {}", source_file.display());
                let _ = fs::remove_file(&tmp_ir_path);
                if output_path.exists() {
                    let _ = fs::remove_file(&output_path);
                }
                exit(status.code().unwrap_or(1));
            }
            Err(e) => {
                eprintln!("Failed to execute clang for LLVM IR generation: {}", e);
                let _ = fs::remove_file(&tmp_ir_path);
                if output_path.exists() {
                    let _ = fs::remove_file(&output_path);
                }
                exit(1);
            }
        }
    }

    for lib_file in &whole_archive_files {
        if let Some(ir_path) = find_llvmir_file(lib_file, llvm_ir_dir) {
            let sidecar_path = PathBuf::from(format!("{}_missing", ir_path.display()));
            merge_missing_ir_file(&mut missing_ir_files, &sidecar_path);
            whole_llvm_archives.push(ir_path);
        } else {
            debug_log(
                debug_mode,
                &format!(
                    "[DEBUG] Warning: LLVM IR file not found for whole static library: {}",
                    lib_file.display()
                ),
            );
        }
    }

    // Libtool's convenience archives are commonly passed via an argv shape
    // that records `--whole-archive` separately from the archive paths.  If
    // that shape was not associated with a concrete input above, preserving
    // normal archive selection would silently discard public API members.
    // Promote the collected archives as a conservative fallback; this keeps
    // the shared-library IR equivalent to the native link.
    if whole_llvm_archives.is_empty() && has_whole_archive_option(&args[1..]) {
        whole_llvm_archives.append(&mut static_llvm_archives);
    }

    if direct_llvm_ir_files.is_empty()
        && static_llvm_archives.is_empty()
        && whole_llvm_archives.is_empty()
    {
        debug_log(
            debug_mode,
            "[DEBUG] No LLVM IR files found, skipping llvm-link",
        );
        exit(0);
    }

    let abs_output = get_absolute_path(&output_path);

    let mut llvm_link_output = PathBuf::from(llvm_ir_dir);
    let rel_output = abs_output.strip_prefix("/").unwrap_or(&abs_output);
    llvm_link_output.push(rel_output);

    if let Err(e) = ensure_dir_exists(&llvm_link_output) {
        eprintln!("Failed to create LLVM IR output directory: {}", e);
        exit(1);
    }

    let clang_cmd_name = clang_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("clang");
    let llvm_link_path = find_llvm_tool("llvm-link", clang_cmd_name);

    let log_path = format!("{}_log", llvm_link_output.display());
    if let Err(e) = ensure_dir_exists(&PathBuf::from(&log_path)) {
        eprintln!("Failed to create log file directory: {}", e);
        exit(1);
    }

    let log_file = match File::create(&log_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create log file {}: {}", log_path, e);
            exit(1);
        }
    };

    debug_log(
        debug_mode,
        &format!(
            "[DEBUG] LLVM IR merge: {} direct input(s), {} selected static archive(s), {} whole static archive(s)",
            direct_llvm_ir_files.len(),
            static_llvm_archives.len(),
            whole_llvm_archives.len()
        ),
    );
    match merge_llvm_ir(
        &llvm_link_path,
        &llvm_link_output,
        &direct_llvm_ir_files,
        &static_llvm_archives,
        &whole_llvm_archives,
        &log_file,
    ) {
        Ok(()) => {
            if let Err(error) = write_missing_ir_file(&llvm_link_output, &missing_ir_files) {
                eprintln!(
                    "Failed to write LLVM IR missing-member file for {}: {}",
                    llvm_link_output.display(),
                    error
                );
                let _ = fs::remove_file(&llvm_link_output);
                if output_path.exists() {
                    let _ = fs::remove_file(&output_path);
                }
                exit(1);
            }

            for ir_file in &temp_llvm_ir_files {
                if let Err(e) = fs::remove_file(ir_file) {
                    eprintln!(
                        "Warning: Failed to remove intermediate file {}: {}",
                        ir_file.display(),
                        e
                    );
                }
            }

            let version_script_dest = if let Some(ref vs_path) = version_script {
                let output_name = output_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("output");

                let vs_filename = format!("{}_verscript", output_name);

                let vs_dest = llvm_link_output
                    .parent()
                    .map(|p| p.join(&vs_filename))
                    .unwrap_or_else(|| PathBuf::from(&vs_filename));

                if vs_path.exists() {
                    if let Err(e) = fs::copy(vs_path, &vs_dest) {
                        eprintln!("Warning: Failed to copy version script: {}", e);
                        None
                    } else {
                        debug_log(
                            debug_mode,
                            &format!(
                                "[DEBUG] Copied version script: {} -> {}",
                                vs_path.display(),
                                vs_dest.display()
                            ),
                        );
                        Some(vs_dest)
                    }
                } else {
                    eprintln!("Warning: Version script not found: {}", vs_path.display());
                    None
                }
            } else {
                None
            };

            let dynamic_list_dest = if let Some(ref list_path) = dynamic_list {
                let output_name = output_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("output");
                let list_filename = format!("{}_dynamiclist", output_name);
                let list_dest = llvm_link_output
                    .parent()
                    .map(|p| p.join(&list_filename))
                    .unwrap_or_else(|| PathBuf::from(&list_filename));
                if list_path.exists() {
                    if let Err(e) = fs::copy(list_path, &list_dest) {
                        eprintln!("Warning: Failed to copy dynamic list: {}", e);
                        None
                    } else {
                        debug_log(
                            debug_mode,
                            &format!(
                                "[DEBUG] Copied dynamic list: {} -> {}",
                                list_path.display(),
                                list_dest.display()
                            ),
                        );
                        Some(list_dest)
                    }
                } else {
                    eprintln!("Warning: Dynamic list not found: {}", list_path.display());
                    None
                }
            } else {
                None
            };

            if let Err(e) = generate_link_cmd_file(
                clang_path,
                &llvm_link_output,
                &output_path,
                &args[1..],
                &library_dirs,
                is_shared,
                version_script_dest.as_deref(),
                dynamic_list_dest.as_deref(),
                soversion.as_deref(),
                &missing_ir_files,
            ) {
                eprintln!("Failed to generate _cmd file: {}", e);
                let _ = fs::remove_file(&llvm_link_output);
                let _ = fs::remove_file(format!("{}_missing", llvm_link_output.display()));
                if output_path.exists() {
                    let _ = fs::remove_file(&output_path);
                }
                exit(1);
            }

            exit(0);
        }
        Err(error) => {
            eprintln!("llvm-link failed, see {} for details", log_path);
            if output_path.exists() {
                let _ = fs::remove_file(&output_path);
            }
            debug_log(
                debug_mode,
                &format!("[DEBUG] LLVM IR merge failed: {error}"),
            );
            exit(1);
        }
    }
}

/// Generate link command script (_cmd file)
#[allow(clippy::too_many_arguments)]
fn generate_link_cmd_file(
    clang_path: &Path,
    llvm_link_output: &Path,
    original_output: &Path,
    original_args: &[String],
    library_dirs: &[String],
    is_shared: bool,
    version_script_dest: Option<&Path>,
    dynamic_list_dest: Option<&Path>,
    soversion: Option<&str>,
    missing_ir_files: &[PathBuf],
) -> Result<(), std::io::Error> {
    let cmd_path = format!("{}_cmd", llvm_link_output.display());
    let normalized_args = normalize_linker_arguments(original_args);

    let bc_filename = llvm_link_output
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output.bc");

    let original_output_filename = original_output
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("a.out");

    let output_filename = if is_shared {
        if let Some(sov) = soversion {
            if let Some(idx) = original_output_filename.find(".so") {
                format!("{}.so.{}", &original_output_filename[..idx], sov)
            } else if let Some(idx) = original_output_filename.find(".dylib") {
                format!("{}.{}.dylib", &original_output_filename[..idx], sov)
            } else {
                original_output_filename.to_string()
            }
        } else {
            original_output_filename.to_string()
        }
    } else {
        original_output_filename.to_string()
    };

    let clang_version = get_clang_version(clang_path).unwrap_or_else(|| "unknown".to_string());
    let cmd_clang_path = precise_usr_bin_clang_path(clang_path, &clang_version);

    let mut script = String::new();
    script.push_str("#!/bin/bash\n");
    script.push_str("#\n");
    script.push_str("# Link command script for LLVM bitcode\n");
    script.push_str("#\n");
    script.push_str("# This script links LLVM bitcode files into executable or shared library\n");
    script.push_str("# using the same linker options as the original build. It allows\n");
    script.push_str("# re-linking LLVM bitcode without re-compilation.\n");
    script.push_str("#\n");
    script.push_str("# Generated by clang-wrap\n");
    script.push_str("# Clang version: ");
    script.push_str(&clang_version);
    script.push('\n');
    script.push_str("# Original output: ");
    script.push_str(&output_filename);
    script.push('\n');
    script.push_str("#\n");
    script.push('\n');
    script.push_str("set -e\n\n");

    // A non-relocatable linker path means no valid distributable `_cmd` can be
    // produced. Fail the current link instead of emitting an intentionally
    // broken command that would let the original build/install succeed.
    if has_nonportable_linker_path_option(&normalized_args) {
        let _ = fs::remove_file(&cmd_path);
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unsupported non-relocatable linker path option (sysroot or linker script)",
        ));
    }

    script.push_str("# Create output directory\n");
    script.push_str("mkdir -p output\n\n");

    script.push_str("# Link bitcode with original options\n");
    script.push_str("cmd=(\n");
    push_array_arg(&mut script, &cmd_clang_path.display().to_string());

    push_array_arg(&mut script, "-x");
    push_array_arg(&mut script, "ir");
    push_array_arg(&mut script, "-fuse-ld=lld");
    // Installed executable commands live in lib/llvmir-bin, while shared
    // libraries from the same package live in the parent lib directory.
    // Preserve this package-local lookup path after normalizing libraries to
    // -l:SONAME arguments.
    push_array_arg(&mut script, "-L..");

    let has_optimization_level = normalized_args
        .iter()
        .any(|arg| is_optimization_level_arg(arg));
    if !has_optimization_level {
        push_array_arg(&mut script, "-O3");
    }

    let mut emitted_bitcode = false;
    let mut visited_linker_scripts = Vec::new();
    let mut index = 0;
    while index < normalized_args.len() {
        let arg = &normalized_args[index];
        let next = normalized_args.get(index + 1);
        if arg == "-o" {
            index += 2;
            continue;
        }
        if arg.starts_with("-o") && arg.len() > 2 {
            index += 1;
            continue;
        }
        if arg == "-c" || arg == "-emit-llvm" || arg == "-ffat-lto-objects" {
            index += 1;
            continue;
        }
        if compiler_only_option_consumes_next(arg) {
            index += usize::from(next.is_some()) + 1;
            continue;
        }
        if is_compiler_only_option(arg) {
            index += 1;
            continue;
        }
        if arg == "-L" {
            if next.is_some() {
                index += 2;
                continue;
            }
        }
        if arg.starts_with("-L") && arg.len() > 2 {
            // Libraries are converted to exact -l:SONAME references below.
            // Retaining an original search directory would leak the build
            // layout into the generated, installable command.
            index += 1;
            continue;
        }
        if arg == "-Xlinker" {
            let Some(payload) = next.map(String::as_str) else {
                index += 1;
                continue;
            };
            if payload.starts_with("--dependency-file=")
                || payload.starts_with("-rpath-link=")
                || payload.starts_with("--rpath-link=")
            {
                index += 2;
                continue;
            }
            if matches!(
                payload,
                "--dependency-file" | "-rpath-link" | "--rpath-link"
            ) {
                // The value of the two-token option may be forwarded through
                // a second -Xlinker.
                if normalized_args
                    .get(index + 2)
                    .is_some_and(|arg| arg == "-Xlinker")
                {
                    4
                } else {
                    3
                };
                continue;
            }
        }
        if is_rpath_link_arg(arg) {
            index += if matches!(arg.as_str(), "-rpath-link" | "--rpath-link") {
                usize::from(next.is_some()) + 1
            } else {
                1
            };
            continue;
        }
        if arg.starts_with("-Wl,--dependency-file,") || arg.starts_with("-Wl,--dependency-file=") {
            index += 1;
            continue;
        }
        if arg == "-Wl,--dependency-file" {
            index += 2;
            continue;
        }
        if arg == "-l" {
            if let Some(library) = next {
                if let Some(rewritten) = resolved_library_link_args(
                    library,
                    library_dirs,
                    clang_path,
                    &mut visited_linker_scripts,
                ) {
                    for argument in rewritten {
                        push_array_arg(&mut script, &argument);
                    }
                } else {
                    push_array_arg(&mut script, "-l");
                    push_array_arg(&mut script, library);
                }
                index += 2;
                continue;
            }
        }
        if let Some(library) = arg.strip_prefix("-l").filter(|library| !library.is_empty()) {
            if let Some(rewritten) = resolved_library_link_args(
                library,
                library_dirs,
                clang_path,
                &mut visited_linker_scripts,
            ) {
                for argument in rewritten {
                    push_array_arg(&mut script, &argument);
                }
            } else {
                push_array_arg(&mut script, arg);
            }
            index += 1;
            continue;
        }
        if is_rpath_arg(arg) {
            let value = if rpath_arg_consumes_next(arg) {
                next.map(String::as_str)
            } else {
                Some(arg.as_str())
            };
            let is_portable = value.is_some_and(|value| value.contains("$ORIGIN"));
            if is_portable {
                push_array_arg(&mut script, arg);
                if rpath_arg_consumes_next(arg) {
                    push_array_arg(&mut script, next.expect("rpath value checked"));
                    index += 2;
                    continue;
                }
            } else if rpath_arg_consumes_next(arg) {
                index += 2;
                continue;
            }
            index += 1;
            continue;
        }
        if arg.starts_with("-Wl,--version-script=")
            || arg.starts_with("-Wl,-version-script,")
            || arg.starts_with("-Wl,-version-script=")
        {
            if let Some(path) = version_script_dest
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
            {
                push_array_arg(&mut script, &format!("-Wl,--version-script=./{path}"));
            }
            index += 1;
            continue;
        }
        if arg.starts_with("-Wl,--dynamic-list=") || arg.starts_with("-Wl,--dynamic-list,") {
            if let Some(path) = dynamic_list_dest
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
            {
                push_array_arg(&mut script, &format!("-Wl,--dynamic-list=./{path}"));
            }
            index += 1;
            continue;
        }
        if arg == "-Wl,-version-script" || arg == "-Wl,--version-script" {
            if let Some(path) = version_script_dest
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
            {
                push_array_arg(&mut script, &format!("-Wl,--version-script=./{path}"));
            }
            // Libtool commonly passes the script path as a separate
            // `-Wl,<path>` argument. It is replaced by the copied sidecar.
            index += usize::from(next.is_some()) + 1;
            continue;
        }
        let is_native_input = !arg.starts_with('-')
            && (arg.ends_with(".o")
                || SOURCE_EXTENSIONS
                    .iter()
                    .any(|extension| arg.ends_with(extension))
                || arg.ends_with(".a"));
        if is_native_input {
            if !emitted_bitcode {
                push_array_arg(&mut script, &format!("./{bc_filename}"));
                for missing_path in missing_ir_files {
                    push_array_arg(&mut script, &format!("-Wl,{}", missing_path.display()));
                }
                emitted_bitcode = true;
            }
            index += 1;
            continue;
        }
        if !arg.starts_with('-') && (arg.ends_with(".so") || arg.contains(".so.")) {
            if let Some(rewritten) = exact_link_arg_for_library_path(Path::new(arg)) {
                push_array_arg(&mut script, &rewritten);
            } else {
                push_array_arg(&mut script, arg);
            }
            index += 1;
            continue;
        }
        push_array_arg(&mut script, arg);
        index += 1;
    }
    if !emitted_bitcode {
        push_array_arg(&mut script, &format!("./{bc_filename}"));
        for missing_path in missing_ir_files {
            push_array_arg(&mut script, &format!("-Wl,{}", missing_path.display()));
        }
    }

    push_array_arg(&mut script, &format!("--output=output/{}", output_filename));
    script.push_str(")\n\n");

    script.push_str("# Execute link command\n");
    script.push_str("\"${cmd[@]}\"\n");

    let mut file = File::create(&cmd_path)?;
    file.write_all(script.as_bytes())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&cmd_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&cmd_path, perms)?;
    }

    Ok(())
}

fn main() {
    let original_args: Vec<String> = env::args().collect();

    // Get program name
    let program_name = get_program_name(&original_args);

    // Determine the actual clang executable to invoke
    let clang_cmd = if program_name == "clang-wrap" {
        "clang"
    } else if program_name == "clangxx" {
        "clang++"
    } else {
        program_name
    };

    // Get the real clang path (skip self)
    let clang_path = get_exe_path(clang_cmd);

    if original_args.len() < 2 {
        let status = Command::new(&clang_path)
            .status()
            .unwrap_or_else(|_| panic!("Failed to execute {}", clang_path.display()));
        exit(status.code().unwrap_or(1));
    }

    let debug_mode = is_debug_mode();

    let args = match expand_at_file_args(&original_args, debug_mode) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("Failed to expand response file: {error}");
            exit(1);
        }
    };

    let emit_llvmir_opt = get_emit_llvmir_opt();

    let llvm_ir_dir = get_llvm_ir_dir();

    // Initialize debug log
    if debug_mode {
        init_debug_log(&llvm_ir_dir);
    }

    // Check for -c and -E options
    let compile_flag_pos = args.iter().position(|arg| arg == "-c");
    let preprocess_flag_pos = args.iter().position(|arg| arg == "-E");

    let (has_compile_flag, has_preprocess_flag) = match (compile_flag_pos, preprocess_flag_pos) {
        (Some(c_pos), Some(e_pos)) => {
            if c_pos > e_pos {
                (true, false)
            } else {
                (false, true)
            }
        }
        (Some(_), None) => (true, false),
        (None, Some(_)) => (false, true),
        (None, None) => (false, false),
    };

    let has_shared_flag = args.iter().any(|arg| arg == "-shared");
    let has_object_inputs = args[1..]
        .iter()
        .any(|arg| arg.ends_with(".o") && !arg.starts_with('-'));

    let has_source_inputs = args[1..]
        .iter()
        .any(|arg| !arg.starts_with('-') && SOURCE_EXTENSIONS.iter().any(|ext| arg.ends_with(ext)));

    let is_link_command =
        !has_compile_flag && (has_object_inputs || has_shared_flag || has_source_inputs);

    if has_preprocess_flag {
        let status = Command::new(&clang_path)
            .args(&args[1..])
            .status()
            .unwrap_or_else(|_| panic!("Failed to execute {}", clang_path.display()));
        exit(status.code().unwrap_or(1));
    }

    if !has_compile_flag && !is_link_command {
        let status = Command::new(&clang_path)
            .args(&args[1..])
            .status()
            .unwrap_or_else(|_| panic!("Failed to execute {}", clang_path.display()));
        exit(status.code().unwrap_or(1));
    }

    if is_link_command {
        if emit_llvmir_opt.is_none() {
            let status = Command::new(&clang_path)
                .args(&args[1..])
                .status()
                .unwrap_or_else(|_| panic!("Failed to execute {}", clang_path.display()));
            exit(status.code().unwrap_or(1));
        }
        handle_link_command(
            &clang_path,
            &args,
            &llvm_ir_dir,
            emit_llvmir_opt.as_deref(),
            debug_mode,
        );
    }

    // Parse compilation arguments
    let mut input_files: Vec<PathBuf> = Vec::new();
    let mut output_file: Option<PathBuf> = None;
    let mut other_args: Vec<String> = Vec::new();
    let mut i = 1;

    let options_with_arg = [
        "-MT",
        "-MF",
        "-MQ",
        "-MD",
        "-MMD",
        "-I",
        "-L",
        "-l",
        "-D",
        "-U",
        "-include",
        "-imacros",
        "-idirafter",
        "-iprefix",
        "-iwithprefix",
        "-iwithprefixbefore",
        "-isystem",
        "-iquote",
        "-isysroot",
        "-framework",
        "-F",
        "-x",
        "-std",
        "-arch",
        "-target",
        "-mllvm",
        "--param",
        "-Xclang",
    ];

    while i < args.len() {
        let arg = &args[i];

        if is_ir_incompatible_option(arg) {
            i += 1;
            continue;
        }

        if arg == "-o" {
            if i + 1 < args.len() {
                output_file = Some(PathBuf::from(&args[i + 1]));
                i += 2;
                continue;
            }
        } else if arg.starts_with("-o") && arg.len() > 2 {
            output_file = Some(PathBuf::from(&arg[2..]));
        } else if options_with_arg.contains(&arg.as_str()) {
            other_args.push(arg.clone());
            if i + 1 < args.len() {
                other_args.push(args[i + 1].clone());
                i += 2;
                continue;
            }
        } else if !arg.starts_with('-')
            && SOURCE_EXTENSIONS
                .iter()
                .any(|extension| arg.ends_with(extension))
        {
            input_files.push(PathBuf::from(arg));
        } else {
            other_args.push(arg.clone());
        }

        i += 1;
    }

    // First pass: normal compilation
    let mut normal_cmd = Command::new(&clang_path);
    normal_cmd.args(&args[1..]);

    debug_log(
        debug_mode,
        &format!(
            "[DEBUG] Normal compilation: {} {}",
            clang_path.display(),
            args[1..].join(" ")
        ),
    );

    let normal_result = normal_cmd.status();

    match normal_result {
        Ok(status) if status.success() => {}
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Failed to execute normal compilation: {}", e);
            exit(1);
        }
    }

    // Second pass: generate LLVM IR
    if emit_llvmir_opt.is_none() {
        exit(0);
    }

    let extra_opts: Vec<String> = if let Some(extra) = emit_llvmir_opt.as_deref() {
        if extra == "1" {
            Vec::new()
        } else {
            extra.split_whitespace().map(|s| s.to_string()).collect()
        }
    } else {
        Vec::new()
    };

    for input_file in &input_files {
        let output_path = output_file.clone().unwrap_or_else(|| {
            let mut output = input_file.clone();
            output.set_extension("o");
            output
        });
        let abs_output = get_absolute_path(&output_path);
        let llvm_ir_path =
            PathBuf::from(&llvm_ir_dir).join(abs_output.strip_prefix("/").unwrap_or(&abs_output));
        if let Err(error) = ensure_dir_exists(&llvm_ir_path) {
            debug_log(
                debug_mode,
                &format!("[DEBUG] Failed to create LLVM IR output directory: {error}"),
            );
            exit(0);
        }

        let mut llvm_cmd = Command::new(&clang_path);
        llvm_cmd.args(&other_args);
        llvm_cmd.args(&extra_opts);
        llvm_cmd
            .arg("-emit-llvm")
            .arg(input_file)
            .arg("-o")
            .arg(&llvm_ir_path);

        let log_path = format!("{}_log", llvm_ir_path.display());
        if let Err(error) = ensure_dir_exists(&PathBuf::from(&log_path)) {
            debug_log(
                debug_mode,
                &format!("[DEBUG] Failed to create LLVM IR log directory: {error}"),
            );
            exit(0);
        }
        let mut log_file = match File::create(&log_path) {
            Ok(file) => file,
            Err(error) => {
                debug_log(
                    debug_mode,
                    &format!("[DEBUG] Failed to create LLVM IR log {log_path}: {error}"),
                );
                exit(0);
            }
        };
        let mut cmd_parts = other_args.clone();
        cmd_parts.extend(extra_opts.clone());
        cmd_parts.extend([
            "-emit-llvm".to_string(),
            input_file.display().to_string(),
            "-o".to_string(),
            llvm_ir_path.display().to_string(),
        ]);
        debug_log(
            debug_mode,
            &format!(
                "[DEBUG] LLVM IR generation: {} {}",
                clang_path.display(),
                cmd_parts.join(" ")
            ),
        );
        let _ = writeln!(
            log_file,
            "Command: {} {}",
            clang_path.display(),
            cmd_parts.join(" ")
        );
        llvm_cmd.stdout(Stdio::from(log_file.try_clone().unwrap()));
        llvm_cmd.stderr(Stdio::from(log_file));

        match llvm_cmd.status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                let _ = fs::remove_file(&llvm_ir_path);
                if output_path.exists() {
                    let _ = fs::remove_file(&output_path);
                }
                eprintln!(
                    "LLVM IR generation failed for {}, see {} for details",
                    input_file.display(),
                    log_path
                );
                exit(status.code().unwrap_or(1));
            }
            Err(error) => {
                debug_log(
                    debug_mode,
                    &format!("[DEBUG] Failed to execute LLVM IR generation: {error}"),
                );
                let _ = fs::remove_file(&llvm_ir_path);
                exit(0);
            }
        }
    }
}
