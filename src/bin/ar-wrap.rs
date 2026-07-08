// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! ar/llvm-ar wrapper
//!
//! After executing ar command, automatically invoke llvm-link to merge LLVM IR files

use std::env;
use std::fs;
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
            let is_bsd_option = arg.chars().all(|c| c.is_ascii_lowercase())
                && arg.len() <= 5
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

    // Find LLVM IR files corresponding to .o files
    let mut llvm_ir_files: Vec<PathBuf> = Vec::new();

    for obj_file in &member_files {
        if !obj_file.extension().map(|e| e == "o").unwrap_or(false) {
            continue;
        }

        if let Some(ir_path) = find_llvmir_file(obj_file, &llvm_ir_dir) {
            llvm_ir_files.push(ir_path);
        } else {
            debug_log(
                debug,
                &format!(
                    "[DEBUG] Warning: LLVM IR file not found for: {}",
                    obj_file.display()
                ),
            );
        }
    }

    if llvm_ir_files.is_empty() {
        debug_log(debug, "[DEBUG] No LLVM IR files found, skipping llvm-link");
        exit(0);
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
            debug_log(
                debug,
                "[DEBUG] No archive file specified, skipping llvm-link",
            );
            exit(0);
        }
    };

    // Build llvm-link output path
    let mut llvm_link_output = PathBuf::from(&llvm_ir_dir);
    let rel_output = archive_path.strip_prefix("/").unwrap_or(&archive_path);
    llvm_link_output.push(rel_output);

    // Ensure output directory exists
    if let Err(e) = ensure_dir_exists(&llvm_link_output) {
        eprintln!("Failed to create LLVM IR output directory: {}", e);
        exit(1);
    }

    // Find llvm-link
    // Prefer using CC or CXX environment variable to determine llvm-link version
    let clang_cmd_for_version = env::var("CC")
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
        .unwrap_or_else(|| ar_cmd.to_string());

    let llvm_link_path = find_llvm_tool("llvm-link", &clang_cmd_for_version);

    debug_log(
        debug,
        &format!(
            "[DEBUG] Using clang cmd for llvm-link version: {}",
            clang_cmd_for_version
        ),
    );

    debug_log(
        debug,
        &format!(
            "[DEBUG] llvm-link: {} -o {} {}",
            llvm_link_path.display(),
            llvm_link_output.display(),
            llvm_ir_files
                .iter()
                .map(|f| f.display().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );

    // Build llvm-link command
    let mut llvm_link_cmd = Command::new(&llvm_link_path);
    llvm_link_cmd.arg("-o").arg(&llvm_link_output);

    for ir_file in &llvm_ir_files {
        llvm_link_cmd.arg(ir_file);
    }

    // Create log file
    let log_path = format!("{}_log", llvm_link_output.display());
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

    // Redirect stdout and stderr to log file
    llvm_link_cmd.stdout(Stdio::from(log_file.try_clone().unwrap()));
    llvm_link_cmd.stderr(Stdio::from(log_file));

    let llvm_link_result = llvm_link_cmd.status();

    match llvm_link_result {
        Ok(status) if status.success() => {
            debug_log(
                debug,
                &format!(
                    "[DEBUG] llvm-link succeeded, output: {}",
                    llvm_link_output.display()
                ),
            );
            exit(0);
        }
        Ok(status) => {
            eprintln!("llvm-link failed, see {} for details", log_path);
            exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("Failed to execute llvm-link: {}", e);
            exit(1);
        }
    }
}
