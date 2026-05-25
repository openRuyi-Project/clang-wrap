// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! cp wrapper
//!
//! After executing cp command, synchronously copy corresponding files in llvmir directory

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use clang_wrap::{debug_log, get_exe_path, get_llvm_ir_dir, is_debug_mode,
    get_program_name, resolve_cmd_name, find_llvmir_file,
    compute_llvmir_path, compute_llvmir_target_dir, ensure_dir_exists,
    sync_llvmir_with_aux_files};

/// Common file operation parameters (for cp/mv/ln)
pub struct FileOpArgs {
    pub sources: Vec<PathBuf>,
    pub destination: Option<PathBuf>,
    pub target_dir: Option<PathBuf>,
    pub other_args: Vec<String>,
}

/// Parse cp command arguments
fn parse_cp_args(args: &[String]) -> FileOpArgs {
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
        } else if arg.starts_with("--target-directory=") {
            result.target_dir = Some(PathBuf::from(&arg[18..]));
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

/// Execute cp command
fn execute_cp(cp_path: &Path, args: &[String], debug: bool) -> i32 {
    debug_log(debug, &format!("[DEBUG] Executing cp: {} {}", cp_path.display(), args.join(" ")));
    
    let status = Command::new(cp_path)
        .args(args)
        .status()
        .expect(&format!("Failed to execute {}", cp_path.display()));
    
    status.code().unwrap_or(1)
}

/// Handle source file llvmir sync (copy to target directory)
fn process_source_llvmir(
    cp_path: &Path,
    source: &Path,
    target_dir: &Path,
    other_args: &[String],
    llvmir_dir: &str,
    debug: bool,
) {
    if let Some(llvmir_source) = find_llvmir_file(source, llvmir_dir) {
        let source_name = source.file_name().expect("Source should have a filename");
        debug_log(debug, &format!("[DEBUG] Found llvmir file: {}", llvmir_source.display()));
        
        let llvmir_target_dir = compute_llvmir_target_dir(target_dir, llvmir_dir);
        
        if let Err(e) = fs::create_dir_all(&llvmir_target_dir) {
            eprintln!("Warning: Failed to create llvmir directory {}: {}", 
                      llvmir_target_dir.display(), e);
            return;
        }
        
        let llvmir_dest = llvmir_target_dir.join(source_name);
        
        // Use common sync function to handle main file and auxiliary files
        sync_llvmir_with_aux_files(
            cp_path,
            "cp",
            &llvmir_source,
            &llvmir_dest,
            other_args,
            &["-t", "--target-directory="],
            debug,
        );
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let program_name = get_program_name(&args);
    let cp_cmd = resolve_cmd_name(program_name, "cp-wrap", "cp");
    let cp_path = get_exe_path(cp_cmd);
    
    if args.len() < 2 {
        let status = Command::new(&cp_path)
            .status()
            .expect(&format!("Failed to execute {}", cp_path.display()));
        exit(status.code().unwrap_or(1));
    }
    
    let debug = is_debug_mode();
    let llvmir_dir = get_llvm_ir_dir();
    
    // First execute the original cp command
    let cp_result = execute_cp(&cp_path, &args[1..], debug);
    
    if cp_result != 0 {
        exit(cp_result);
    }
    
    // Parse arguments
    let parsed = parse_cp_args(&args[1..]);
    
    debug_log(debug, "[DEBUG] Parsed cp args:");
    debug_log(debug, &format!("[DEBUG]   sources: {:?}", parsed.sources));
    debug_log(debug, &format!("[DEBUG]   destination: {:?}", parsed.destination));
    debug_log(debug, &format!("[DEBUG]   target_dir: {:?}", parsed.target_dir));
    
    // Case 1: Use -t DIRECTORY to specify target directory
    if let Some(ref target_dir) = parsed.target_dir {
        for source in &parsed.sources {
            process_source_llvmir(&cp_path, source, target_dir, &parsed.other_args, &llvmir_dir, debug);
        }
        exit(0);
    }
    
    // Case 2: Multiple source files, last argument is target directory
    if parsed.sources.len() > 1 && parsed.destination.is_some() {
        let dest = parsed.destination.as_ref().unwrap();
        
        if dest.is_dir() {
            for source in &parsed.sources {
                process_source_llvmir(&cp_path, source, dest, &parsed.other_args, &llvmir_dir, debug);
            }
            exit(0);
        }
    }
    
    // Case 3: Single source file with explicit target
    if parsed.sources.len() == 1 {
        let source = &parsed.sources[0];
        
        if let Some(llvmir_source) = find_llvmir_file(source, &llvmir_dir) {
            debug_log(debug, &format!("[DEBUG] Found llvmir file: {}", llvmir_source.display()));
            
            let dest = match &parsed.destination {
                Some(d) => d.clone(),
                None => exit(0),
            };
            
            let llvmir_dest = compute_llvmir_path(&dest, &llvmir_dir);
            
            debug_log(debug, &format!("[DEBUG] Copying in llvmir: {} -> {}", 
                      llvmir_source.display(), llvmir_dest.display()));
            
            if let Err(e) = ensure_dir_exists(&llvmir_dest) {
                eprintln!("Warning: Failed to create llvmir directory: {}", e);
                exit(0);
            }
            
            sync_llvmir_with_aux_files(
                &cp_path,
                "cp",
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