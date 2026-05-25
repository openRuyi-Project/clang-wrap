// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! mv 包装器
//!
//! 在执行 mv 命令后，同步移动 llvmir 目录中的对应文件

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use clang_wrap::{debug_log, get_exe_path, get_llvm_ir_dir, is_debug_mode,
    get_program_name, resolve_cmd_name, find_llvmir_file_with_debug,
    compute_llvmir_path, compute_llvmir_target_dir, ensure_dir_exists,
    sync_llvmir_with_aux_files, DEBUG_LOG_PATH};

/// 通用文件操作参数
struct FileOpArgs {
    sources: Vec<PathBuf>,
    destination: Option<PathBuf>,
    target_dir: Option<PathBuf>,
    other_args: Vec<String>,
}

/// 解析 mv 命令参数
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

/// 执行 mv 命令
fn execute_mv(mv_path: &Path, args: &[String], debug: bool) -> i32 {
    debug_log(debug, &format!("[DEBUG] Executing mv: {} {}", mv_path.display(), args.join(" ")));
    
    let status = Command::new(mv_path)
        .args(args)
        .status()
        .expect(&format!("Failed to execute {}", mv_path.display()));
    
    status.code().unwrap_or(1)
}

/// 处理源文件的 llvmir 同步（移动到目标目录）
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
        debug_log(debug, &format!("[DEBUG] Found llvmir file: {}", llvmir_source.display()));
        
        let llvmir_target_dir = compute_llvmir_target_dir(target_dir, llvmir_dir);
        
        if let Err(e) = fs::create_dir_all(&llvmir_target_dir) {
            eprintln!("Warning: Failed to create llvmir directory {}: {}", 
                      llvmir_target_dir.display(), e);
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

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let debug = is_debug_mode();
    let llvmir_dir = get_llvm_ir_dir();
    
    // 初始化调试日志
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
            .expect(&format!("Failed to execute {}", mv_path.display()));
        exit(status.code().unwrap_or(1));
    }
    
    // 先执行原始的 mv 命令
    let mv_result = execute_mv(&mv_path, &args[1..], debug);
    
    if mv_result != 0 {
        exit(mv_result);
    }
    
    // 解析参数
    let parsed = parse_mv_args(&args[1..]);
    
    debug_log(debug, "[DEBUG] Parsed mv args:");
    debug_log(debug, &format!("[DEBUG]   sources: {:?}", parsed.sources));
    debug_log(debug, &format!("[DEBUG]   destination: {:?}", parsed.destination));
    debug_log(debug, &format!("[DEBUG]   target_dir: {:?}", parsed.target_dir));
    
    // 情况 1: 使用 -t DIRECTORY 指定目标目录
    if let Some(ref target_dir) = parsed.target_dir {
        for source in &parsed.sources {
            process_source_llvmir(&mv_path, source, target_dir, &parsed.other_args, &llvmir_dir, debug);
        }
        exit(0);
    }
    
    // 情况 2: 多个源文件，最后一个参数是目标目录
    if parsed.sources.len() > 1 && parsed.destination.is_some() {
        let dest = parsed.destination.as_ref().unwrap();
        
        if dest.is_dir() {
            for source in &parsed.sources {
                process_source_llvmir(&mv_path, source, dest, &parsed.other_args, &llvmir_dir, debug);
            }
            exit(0);
        }
    }
    
    // 情况 3: 单个源文件，有明确的目标
    if parsed.sources.len() == 1 {
        let source = &parsed.sources[0];
        
        if let Some(llvmir_source) = find_llvmir_file_with_debug(source, &llvmir_dir, debug) {
            debug_log(debug, &format!("[DEBUG] Found llvmir file: {}", llvmir_source.display()));
            
            let dest = match &parsed.destination {
                Some(d) => d.clone(),
                None => exit(0),
            };
            
            let llvmir_dest = compute_llvmir_path(&dest, &llvmir_dir);
            
            debug_log(debug, &format!("[DEBUG] Moving in llvmir: {} -> {}", 
                      llvmir_source.display(), llvmir_dest.display()));
            
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