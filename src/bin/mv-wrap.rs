// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! mv wrapper
//!
//! After executing mv command, synchronously move corresponding files in llvmir directory

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use clang_wrap::{
    compute_llvmir_path, compute_llvmir_target_dir, debug_log, ensure_dir_exists,
    find_llvmir_file_with_debug, get_exe_path, get_llvm_ir_dir, get_program_name, is_debug_mode,
    resolve_cmd_name, sync_llvmir_with_aux_files, DEBUG_LOG_PATH,
};

/// Common file operation parameters
struct FileOpArgs {
    sources: Vec<PathBuf>,
    destination: Option<PathBuf>,
    target_dir: Option<PathBuf>,
    other_args: Vec<String>,
}

/// Parse mv command arguments
fn parse_mv_args(args: &[String]) -> FileOpArgs {
    let mut result = FileOpArgs {
        sources: Vec::new(),
        destination: None,
        target_dir: None,
        other_args: Vec::new(),
    };

    let mut i = 0;
    let n = args.len();

    while i < n {
        let arg = &args[i];

        if arg == "-t" {
            if i + 1 < n {
                result.target_dir = Some(PathBuf::from(&args[i + 1]));
                result.other_args.push(arg.clone());
                result.other_args.push(args[i + 1].clone());
                i += 2;
                continue;
            }
        } else if arg.starts_with("-t") && arg.len() > 2 {
            result.target_dir = Some(PathBuf::from(&arg[2..]));
            result.other_args.push(arg.clone());
        } else if let Some(dir) = arg.strip_prefix("--target-directory=") {
            result.target_dir = Some(PathBuf::from(dir));
            result.other_args.push(arg.clone());
        } else if arg.starts_with('-') {
            result.other_args.push(arg.clone());
        } else {
            result.sources.push(PathBuf::from(arg));
        }

        i += 1;
    }

    if result.target_dir.is_none() && result.sources.len() > 1 {
        result.destination = result.sources.pop();
    }

    result
}

/// Execute mv command
fn execute_mv(mv_path: &Path, args: &[String], debug: bool) -> i32 {
    debug_log(
        debug,
        &format!(
            "[DEBUG] Executing mv: {} {}",
            mv_path.display(),
            args.join(" ")
        ),
    );

    let status = Command::new(mv_path)
        .args(args)
        .status()
        .unwrap_or_else(|_| panic!("Failed to execute {}", mv_path.display()));

    status.code().unwrap_or(1)
}

/// Handle source file llvmir sync (move to target directory)
fn process_source_llvmir(
    mv_path: &Path,
    source: &Path,
    target_dir: &Path,
    other_args: &[String],
    llvmir_dir: &str,
    debug: bool,
) {
    if let Some(llvmir_source) = find_llvmir_file_with_debug(source, llvmir_dir, debug) {
        let source_name = source.file_name().expect("Source should have a filename");
        debug_log(
            debug,
            &format!("[DEBUG] Found llvmir file: {}", llvmir_source.display()),
        );

        let llvmir_target_dir = compute_llvmir_target_dir(target_dir, llvmir_dir);

        if let Err(e) = fs::create_dir_all(&llvmir_target_dir) {
            eprintln!(
                "Warning: Failed to create llvmir directory {}: {}",
                llvmir_target_dir.display(),
                e
            );
            return;
        }

        let llvmir_dest = llvmir_target_dir.join(source_name);

        sync_llvmir_with_aux_files(
            mv_path,
            "mv",
            &llvmir_source,
            &llvmir_dest,
            other_args,
            &["-t", "--target-directory="],
            debug,
        );
    }
}

fn resolve_single_dest(source: &Path, dest: &Path) -> PathBuf {
    if dest.is_dir() {
        if let Some(source_name) = source.file_name() {
            return dest.join(source_name);
        }
    }

    dest.to_path_buf()
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let debug = is_debug_mode();
    let llvmir_dir = get_llvm_ir_dir();

    // Initialize debug log
    if debug {
        let log_path = PathBuf::from(&llvmir_dir).join("mv-wrap.log");
        let _ = DEBUG_LOG_PATH.set(log_path);
        debug_log(debug, "[DEBUG] ========== mv-wrap started ==========");
        debug_log(debug, &format!("[DEBUG] Args: {:?}", args));
    }

    let program_name = get_program_name(&args);
    let mv_cmd = resolve_cmd_name(program_name, "mv-wrap", "mv");
    let mv_path = get_exe_path(mv_cmd);

    if args.len() < 2 {
        let status = Command::new(&mv_path)
            .status()
            .unwrap_or_else(|_| panic!("Failed to execute {}", mv_path.display()));
        exit(status.code().unwrap_or(1));
    }

    // First execute original mv command
    let mv_result = execute_mv(&mv_path, &args[1..], debug);

    if mv_result != 0 {
        exit(mv_result);
    }

    // Parse arguments
    let parsed = parse_mv_args(&args[1..]);

    debug_log(debug, "[DEBUG] Parsed mv args:");
    debug_log(debug, &format!("[DEBUG]   sources: {:?}", parsed.sources));
    debug_log(
        debug,
        &format!("[DEBUG]   destination: {:?}", parsed.destination),
    );
    debug_log(
        debug,
        &format!("[DEBUG]   target_dir: {:?}", parsed.target_dir),
    );

    // Case 1: Use -t DIRECTORY to specify target directory
    if let Some(ref target_dir) = parsed.target_dir {
        for source in &parsed.sources {
            process_source_llvmir(
                &mv_path,
                source,
                target_dir,
                &parsed.other_args,
                &llvmir_dir,
                debug,
            );
        }
        exit(0);
    }

    // Case 2: Multiple source files, last argument is target directory
    if parsed.sources.len() > 1 {
        let Some(dest) = parsed.destination.as_ref() else {
            exit(0);
        };

        if dest.is_dir() {
            for source in &parsed.sources {
                process_source_llvmir(
                    &mv_path,
                    source,
                    dest,
                    &parsed.other_args,
                    &llvmir_dir,
                    debug,
                );
            }
            exit(0);
        }
    }

    // Case 3: Single source file, with explicit target
    if parsed.sources.len() == 1 {
        let source = &parsed.sources[0];

        if let Some(llvmir_source) = find_llvmir_file_with_debug(source, &llvmir_dir, debug) {
            debug_log(
                debug,
                &format!("[DEBUG] Found llvmir file: {}", llvmir_source.display()),
            );

            let dest = match &parsed.destination {
                Some(d) => d.clone(),
                None => exit(0),
            };

            let actual_dest = resolve_single_dest(source, &dest);
            let llvmir_dest = compute_llvmir_path(&actual_dest, &llvmir_dir);

            debug_log(
                debug,
                &format!(
                    "[DEBUG] Moving in llvmir: {} -> {}",
                    llvmir_source.display(),
                    llvmir_dest.display()
                ),
            );

            if let Err(e) = ensure_dir_exists(&llvmir_dest) {
                eprintln!("Warning: Failed to create llvmir directory: {}", e);
                exit(0);
            }

            sync_llvmir_with_aux_files(
                &mv_path,
                "mv",
                &llvmir_source,
                &llvmir_dest,
                &parsed.other_args,
                &["-t", "--target-directory="],
                debug,
            );
        }
    }

    exit(0);
}
