// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! clang/clang++ wrapper
//!
//! Generate LLVM IR during compilation, use llvm-link to merge LLVM IR files during linking

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
            &[],
        )
        .unwrap();

        let script = fs::read_to_string(format!("{}_cmd", bitcode.display())).unwrap();
        assert!(script.contains("-Wl,--version-script=./program_verscript"));
        assert!(!script.contains(".libs/program.ver"));
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
            &[],
        )
        .unwrap();

        let script = fs::read_to_string(format!("{}_cmd", bitcode.display())).unwrap();
        assert!(!script.contains("dependency-file"));
        assert!(!script.contains("link.d"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn static_archives_are_repeatedly_linked_with_only_needed() {
        let dir = unique_temp_dir("llvm-link-archive-selection");
        let linker = dir.join("fake-llvm-link");
        let invocation_log = dir.join("invocations");
        let output = dir.join("merged.bc");
        let direct = dir.join("main.bc");
        let archive_a = dir.join("liba.a");
        let archive_b = dir.join("libb.a");
        let log_file = File::create(dir.join("merge.log")).unwrap();
        fs::write(&direct, "direct").unwrap();
        fs::write(&archive_a, "archive a").unwrap();
        fs::write(&archive_b, "archive b").unwrap();
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
            &log_file,
        )
        .unwrap();

        let invocations = fs::read_to_string(&invocation_log).unwrap();
        let lines = invocations.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3, "{invocations}");
        assert!(!lines[0].contains("--only-needed"));
        for line in &lines[1..] {
            assert!(line.starts_with("--only-needed -o "), "{line}");
            assert!(line.contains(archive_a.to_str().unwrap()), "{line}");
            assert!(line.contains(archive_b.to_str().unwrap()), "{line}");
        }
        assert!(output.exists());
        assert!(!PathBuf::from(format!("{}.direct-input", output.display())).exists());
        assert!(!PathBuf::from(format!("{}.static-archive-current", output.display())).exists());
        assert!(!PathBuf::from(format!("{}.static-archive-next", output.display())).exists());
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

/// Merge direct bitcode and LLVM static archives with ELF-like archive member
/// selection. llvm-link only performs one left-to-right archive scan, so repeat
/// it once per archive to resolve dependencies introduced by later archives.
fn merge_llvm_ir(
    llvm_link: &Path,
    output: &Path,
    direct_inputs: &[PathBuf],
    static_archives: &[PathBuf],
    log_file: &File,
) -> std::io::Result<()> {
    if static_archives.is_empty() {
        return run_llvm_link(llvm_link, output, direct_inputs, false, log_file);
    }

    let seed = PathBuf::from(format!("{}.direct-input", output.display()));
    let current = PathBuf::from(format!("{}.static-archive-current", output.display()));
    let next = PathBuf::from(format!("{}.static-archive-next", output.display()));
    let cleanup = || {
        let _ = fs::remove_file(&seed);
        let _ = fs::remove_file(&current);
        let _ = fs::remove_file(&next);
    };
    cleanup();

    let result = (|| {
        // A link target normally has direct object/source input. If it does
        // not, retain llvm-link's old behavior rather than manufacturing an
        // empty seed that could change shared-library semantics.
        if direct_inputs.is_empty() {
            return run_llvm_link(llvm_link, output, static_archives, false, log_file);
        }
        run_llvm_link(llvm_link, &seed, direct_inputs, false, log_file)?;
        fs::rename(&seed, &current)?;
        for _ in 0..static_archives.len() {
            let mut inputs = Vec::with_capacity(static_archives.len() + 1);
            inputs.push(current.clone());
            inputs.extend_from_slice(static_archives);
            run_llvm_link(llvm_link, &next, &inputs, true, log_file)?;
            fs::rename(&next, &current)?;
        }
        fs::rename(&current, output)?;
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
    let mut output_file: Option<PathBuf> = None;
    let mut other_args: Vec<String> = Vec::new();
    let mut library_dirs: Vec<String> = Vec::new();
    let mut is_shared = false;
    let mut version_script: Option<PathBuf> = None;
    let mut soversion: Option<String> = None;
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
            library_files.push(PathBuf::from(arg));
        } else if arg.starts_with("-Wl,") || arg.starts_with("-framework") {
            let normalized_arg = if let Some(path) = arg.strip_prefix("-Wl,-version-script,") {
                format!("-Wl,--version-script={}", path)
            } else if let Some(path) = arg.strip_prefix("-Wl,--version-script,") {
                format!("-Wl,--version-script={}", path)
            } else if arg == "-Wl,-version-script" {
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

            if let Some(path) = normalized_arg.strip_prefix("-Wl,--version-script=") {
                version_script = Some(PathBuf::from(path));
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
    let mut temp_llvm_ir_files: Vec<PathBuf> = Vec::new();
    let mut missing_ir_files: Vec<PathBuf> = Vec::new();

    // 1. Find LLVM IR files corresponding to .o files
    for obj_file in &object_files {
        let abs_obj = get_absolute_path(obj_file);

        let mut ir_path = PathBuf::from(llvm_ir_dir);
        let rel_path = abs_obj.strip_prefix("/").unwrap_or(&abs_obj);
        ir_path.push(rel_path);

        if ir_path.exists() {
            direct_llvm_ir_files.push(ir_path);
        } else {
            add_missing_ir_file(&mut missing_ir_files, abs_obj);
            debug_log(
                debug_mode,
                &format!(
                    "[DEBUG] Warning: LLVM IR file not found: {}",
                    ir_path.display()
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

    if direct_llvm_ir_files.is_empty() && static_llvm_archives.is_empty() {
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
            "[DEBUG] LLVM IR merge: {} direct input(s), {} static archive(s)",
            direct_llvm_ir_files.len(),
            static_llvm_archives.len()
        ),
    );
    match merge_llvm_ir(
        &llvm_link_path,
        &llvm_link_output,
        &direct_llvm_ir_files,
        &static_llvm_archives,
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

            if let Err(e) = generate_link_cmd_file(
                clang_path,
                &llvm_link_output,
                &output_path,
                &args[1..],
                &library_dirs,
                is_shared,
                version_script_dest.as_deref(),
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
    soversion: Option<&str>,
    missing_ir_files: &[PathBuf],
) -> Result<(), std::io::Error> {
    let cmd_path = format!("{}_cmd", llvm_link_output.display());

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
    if has_nonportable_linker_path_option(original_args) {
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

    let has_optimization_level = original_args
        .iter()
        .any(|arg| is_optimization_level_arg(arg));
    if !has_optimization_level {
        push_array_arg(&mut script, "-O3");
    }

    let mut emitted_bitcode = false;
    let mut visited_linker_scripts = Vec::new();
    let mut index = 0;
    while index < original_args.len() {
        let arg = &original_args[index];
        let next = original_args.get(index + 1);
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
                index += if original_args
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
        if arg.starts_with("-Wl,--version-script=") || arg.starts_with("-Wl,-version-script,") {
            if let Some(path) = version_script_dest
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
            {
                push_array_arg(&mut script, &format!("-Wl,--version-script=./{path}"));
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
