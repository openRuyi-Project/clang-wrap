// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! ln wrapper
//!
//! After executing ln command, synchronously create links in llvmir directory

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use clang_wrap::{debug_log, get_exe_path, get_llvm_ir_dir, is_debug_mode,
    get_program_name, resolve_cmd_name, find_llvmir_file,
    compute_llvmir_path, compute_llvmir_target_dir, ensure_dir_exists,
    sync_llvmir_with_aux_files};

/// Common file operation parameters
struct FileOpArgs {
    targets: Vec<PathBuf>,
    link_name_or_dir: Option<PathBuf>,
    target_dir: Option<PathBuf>,
    other_args: Vec<String>,
}

/// Parse ln command arguments
fn parse_ln_args(args: &[String]) -> FileOpArgs {
    let mut result = FileOpArgs {
        targets: Vec::new(),
        link_name_or_dir: None,
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
        } else if arg.starts_with("--target-directory=") {
            result.target_dir = Some(PathBuf::from(&arg[18..]));
            result.other_args.push(arg.clone());
        } else if arg.starts_with('-') {
            result.other_args.push(arg.clone());
        } else {
            result.targets.push(PathBuf::from(arg));
        }
        
        i += 1;
    }
    
    if result.target_dir.is_none() && result.targets.len() > 1 {
        result.link_name_or_dir = result.targets.pop();
    }
    
    result
}

/// Execute ln command
fn execute_ln(ln_path: &Path, args: &[String], debug: bool) -> i32 {
    debug_log(debug, &format!("[DEBUG] Executing ln: {} {}", ln_path.display(), args.join(" ")));
    
    let status = Command::new(ln_path)
        .args(args)
        .status()
        .expect(&format!("Failed to execute {}", ln_path.display()));
    
    status.code().unwrap_or(1)
}

/// Handle target file llvmir sync (create link to target directory)
fn process_target_llvmir(
    ln_path: &Path,
    target: &Path,
    target_dir: &Path,
    other_args: &[String],
    llvmir_dir: &str,
    debug: bool,
) {
    if let Some(llvmir_source) = find_llvmir_file(target, llvmir_dir) {
        let target_name = target.file_name().expect("Target should have a filename");
        debug_log(debug, &format!("[DEBUG] Found llvmir file: {}", llvmir_source.display()));
        
        let llvmir_target_dir = compute_llvmir_target_dir(target_dir, llvmir_dir);
        
        if let Err(e) = fs::create_dir_all(&llvmir_target_dir) {
            eprintln!("Warning: Failed to create llvmir directory {}: {}", 
                      llvmir_target_dir.display(), e);
            return;
        }
        
        let llvmir_link_full = llvmir_target_dir.join(target_name);
        
        sync_llvmir_with_aux_files(
            ln_path,
            "ln",
            &llvmir_source,
            &llvmir_link_full,
            other_args,
            &["-t", "--target-directory="],
            debug,
        );
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let program_name = get_program_name(&args);
    let ln_cmd = resolve_cmd_name(program_name, "ln-wrap", "ln");
    let ln_path = get_exe_path(ln_cmd);
    
    if args.len() < 2 {
        let status = Command::new(&ln_path)
            .status()
            .expect(&format!("Failed to execute {}", ln_path.display()));
        exit(status.code().unwrap_or(1));
    }
    
    let debug = is_debug_mode();
    let llvmir_dir = get_llvm_ir_dir();
    
    // First execute original ln command
    let ln_result = execute_ln(&ln_path, &args[1..], debug);
    
    if ln_result != 0 {
        exit(ln_result);
    }
    
    // Parse arguments
    let parsed = parse_ln_args(&args[1..]);
    
    debug_log(debug, "[DEBUG] Parsed ln args:");
    debug_log(debug, &format!("[DEBUG]   targets: {:?}", parsed.targets));
    debug_log(debug, &format!("[DEBUG]   link_name_or_dir: {:?}", parsed.link_name_or_dir));
    debug_log(debug, &format!("[DEBUG]   target_dir: {:?}", parsed.target_dir));
    
    // Case 1: Use -t DIRECTORY to specify target directory
    if let Some(ref target_dir) = parsed.target_dir {
        for target in &parsed.targets {
            process_target_llvmir(&ln_path, target, target_dir, &parsed.other_args, &llvmir_dir, debug);
        }
        exit(0);
    }
    
    // Case 2: Multiple target files, last argument is directory
    if parsed.targets.len() > 1 && parsed.link_name_or_dir.is_some() {
        let link_dir = parsed.link_name_or_dir.as_ref().unwrap();
        
        if link_dir.is_dir() {
            for target in &parsed.targets {
                process_target_llvmir(&ln_path, target, link_dir, &parsed.other_args, &llvmir_dir, debug);
            }
            exit(0);
        }
    }
    
    // Case 3: Single target file, with explicit link name
    if parsed.targets.len() == 1 {
        let target = &parsed.targets[0];
        
        if let Some(llvmir_source) = find_llvmir_file(target, &llvmir_dir) {
            debug_log(debug, &format!("[DEBUG] Found llvmir file: {}", llvmir_source.display()));
            
            let link_name = match &parsed.link_name_or_dir {
                Some(name) => name.clone(),
                None => {
                    let target_name = target.file_name().expect("Target should have a filename");
                    PathBuf::from(".").join(target_name)
                }
            };
            
            let llvmir_link = compute_llvmir_path(&link_name, &llvmir_dir);
            
            debug_log(debug, &format!("[DEBUG] Creating link in llvmir: {} -> {}", 
                      llvmir_link.display(), llvmir_source.display()));
            
            if let Err(e) = ensure_dir_exists(&llvmir_link) {
                eprintln!("Warning: Failed to create llvmir directory: {}", e);
                exit(0);
            }
            
            sync_llvmir_with_aux_files(
                &ln_path,
                "ln",
                &llvmir_source,
                &llvmir_link,
                &parsed.other_args,
                &["-t", "--target-directory="],
                debug,
            );
        }
    }
    
    exit(0);
}