// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! strip wrapper
//!
//! 在执行 strip 命令后，同步处理 llvmir 目录中的对应文件

use std::env;
use std::path::PathBuf;
use std::process::{Command, exit};

use clang_wrap::{debug_log, get_exe_path, get_llvm_ir_dir, is_debug_mode,
    is_emit_llvmir_enabled, init_debug_log, get_program_name, resolve_cmd_name,
    find_llvmir_file, compute_llvmir_path, copy_file, copy_and_modify_cmd_file,
    find_all_aux_files, AuxFileSuffix};

/// 解析 strip 命令参数
struct StripArgs {
    files: Vec<PathBuf>,
    output: Option<PathBuf>,
    other_args: Vec<String>,
}

fn parse_strip_args(args: &[String]) -> StripArgs {
    let mut result = StripArgs {
        files: Vec::new(),
        output: None,
        other_args: Vec::new(),
    };
    
    let mut i = 0;
    let n = args.len();
    
    while i < n {
        let arg = &args[i];
        
        if arg == "-o" || arg == "--output" {
            if i + 1 < n {
                result.output = Some(PathBuf::from(&args[i + 1]));
                result.other_args.push(arg.clone());
                result.other_args.push(args[i + 1].clone());
                i += 2;
                continue;
            }
        } else if arg.starts_with("-o") && arg.len() > 2 {
            result.output = Some(PathBuf::from(&arg[2..]));
            result.other_args.push(arg.clone());
        } else if arg.starts_with("--output=") {
            result.output = Some(PathBuf::from(&arg[9..]));
            result.other_args.push(arg.clone());
        } else if arg.starts_with('-') {
            result.other_args.push(arg.clone());
        } else {
            result.files.push(PathBuf::from(arg));
        }
        
        i += 1;
    }
    
    result
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let program_name = get_program_name(&args);
    let strip_cmd = resolve_cmd_name(program_name, "strip-wrap", "strip");
    let strip_path = get_exe_path(strip_cmd);
    
    if args.len() < 2 {
        let status = Command::new(&strip_path)
            .status()
            .expect(&format!("Failed to execute {}", strip_path.display()));
        exit(status.code().unwrap_or(1));
    }
    
    let debug = is_debug_mode();
    let emit_llvmir = is_emit_llvmir_enabled();
    let llvmir_dir = get_llvm_ir_dir();
    
    if debug {
        init_debug_log(&llvmir_dir);
    }
    
    debug_log(debug, &format!("[DEBUG] Executing strip: {} {}", strip_path.display(), args[1..].join(" ")));
    
    let parsed = parse_strip_args(&args[1..]);
    
    debug_log(debug, "[DEBUG] Parsed strip args:");
    debug_log(debug, &format!("[DEBUG]   files: {:?}", parsed.files));
    debug_log(debug, &format!("[DEBUG]   output: {:?}", parsed.output));
    
    // 执行原始的 strip 命令
    let status = Command::new(&strip_path)
        .args(&args[1..])
        .status()
        .expect(&format!("Failed to execute {}", strip_path.display()));
    
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
    
    if !emit_llvmir {
        exit(0);
    }
    
    // 对于 llvmir 文件，只需要复制即可（不需要 strip）
    if let Some(ref output) = parsed.output {
        // 模式: strip -o OUTPUT FILE
        for input_file in &parsed.files {
            if let Some(llvmir_source) = find_llvmir_file(input_file, &llvmir_dir) {
                debug_log(debug, &format!("[DEBUG] Found llvmir file: {}", llvmir_source.display()));
                
                let llvmir_dest = compute_llvmir_path(output, &llvmir_dir);
                
                // 复制 llvmir 文件
                copy_file(&llvmir_source, &llvmir_dest, debug);
                
                // 获取源文件名和目标文件名用于 _cmd 文件的修改
                let source_name = input_file.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("input");
                let dest_name = output.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("output");
                
                // 复制辅助文件
                for (suffix, aux_source) in find_all_aux_files(&llvmir_source) {
                    let aux_dest = PathBuf::from(format!("{}{}", llvmir_dest.display(), suffix.as_str()));
                    debug_log(debug, &format!("[DEBUG] Copying aux file: {} -> {}", 
                              aux_source.display(), aux_dest.display()));
                    
                    // _cmd 文件需要特殊处理（修改内容）
                    if matches!(suffix, AuxFileSuffix::Cmd) {
                        copy_and_modify_cmd_file(&aux_source, &aux_dest, source_name, dest_name, debug);
                    } else {
                        copy_file(&aux_source, &aux_dest, debug);
                    }
                }
            }
        }
    } else {
        // 模式: strip FILE... - 就地修改
        for input_file in &parsed.files {
            if let Some(llvmir_file) = find_llvmir_file(input_file, &llvmir_dir) {
                debug_log(debug, &format!("[DEBUG] llvmir file exists at: {}", llvmir_file.display()));
            }
        }
    }
    
    exit(0);
}