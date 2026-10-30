// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! ar/llvm-ar wrapper
//!
//! After executing ar command, automatically create a matching LLVM IR archive.

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::{exit, Command, Stdio};

use clang_wrap::{
    debug_log, ensure_dir_exists, find_llvm_tool, find_llvmir_file, get_exe_path, get_llvm_ir_dir,
    get_program_name, init_debug_log, is_debug_mode, is_emit_llvmir_enabled,
};

fn main() {
    let args: Vec<String> = env::args().collect();

    // Get program name
    let program_name = get_program_name(&args);

    // Determine the actual command to invoke
    let ar_cmd = if program_name == "ar-wrap" {
        "ar"
    } else if program_name == "llvm-ar-wrap" {
        "llvm-ar"
    } else {
        program_name
    };

    // Get the real ar/llvm-ar path (skip self)
    let ar_path = get_exe_path(ar_cmd);

    let debug = is_debug_mode();
    let emit_llvmir = is_emit_llvmir_enabled();
    let llvm_ir_dir = get_llvm_ir_dir();

    // Initialize debug log
    if debug {
        init_debug_log(&llvm_ir_dir);
    }

    debug_log(
        debug,
        &format!(
            "[DEBUG] Executing {}: {} {}",
            ar_path.display(),
            ar_cmd,
            args[1..].join(" ")
        ),
    );

    // Parse ar command arguments
    let mut is_create_or_replace = false;
    let mut archive_file: Option<PathBuf> = None;
    let mut member_files: Vec<PathBuf> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if arg.starts_with('-') {
            // GNU style options
            if arg.contains('c') || arg.contains('r') {
                is_create_or_replace = true;
            }

            if arg == "--plugin" {
                i += 2;
                continue;
            } else if arg.starts_with("--plugin=") {
                i += 1;
                continue;
            }
        } else if archive_file.is_none() {
            // First non-option argument
            let is_bsd_option = arg.chars().all(|c| c.is_ascii_alphabetic())
                && arg.len() <= 8
                && (arg.contains('c') || arg.contains('r'));

            if is_bsd_option {
                is_create_or_replace = true;
            } else {
                archive_file = Some(PathBuf::from(arg));
            }
        } else {
            member_files.push(PathBuf::from(arg));
        }

        i += 1;
    }

    // Execute ar/llvm-ar command
    let status = Command::new(&ar_path)
        .args(&args[1..])
        .status()
        .unwrap_or_else(|_| panic!("Failed to execute {}", ar_path.display()));

    if !status.success() {
        exit(status.code().unwrap_or(1));
    }

    // If not create/replace operation, or LLVM IR generation not enabled, exit directly
    if !is_create_or_replace || !emit_llvmir {
        exit(0);
    }

    // Find LLVM IR files corresponding to .o files. The LLVM archive must only
    // contain bitcode members: llvm-link rejects an archive that contains a
    // native object, so missing IR members are recorded in a sidecar instead.
    let mut llvm_ir_files: Vec<PathBuf> = Vec::new();
    let mut missing_ir_files: Vec<PathBuf> = Vec::new();

    for obj_file in &member_files {
        // Libtool compiles PIC objects with the `.os` suffix; both are native
        // object files that need a corresponding LLVM IR member.
        if !obj_file
            .extension()
            .map(|e| e == "o" || e == "os" || e == "lo")
            .unwrap_or(false)
        {
            continue;
        }

        if let Some(ir_path) = find_llvmir_file(obj_file, &llvm_ir_dir) {
            llvm_ir_files.push(ir_path);
        } else {
            let missing_path = if obj_file.is_absolute() {
                obj_file.clone()
            } else {
                env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(obj_file)
            };
            missing_ir_files.push(missing_path);
            debug_log(
                debug,
                &format!(
                    "[DEBUG] Warning: LLVM IR file not found for: {}",
                    obj_file.display()
                ),
            );
        }
    }

    // Get absolute path of archive file
    let archive_path = match &archive_file {
        Some(path) => {
            if path.is_absolute() {
                path.clone()
            } else {
                env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(path)
            }
        }
        None => {
            debug_log(debug, "[DEBUG] No archive file specified, skipping llvm-ar");
            exit(0);
        }
    };

    let mut llvm_archive_output = PathBuf::from(&llvm_ir_dir);
    let rel_output = archive_path.strip_prefix("/").unwrap_or(&archive_path);
    llvm_archive_output.push(rel_output);

    if let Err(e) = ensure_dir_exists(&llvm_archive_output) {
        eprintln!("Failed to create LLVM IR output directory: {}", e);
        exit(1);
    }

    // Libtool may pass native archive members through temporary `.lax`
    // symlinks. Those links are removed before install-wrap consumes the
    // `_missing` sidecar, so preserve a private copy while the member still
    // exists. The copy is only a build-tree input; install-wrap publishes it
    // under the final artifact's asm/ directory and removes the sidecar.
    let native_members_dir =
        PathBuf::from(format!("{}.native-members", llvm_archive_output.display()));
    for (index, missing) in missing_ir_files.iter().enumerate() {
        let Some(source) = missing.canonicalize().ok().filter(|path| path.is_file()) else {
            continue;
        };
        let basename = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("native.o");
        let destination = native_members_dir.join(format!("{index}-{basename}"));
        if let Err(error) = ensure_dir_exists(&destination).and_then(|_| {
            fs::copy(&source, &destination)
                .map(|_| ())
                .map_err(std::io::Error::other)
        }) {
            debug_log(
                debug,
                &format!(
                    "[DEBUG] Failed to preserve native archive member {}: {}",
                    source.display(),
                    error
                ),
            );
        } else {
            // Keep the original path in `_missing` for compatibility. The
            // installer can resolve it from this stable cache if libtool has
            // already removed the temporary `.lax` symlink.
        }
    }

    let missing_path = PathBuf::from(format!("{}_missing", llvm_archive_output.display()));
    if missing_ir_files.is_empty() {
        if let Err(e) = fs::remove_file(&missing_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Failed to remove LLVM IR missing-member file: {}", e);
                exit(1);
            }
        }
    } else {
        let mut missing_file = match File::create(&missing_path) {
            Ok(file) => file,
            Err(e) => {
                eprintln!(
                    "Failed to create LLVM IR missing-member file {}: {}",
                    missing_path.display(),
                    e
                );
                exit(1);
            }
        };
        for missing in &missing_ir_files {
            if let Err(e) = writeln!(missing_file, "{}", missing.display()) {
                eprintln!(
                    "Failed to write LLVM IR missing-member file {}: {}",
                    missing_path.display(),
                    e
                );
                exit(1);
            }
        }
    }

    if llvm_ir_files.is_empty() {
        debug_log(debug, "[DEBUG] No LLVM IR files found, skipping llvm-ar");
        exit(0);
    }

    // Prefer the version encoded in the invoked llvm-ar name. This matters
    // when `llvm-ar-21 -> ar`: the matching real tool must be llvm-ar-21,
    // even if CC is unset (or names a different compiler version).
    let clang_cmd_for_version = if let Some(version) = ar_cmd.strip_prefix("llvm-ar-") {
        format!("clang-{version}")
    } else {
        env::var("CC")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| env::var("CXX").ok().filter(|s| !s.is_empty()))
            .map(|s| {
                // Extract filename from path
                PathBuf::from(&s)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&s)
                    .to_string()
            })
            .unwrap_or_else(|| ar_cmd.to_string())
    };

    let mut llvm_ar_path = find_llvm_tool("llvm-ar", &clang_cmd_for_version);
    // When this wrapper is installed as `llvm-ar -> ar`, `find_llvm_tool`
    // can return the wrapper itself from PATH. The native archive invocation
    // above already excludes self through get_exe_path; do the same for the
    // IR archive invocation to avoid a second wrapper pass.
    if let (Ok(current_exe), Ok(found_exe)) = (
        env::current_exe().and_then(|path| path.canonicalize()),
        llvm_ar_path.canonicalize(),
    ) {
        if current_exe == found_exe {
            // Keep the versioned llvm-ar name (e.g. llvm-ar-22) when resolving
            // the real tool; `ar` would resolve to GNU ar, which cannot write
            // an LLVM bitcode archive.
            let llvm_ar_name = llvm_ar_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("llvm-ar");
            llvm_ar_path = get_exe_path(llvm_ar_name);
        }
    }

    debug_log(
        debug,
        &format!(
            "[DEBUG] Using clang cmd for llvm-ar version: {}",
            clang_cmd_for_version
        ),
    );

    debug_log(
        debug,
        &format!(
            "[DEBUG] llvm-ar: {} rcsD {} {}",
            llvm_ar_path.display(),
            llvm_archive_output.display(),
            llvm_ir_files
                .iter()
                .map(|f| f.display().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );

    let mut llvm_ar_cmd = Command::new(&llvm_ar_path);
    llvm_ar_cmd.arg("rcsD").arg(&llvm_archive_output);

    for ir_file in &llvm_ir_files {
        llvm_ar_cmd.arg(ir_file);
    }

    // Create log file
    let log_path = format!("{}_log", llvm_archive_output.display());
    if let Err(e) = ensure_dir_exists(&PathBuf::from(&log_path)) {
        eprintln!("Failed to create log file directory: {}", e);
        exit(1);
    }

    let log_file = match fs::File::create(&log_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create log file {}: {}", log_path, e);
            exit(1);
        }
    };

    // Redirect stdout and stderr to log file.
    llvm_ar_cmd.stdout(Stdio::from(log_file.try_clone().unwrap()));
    llvm_ar_cmd.stderr(Stdio::from(log_file));

    let llvm_ar_result = llvm_ar_cmd.status();

    match llvm_ar_result {
        Ok(status) if status.success() => {
            debug_log(
                debug,
                &format!(
                    "[DEBUG] llvm-ar succeeded, output: {}",
                    llvm_archive_output.display()
                ),
            );
            exit(0);
        }
        Ok(status) => {
            eprintln!("llvm-ar failed, see {} for details", log_path);
            exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("Failed to execute llvm-ar: {}", e);
            exit(1);
        }
    }
}
