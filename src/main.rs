// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! clang/clang++ wrapper
//!
//! Generates LLVM IR during compilation, merges LLVM IR files using llvm-link during linking

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, exit, Stdio};

use clang_wrap::{debug_log, get_exe_path, get_llvm_ir_dir, is_debug_mode,
    get_emit_llvmir_opt, find_llvm_tool, get_program_name,
    get_absolute_path, ensure_dir_exists, generate_tmp_suffix, append_tmp_suffix,
    shell_escape, expand_at_file_args, init_debug_log};

// ============================================================================
// Constant definitions
// ============================================================================

/// Source file extensions
const SOURCE_EXTENSIONS: &[&str] = &[".c", ".cpp", ".cc", ".cxx", ".c++", ".m", ".mm", ".S", ".s", ".asm"];

/// Get clang version information
fn get_clang_version(clang_path: &Path) -> Option<String> {
    let output = Command::new(clang_path)
        .arg("--version")
        .output()
        .ok()?;
    
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(first_line) = stdout.lines().next() {
            return Some(first_line.to_string());
        }
    }
    None
}

/// Get target architecture through full compilation command
/// Run with --version appended to the full compilation command, parse the Target line in output
fn get_compile_target(clang_path: &Path, args: &[String]) -> Option<String> {
    let mut cmd = Command::new(clang_path);
    cmd.args(args);
    cmd.arg("--version");
    
    let output = cmd.output().ok()?;
    
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.starts_with("Target:") {
                let target = line["Target:".len()..].trim();
                return Some(target.to_string());
            }
        }
    }
    None
}

/// Return corresponding -march option based on target architecture
fn get_march_for_target(target: &str) -> Option<&'static str> {
    // Detect riscv64
    if target.starts_with("riscv64") || target.contains("riscv64") {
        return Some("-march=rva23u64");
    }
    // Detect x86-64 (including x86_64)
    if target.starts_with("x86_64") || target.starts_with("x86-64") 
        || target.contains("x86_64") || target.contains("x86-64") {
        return Some("-march=x86-64-v4");
    }
    None
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
    let mut link_flags: Vec<String> = Vec::new();
    let mut library_dirs: Vec<String> = Vec::new();
    let mut libraries: Vec<String> = Vec::new();
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
            link_flags.push(arg.clone());
        } else if arg == "-L" {
            if i + 1 < args.len() {
                library_dirs.push(args[i + 1].clone());
                link_flags.push(arg.clone());
                link_flags.push(args[i + 1].clone());
                i += 2;
                continue;
            }
        } else if arg.starts_with("-L") && arg.len() > 2 {
            let dir = &arg[2..];
            library_dirs.push(dir.to_string());
            link_flags.push(arg.clone());
        } else if arg == "-l" {
            if i + 1 < args.len() {
                libraries.push(args[i + 1].clone());
                link_flags.push(arg.clone());
                link_flags.push(args[i + 1].clone());
                i += 2;
                continue;
            }
        } else if arg.starts_with("-l") && arg.len() > 2 {
            let lib = &arg[2..];
            libraries.push(lib.to_string());
            link_flags.push(arg.clone());
        } else if arg.ends_with(".o") && !arg.starts_with('-') {
            object_files.push(PathBuf::from(arg));
        } else if !arg.starts_with('-') && SOURCE_EXTENSIONS.iter().any(|ext| arg.ends_with(ext)) {
            source_files.push(PathBuf::from(arg));
        } else if !arg.starts_with('-') && (arg.ends_with(".so") || arg.contains(".so.") || arg.ends_with(".a")) {
            library_files.push(PathBuf::from(arg));
        } else if arg.starts_with("-Wl,") || arg.starts_with("-framework") {
            let normalized_arg = if arg.starts_with("-Wl,-version-script,") {
                let path = &arg[20..];
                format!("-Wl,--version-script={}", path)
            } else if arg.starts_with("-Wl,--version-script,") {
                let path = &arg[21..];
                format!("-Wl,--version-script={}", path)
            } else if arg == "-Wl,-version-script" {
                if i + 1 < args.len() && args[i + 1].starts_with("-Wl,") {
                    let next_arg = &args[i + 1];
                    let path = &next_arg[4..];
                    i += 1;
                    format!("-Wl,--version-script={}", path)
                } else {
                    arg.clone()
                }
            } else if arg == "-Wl,-soname" {
                if i + 1 < args.len() && args[i + 1].starts_with("-Wl,") {
                    let next_arg = &args[i + 1];
                    let soname = &next_arg[4..];
                    i += 1;
                    format!("-Wl,-soname,{}", soname)
                } else {
                    arg.clone()
                }
            } else {
                arg.clone()
            };
            
            link_flags.push(normalized_arg.clone());
            other_args.push(normalized_arg.clone());
            
            if normalized_arg.starts_with("-Wl,--version-script=") {
                let path = &normalized_arg[21..];
                version_script = Some(PathBuf::from(path));
            }
            
            if normalized_arg.starts_with("-Wl,-soname,") {
                let soname = &normalized_arg[13..];
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
    debug_log(debug_mode, &format!("[DEBUG] Normal linking: {} {}", clang_path.display(), args[1..].join(" ")));
    
    let status = Command::new(clang_path)
        .args(&args[1..])
        .status()
        .expect(&format!("Failed to execute {}", clang_path.display()));
    
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }

    // Second pass: use llvm-link to merge LLVM IR files
    let mut llvm_ir_files: Vec<PathBuf> = Vec::new();
    let mut temp_llvm_ir_files: Vec<PathBuf> = Vec::new();
    let mut merged_static_lib_paths: Vec<PathBuf> = Vec::new();
    
    // 1. Find LLVM IR files corresponding to .o files
    for obj_file in &object_files {
        let abs_obj = get_absolute_path(obj_file);
        
        let mut ir_path = PathBuf::from(llvm_ir_dir);
        let rel_path = abs_obj.strip_prefix("/")
            .unwrap_or(&abs_obj);
        ir_path.push(rel_path);
        
        if ir_path.exists() {
            llvm_ir_files.push(ir_path);
        } else {
            debug_log(debug_mode, &format!("[DEBUG] Warning: LLVM IR file not found: {}", ir_path.display()));
        }
    }
    
    // 1.5 Find LLVM IR files corresponding to static libraries
    for lib_file in &library_files {
        if !lib_file.extension().map(|e| e == "a").unwrap_or(false) {
            continue;
        }
        
        let abs_lib = get_absolute_path(lib_file);
        
        let mut ir_path = PathBuf::from(llvm_ir_dir);
        let rel_path = abs_lib.strip_prefix("/")
            .unwrap_or(&abs_lib);
        ir_path.push(rel_path);
        
        if ir_path.exists() {
            debug_log(debug_mode, &format!("[DEBUG] Found LLVM IR for static library: {} -> {}", lib_file.display(), ir_path.display()));
            llvm_ir_files.push(ir_path);
            merged_static_lib_paths.push(lib_file.clone());
        } else {
            debug_log(debug_mode, &format!("[DEBUG] Warning: LLVM IR file not found for static library: {}", ir_path.display()));
        }
    }
    
    // Get target architecture and determine if -march option needs to be added
    // Use full compilation command to get target architecture
    let march_opt = get_compile_target(clang_path, &args[1..])
        .and_then(|target| get_march_for_target(&target));
    
    if let Some(march) = march_opt {
        debug_log(debug_mode, &format!("[DEBUG] Adding march option for LLVM IR generation in link command: {}", march));
    }
    
    // 2. Generate LLVM IR for source files
    for source_file in &source_files {
        let abs_source = get_absolute_path(source_file);
        
        let mut ir_path = PathBuf::from(llvm_ir_dir);
        let rel_source = abs_source.strip_prefix("/")
            .unwrap_or(&abs_source);
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
        
        // Add -march option (if detected as needed)
        if let Some(march) = march_opt {
            llvm_gen_cmd.arg(march);
        }
        
        llvm_gen_cmd.arg("-emit-llvm");
        llvm_gen_cmd.arg("-c");
        llvm_gen_cmd.arg(source_file);
        llvm_gen_cmd.arg("-o").arg(&tmp_ir_path);
        
        {
            let mut cmd_parts = vec![clang_path.display().to_string()];
            cmd_parts.extend(other_args.clone());
            if let Some(march) = march_opt {
                cmd_parts.push(march.to_string());
            }
            cmd_parts.push("-emit-llvm".to_string());
            cmd_parts.push("-c".to_string());
            cmd_parts.push(source_file.display().to_string());
            cmd_parts.push("-o".to_string());
            cmd_parts.push(tmp_ir_path.display().to_string());
            debug_log(debug_mode, &format!("[DEBUG] Generating LLVM IR for source: {}", cmd_parts.join(" ")));
        }
        
        let llvm_gen_result = llvm_gen_cmd.status();
        
        match llvm_gen_result {
            Ok(status) if status.success() => {
                llvm_ir_files.push(tmp_ir_path.clone());
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

    if llvm_ir_files.is_empty() {
        debug_log(debug_mode, "[DEBUG] No LLVM IR files found, skipping llvm-link");
        exit(0);
    }

    let abs_output = get_absolute_path(&output_path);

    let mut llvm_link_output = PathBuf::from(llvm_ir_dir);
    let rel_output = abs_output.strip_prefix("/")
        .unwrap_or(&abs_output);
    llvm_link_output.push(rel_output);

    if let Err(e) = ensure_dir_exists(&llvm_link_output) {
        eprintln!("Failed to create LLVM IR output directory: {}", e);
        exit(1);
    }

    let clang_cmd_name = clang_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("clang");
    let llvm_link_path = find_llvm_tool("llvm-link", clang_cmd_name);
    
    let mut llvm_link_cmd = Command::new(&llvm_link_path);
    llvm_link_cmd.arg("-o").arg(&llvm_link_output);
    
    for ir_file in &llvm_ir_files {
        llvm_link_cmd.arg(ir_file);
    }

    {
        let mut cmd_parts = vec![llvm_link_path.display().to_string()];
        cmd_parts.push("-o".to_string());
        cmd_parts.push(llvm_link_output.display().to_string());
        for ir_file in &llvm_ir_files {
            cmd_parts.push(ir_file.display().to_string());
        }
        debug_log(debug_mode, &format!("[DEBUG] llvm-link: {}", cmd_parts.join(" ")));
    }

    let log_path = format!("{}_log", llvm_link_output.display());
    if let Err(e) = ensure_dir_exists(&PathBuf::from(&log_path)) {
        eprintln!("Failed to create log file directory: {}", e);
        exit(1);
    }

    let mut log_file = match File::create(&log_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create log file {}: {}", log_path, e);
            exit(1);
        }
    };

    let mut cmd_parts: Vec<String> = vec![llvm_link_path.display().to_string()];
    cmd_parts.push("-o".to_string());
    cmd_parts.push(llvm_link_output.display().to_string());
    for ir_file in &llvm_ir_files {
        cmd_parts.push(ir_file.display().to_string());
    }
    
    let cmd_str = cmd_parts.join(" ");
    let _ = writeln!(log_file, "Command: {}", cmd_str);

    llvm_link_cmd.stdout(Stdio::from(log_file.try_clone().unwrap()));
    llvm_link_cmd.stderr(Stdio::from(log_file));

    let llvm_link_result = llvm_link_cmd.status();

    match llvm_link_result {
        Ok(status) if status.success() => {
            for ir_file in &temp_llvm_ir_files {
                if let Err(e) = fs::remove_file(ir_file) {
                    eprintln!("Warning: Failed to remove intermediate file {}: {}", ir_file.display(), e);
                }
            }
            
            let version_script_dest = if let Some(ref vs_path) = version_script {
                let output_name = output_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("output");
                
                let vs_filename = format!("{}_verscript", output_name);
                
                let vs_dest = llvm_link_output.parent()
                    .map(|p| p.join(&vs_filename))
                    .unwrap_or_else(|| PathBuf::from(&vs_filename));
                
                if vs_path.exists() {
                    if let Err(e) = fs::copy(vs_path, &vs_dest) {
                        eprintln!("Warning: Failed to copy version script: {}", e);
                        None
                    } else {
                        debug_log(debug_mode, &format!("[DEBUG] Copied version script: {} -> {}", vs_path.display(), vs_dest.display()));
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
                &link_flags,
                &library_dirs,
                &libraries,
                &library_files,
                &merged_static_lib_paths,
                is_shared,
                llvm_ir_dir,
                &other_args,
                version_script_dest.as_deref(),
                soversion.as_deref(),
            ) {
                eprintln!("Warning: Failed to generate _cmd file: {}", e);
            }
            
            exit(0);
        }
        Ok(status) => {
            eprintln!("llvm-link failed, see {} for details", log_path);
            if output_path.exists() {
                let _ = fs::remove_file(&output_path);
            }
            exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("Failed to execute llvm-link: {}", e);
            if output_path.exists() {
                let _ = fs::remove_file(&output_path);
            }
            exit(1);
        }
    }
}

/// Generate link command script (_cmd file)
fn generate_link_cmd_file(
    clang_path: &Path,
    llvm_link_output: &Path,
    original_output: &Path,
    link_flags: &[String],
    library_dirs: &[String],
    libraries: &[String],
    library_files: &[PathBuf],
    merged_static_libs: &[PathBuf],
    is_shared: bool,
    _llvm_ir_dir: &str,
    other_args: &[String],
    version_script_dest: Option<&Path>,
    soversion: Option<&str>,
) -> Result<(), std::io::Error> {
    let cmd_path = format!("{}_cmd", llvm_link_output.display());
    
    let bc_filename = llvm_link_output.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output.bc");
    
    let original_output_filename = original_output.file_name()
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
    script.push_str("\n");
    script.push_str("# Original output: ");
    script.push_str(&output_filename);
    script.push_str("\n");
    script.push_str("#\n");
    script.push_str("\n");
    script.push_str("set -e\n\n");
    
    script.push_str("# Create output directory\n");
    script.push_str("mkdir -p output\n\n");
    
    script.push_str("# Link bitcode with original options\n");
    script.push_str("cmd=\"");
    script.push_str(&clang_path.display().to_string());
    
    if is_shared {
        script.push_str(" -shared -fPIC");
    }
    
    script.push_str(" -x ir");
    script.push_str(" -fuse-ld=lld");
    
    let mut skip_next = false;
    for arg in other_args {
        if skip_next {
            skip_next = false;
            continue;
        }
        
        if arg == "-c" || arg == "-emit-llvm" || arg == "-shared" {
            continue;
        }
        if arg.starts_with("-Wl,--version-script=") {
            continue;
        }
        if arg.starts_with("-Wl,") {
            continue;
        }
        if arg == "-I" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("-I") {
            continue;
        }
        if arg == "-D" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("-D") {
            continue;
        }
        if arg == "-U" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("-U") {
            continue;
        }
        if arg == "-include" {
            skip_next = true;
            continue;
        }
        if arg == "-imacros" {
            skip_next = true;
            continue;
        }
        if arg == "-idirafter" {
            skip_next = true;
            continue;
        }
        if arg == "-iprefix" {
            skip_next = true;
            continue;
        }
        if arg == "-iwithprefix" {
            skip_next = true;
            continue;
        }
        if arg == "-iwithprefixbefore" {
            skip_next = true;
            continue;
        }
        if arg == "-isystem" {
            skip_next = true;
            continue;
        }
        if arg == "-iquote" {
            skip_next = true;
            continue;
        }
        if arg == "-isysroot" {
            skip_next = true;
            continue;
        }
        if arg == "-MT" {
            skip_next = true;
            continue;
        }
        if arg == "-MF" {
            skip_next = true;
            continue;
        }
        if arg == "-MQ" {
            skip_next = true;
            continue;
        }
        if arg == "-MD" || arg == "-MMD" {
            continue;
        }
        if arg == "-std" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("-std=") {
            continue;
        }
        
        script.push_str(" ");
        script.push_str(&shell_escape(arg));
    }
    
    for flag in link_flags {
        if flag == "-shared" && is_shared {
            continue;
        }
        if flag.starts_with("-L") || flag.starts_with("-l") {
            continue;
        }
        if flag.starts_with("-Wl,--version-script=") {
            continue;
        }
        script.push_str(" ");
        script.push_str(&shell_escape(flag));
    }
    
    for dir in library_dirs {
        script.push_str(" -L");
        script.push_str(&shell_escape(dir));
    }
    
    script.push_str(" -L ..");
    
    script.push_str(" ./");
    script.push_str(bc_filename);
    
    for lib_file in library_files {
        if merged_static_libs.contains(lib_file) {
            continue;
        }
        
        if let Some(lib_basename) = lib_file.file_name().and_then(|n| n.to_str()) {
            if lib_basename.starts_with("lib") {
                let lib_name = if let Some(idx) = lib_basename.find(".so") {
                    &lib_basename[3..idx]
                } else if let Some(idx) = lib_basename.find(".a") {
                    &lib_basename[3..idx]
                } else {
                    lib_basename
                };
                script.push_str(" -l");
                script.push_str(lib_name);
            } else {
                script.push_str(" ./");
                script.push_str(lib_basename);
            }
        }
    }
    
    for lib in libraries {
        script.push_str(" -l");
        script.push_str(&shell_escape(lib));
    }
    
    if let Some(vs_path) = version_script_dest {
        if let Some(vs_filename) = vs_path.file_name().and_then(|n| n.to_str()) {
            script.push_str(" -Wl,--version-script=./");
            script.push_str(vs_filename);
        }
    }
    
    script.push_str(" -o output/");
    script.push_str(&output_filename);
    script.push_str("\"\n\n");
    
    script.push_str("# Function to replace library paths with .bc files\n");
    script.push_str("_try_bc() {\n");
    script.push_str("  local cmd_dir=\"$(dirname \"$0\")\"\n");
    script.push_str("  local llvmir_dir=\"$cmd_dir\"\n");
    script.push_str("  local result=\"\"\n");
    script.push_str("  local seen_libs=\"\"\n");
    script.push_str("  for arg in $@; do\n");
    script.push_str("    case \"$arg\" in\n");
    script.push_str("      -*)\n");
    script.push_str("        result=\"$result $arg\"\n");
    script.push_str("        ;;\n");
    script.push_str("      output/*)\n");
    script.push_str("        result=\"$result $arg\"\n");
    script.push_str("        ;;\n");
    script.push_str("      *.a|*.so|*.so.*)\n");
    script.push_str("        local lib_basename=$(basename \"$arg\")\n");
    script.push_str("        if echo \"$seen_libs\" | grep -q \" $lib_basename \"; then\n");
    script.push_str("          continue\n");
    script.push_str("        fi\n");
    script.push_str("        seen_libs=\"$seen_libs $lib_basename \"\n");
    script.push_str("        if [ -f \"$arg\" ]; then\n");
    script.push_str("          result=\"$result $arg\"\n");
    script.push_str("        else\n");
    script.push_str("          local bc_file=\"$llvmir_dir/$lib_basename.bc\"\n");
    script.push_str("          if [ -f \"$bc_file\" ]; then\n");
    script.push_str("            result=\"$result $bc_file\"\n");
    script.push_str("          else\n");
    script.push_str("            result=\"$result $arg\"\n");
    script.push_str("          fi\n");
    script.push_str("        fi\n");
    script.push_str("        ;;\n");
    script.push_str("      *)\n");
    script.push_str("        result=\"$result $arg\"\n");
    script.push_str("        ;;\n");
    script.push_str("    esac\n");
    script.push_str("  done\n");
    script.push_str("  \n");
    script.push_str("  echo \"$result\"\n");
    script.push_str("}\n\n");
    
    script.push_str("# Execute link command with .bc file substitution\n");
    script.push_str("newcmd=`_try_bc $cmd`\n");
    script.push_str("eval $newcmd\n");
    
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
            .expect(&format!("Failed to execute {}", clang_path.display()));
        exit(status.code().unwrap_or(1));
    }

    let debug_mode = is_debug_mode();
    
    let args = expand_at_file_args(&original_args, debug_mode);

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
    let has_object_inputs = args[1..].iter().any(|arg| {
        arg.ends_with(".o") && !arg.starts_with('-')
    });
    
    let has_source_inputs = args[1..].iter().any(|arg| {
        !arg.starts_with('-') && SOURCE_EXTENSIONS.iter().any(|ext| arg.ends_with(ext))
    });
    
    let is_link_command = !has_compile_flag && (has_object_inputs || has_shared_flag || has_source_inputs);
    
    if has_preprocess_flag {
        let status = Command::new(&clang_path)
            .args(&args[1..])
            .status()
            .expect(&format!("Failed to execute {}", clang_path.display()));
        exit(status.code().unwrap_or(1));
    }
    
    if !has_compile_flag && !is_link_command {
        let status = Command::new(&clang_path)
            .args(&args[1..])
            .status()
            .expect(&format!("Failed to execute {}", clang_path.display()));
        exit(status.code().unwrap_or(1));
    }
    
    if is_link_command {
        if emit_llvmir_opt.is_none() {
            let status = Command::new(&clang_path)
                .args(&args[1..])
                .status()
                .expect(&format!("Failed to execute {}", clang_path.display()));
            exit(status.code().unwrap_or(1));
        }
        handle_link_command(&clang_path, &args, &llvm_ir_dir, emit_llvmir_opt.as_deref(), debug_mode);
    }

    // Parse compilation arguments
    let mut input_file: Option<PathBuf> = None;
    let mut output_file: Option<PathBuf> = None;
    let mut other_args: Vec<String> = Vec::new();
    let mut i = 1;

    let options_with_arg = [
        "-MT", "-MF", "-MQ", "-MD", "-MMD",
        "-I", "-L", "-l", "-D", "-U",
        "-include", "-imacros", "-idirafter", "-iprefix", "-iwithprefix",
        "-iwithprefixbefore", "-isystem", "-iquote", "-isysroot",
        "-framework", "-F",
        "-x", "-std", "-arch",
        "-target", "-mllvm",
        "--param", "-Xclang",
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
        } else if !arg.starts_with('-') && input_file.is_none() {
            if Path::new(arg).exists() {
                input_file = Some(PathBuf::from(arg));
            } else {
                other_args.push(arg.clone());
            }
        } else {
            other_args.push(arg.clone());
        }
        
        i += 1;
    }

    let output_path = match &output_file {
        Some(path) => path.clone(),
        None => {
            if let Some(ref input) = input_file {
                let stem = input.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("output");
                PathBuf::from(format!("{}.o", stem))
            } else {
                PathBuf::from("output.o")
            }
        }
    };

    // First pass: normal compilation
    let mut normal_cmd = Command::new(&clang_path);
    normal_cmd.args(&args[1..]);
    
    debug_log(debug_mode, &format!("[DEBUG] Normal compilation: {} {}", clang_path.display(), args[1..].join(" ")));
    
    let normal_result = normal_cmd.status();
    
    match normal_result {
        Ok(status) if status.success() => {},
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
    
    // Get target architecture and determine if -march option needs to be added
    // Use full compilation command to get target architecture
    let march_opt = get_compile_target(&clang_path, &args[1..])
        .and_then(|target| get_march_for_target(&target));
    
    if let Some(march) = march_opt {
        debug_log(debug_mode, &format!("[DEBUG] Adding march option for LLVM IR generation: {}", march));
    }
    
    let mut llvm_cmd = Command::new(&clang_path);
    llvm_cmd.args(&other_args);
    
    for opt in &extra_opts {
        llvm_cmd.arg(opt);
    }
    
    // Add -march option (if detected as needed)
    if let Some(march) = march_opt {
        llvm_cmd.arg(march);
    }
    
    llvm_cmd.arg("-emit-llvm");
    
    if let Some(ref input) = input_file {
        llvm_cmd.arg(input);
    }
    
    let abs_output = get_absolute_path(&output_path);
    
    let mut ir_path = PathBuf::from(&llvm_ir_dir);
    let rel_output = abs_output.strip_prefix("/")
        .unwrap_or(&abs_output);
    ir_path.push(rel_output);
    
    let llvm_ir_path = ir_path;
    
    if let Err(e) = ensure_dir_exists(&llvm_ir_path) {
        debug_log(debug_mode, &format!("[DEBUG] Warning: Failed to create LLVM IR output directory: {}", e));
        exit(0);
    }

    let has_explicit_output = output_file.is_some() || input_file.is_some();
    
    let (ir_output_path, use_tmp_file) = if has_explicit_output {
        (llvm_ir_path.clone(), false)
    } else {
        let tmp_suffix = generate_tmp_suffix();
        (append_tmp_suffix(&llvm_ir_path, &tmp_suffix), true)
    };

    llvm_cmd.arg("-o").arg(&ir_output_path);

    {
        let mut cmd_args: Vec<String> = other_args.clone();
        for opt in &extra_opts {
            cmd_args.push(opt.clone());
        }
        if let Some(march) = march_opt {
            cmd_args.push(march.to_string());
        }
        cmd_args.push("-emit-llvm".to_string());
        if let Some(ref input) = input_file {
            cmd_args.push(input.display().to_string());
        }
        cmd_args.push("-o".to_string());
        cmd_args.push(ir_output_path.display().to_string());
        debug_log(debug_mode, &format!("[DEBUG] LLVM IR generation: {} {}", clang_path.display(), cmd_args.join(" ")));
    }

    let log_path = format!("{}_log", llvm_ir_path.display());
    
    if let Err(e) = ensure_dir_exists(&PathBuf::from(&log_path)) {
        debug_log(debug_mode, &format!("[DEBUG] Warning: Failed to create log file directory: {}", e));
        exit(0);
    }
    
    let mut log_file = match File::create(&log_path) {
        Ok(f) => f,
        Err(e) => {
            debug_log(debug_mode, &format!("[DEBUG] Warning: Failed to create log file {}: {}", log_path, e));
            exit(0);
        }
    };

    let mut cmd_parts: Vec<String> = vec![clang_path.display().to_string()];
    cmd_parts.extend(other_args.clone());
    for opt in &extra_opts {
        cmd_parts.push(opt.clone());
    }
    cmd_parts.push("-emit-llvm".to_string());
    if let Some(ref input) = input_file {
        cmd_parts.push(input.display().to_string());
    }
    cmd_parts.push("-o".to_string());
    cmd_parts.push(ir_output_path.display().to_string());
    
    let cmd_str = cmd_parts.join(" ");
    let _ = writeln!(log_file, "Command: {}", cmd_str);

    llvm_cmd.stdout(Stdio::from(log_file.try_clone().unwrap()));
    llvm_cmd.stderr(Stdio::from(log_file));

    let llvm_result = llvm_cmd.status();

    match llvm_result {
        Ok(status) if status.success() => {
            if use_tmp_file {
                if let Err(_e) = fs::rename(&ir_output_path, &llvm_ir_path) {
                    if let Err(copy_err) = fs::copy(&ir_output_path, &llvm_ir_path) {
                        eprintln!("Warning: Failed to move temp LLVM IR file: {} -> {}: copy error: {}", 
                                  ir_output_path.display(), llvm_ir_path.display(), copy_err);
                        let _ = fs::remove_file(&ir_output_path);
                        exit(0);
                    }
                    let _ = fs::remove_file(&ir_output_path);
                }
            }
            exit(0);
        }
        Ok(_status) => {
            debug_log(debug_mode, &format!("[DEBUG] Warning: LLVM IR generation failed, see {} for details", log_path));
            let _ = fs::remove_file(&ir_output_path);
            exit(0);
        }
        Err(e) => {
            debug_log(debug_mode, &format!("[DEBUG] Warning: Failed to execute LLVM IR generation: {}", e));
            let _ = fs::remove_file(&ir_output_path);
            exit(0);
        }
    }
}