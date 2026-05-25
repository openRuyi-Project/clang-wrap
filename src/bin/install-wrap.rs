// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! install wrapper
//!
//! After executing install command, synchronously install corresponding files in llvmir directory

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use clang_wrap::{debug_log, get_exe_path, get_llvm_ir_dir, is_debug_mode,
    get_program_name, resolve_cmd_name, get_absolute_path};

/// Parse install command arguments
struct InstallArgs {
    /// -d or --directory: treat all arguments as directory names, create them
    create_dirs: bool,
    /// -D: create leading components of dest, then copy source to dest
    create_leading: bool,
    target_dir: Option<PathBuf>,
    sources: Vec<PathBuf>,
    destination: Option<PathBuf>,
    no_target_dir: bool,
    mode: Option<String>,
    other_args: Vec<String>,
}

fn parse_install_args(args: &[String]) -> InstallArgs {
    let mut result = InstallArgs {
        create_dirs: false,
        create_leading: false,
        target_dir: None,
        sources: Vec::new(),
        destination: None,
        no_target_dir: false,
        mode: None,
        other_args: Vec::new(),
    };
    
    let mut i = 0;
    let n = args.len();
    
    while i < n {
        let arg = &args[i];
        
        if arg == "-d" || arg == "--directory" {
            result.create_dirs = true;
            result.other_args.push(arg.clone());
        } else if arg == "-D" {
            // -D: create leading components of dest, then copy source to dest
            result.create_leading = true;
            result.other_args.push(arg.clone());
        } else if arg == "-t" {
            if i + 1 < n {
                result.target_dir = Some(PathBuf::from(&args[i + 1]));
                result.other_args.push(arg.clone());
                result.other_args.push(args[i + 1].clone());
                i += 1;
            }
        } else if arg.starts_with("-t") && arg.len() > 2 {
            result.target_dir = Some(PathBuf::from(&arg[2..]));
            result.other_args.push(arg.clone());
        } else if arg == "-T" {
            result.no_target_dir = true;
            result.other_args.push(arg.clone());
        } else if arg == "-m" {
            if i + 1 < n {
                result.mode = Some(args[i + 1].clone());
                result.other_args.push(arg.clone());
                result.other_args.push(args[i + 1].clone());
                i += 1;
            }
        } else if arg.starts_with("-m") && arg.len() > 2 {
            result.mode = Some(arg[2..].to_string());
            result.other_args.push(arg.clone());
        } else if arg.starts_with("--mode=") {
            result.mode = Some(arg[7..].to_string());
            result.other_args.push(arg.clone());
        } else if arg.starts_with('-') {
            result.other_args.push(arg.clone());
            if arg.starts_with("--target-directory=") {
                result.target_dir = Some(PathBuf::from(&arg[18..]));
            }
        } else {
            result.sources.push(PathBuf::from(arg));
        }
        
        i += 1;
    }
    
    if !result.create_dirs && result.target_dir.is_none() && result.sources.len() > 1 {
        result.destination = result.sources.pop();
    }
    
    result
}

/// Find corresponding LLVM IR files in llvmir directory
fn find_llvmir_files_internal(source: &Path, llvmir_dir: &str, source_basename: Option<&str>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    
    let abs_source = get_absolute_path(source);
    
    // If source file is a symlink, get its resolved path
    let abs_source_resolved = if source.is_symlink() {
        if let Ok(target) = abs_source.read_link() {
            let parent = abs_source.parent().unwrap_or(&abs_source);
            parent.join(target)
        } else {
            abs_source.clone()
        }
    } else {
        abs_source.clone()
    };
    
    // Build path in llvmir
    let mut ir_path = PathBuf::from(llvmir_dir);
    let rel_path = abs_source.strip_prefix("/")
        .unwrap_or(&abs_source);
    ir_path.push(rel_path);
    
    // If source file is a symlink, also build llvmir path for resolved path
    let ir_path_resolved = if abs_source_resolved != abs_source {
        let mut resolved = PathBuf::from(llvmir_dir);
        let rel_resolved = abs_source_resolved.strip_prefix("/")
            .unwrap_or(&abs_source_resolved);
        resolved.push(rel_resolved);
        Some(resolved)
    } else {
        None
    };
    
    // 1. Check LLVM bitcode file with same name
    if ir_path.exists() {
        result.push(ir_path.clone());
    }
    
    let mut bc_path = ir_path.clone();
    bc_path.set_extension("bc");
    if bc_path.exists() && !result.contains(&bc_path) {
        result.push(bc_path);
    }
    
    // 2. Check _cmd file
    let cmd_path = PathBuf::from(format!("{}_cmd", ir_path.display()));
    if cmd_path.exists() {
        result.push(cmd_path);
    }
    
    // If source file is a symlink, also check _cmd file for resolved path
    if let Some(ref ir_path_resolved) = ir_path_resolved {
        let cmd_path_resolved = PathBuf::from(format!("{}_cmd", ir_path_resolved.display()));
        if cmd_path_resolved.exists() && !result.contains(&cmd_path_resolved) {
            result.push(cmd_path_resolved);
        }
        
        if ir_path_resolved.exists() && !result.contains(ir_path_resolved) {
            result.push(ir_path_resolved.clone());
        }
    }
    
    // If source filename ends with T, also need to find _cmd file without T suffix
    if let Some(basename) = source_basename {
        if basename.ends_with('T') {
            let base_without_t = &basename[..basename.len()-1];
            let ir_dir = ir_path.parent().unwrap_or(&ir_path);
            let cmd_path_without_t = ir_dir.join(format!("{}_cmd", base_without_t));
            if cmd_path_without_t.exists() && !result.contains(&cmd_path_without_t) {
                result.push(cmd_path_without_t);
            }
        }
    }
    
    // 3. Check verscript file
    let vs_path = PathBuf::from(format!("{}_verscript", ir_path.display()));
    if vs_path.exists() && !result.contains(&vs_path) {
        result.push(vs_path);
    }
    
    if let Some(ref ir_path_resolved) = ir_path_resolved {
        let vs_path_resolved = PathBuf::from(format!("{}_verscript", ir_path_resolved.display()));
        if vs_path_resolved.exists() && !result.contains(&vs_path_resolved) {
            result.push(vs_path_resolved);
        }
    }
    
    result
}

/// Find corresponding LLVM IR files in llvmir directory
fn find_llvmir_files(source: &Path, llvmir_dir: &str) -> Vec<PathBuf> {
    let basename = source.file_name()
        .and_then(|n| n.to_str());
    find_llvmir_files_internal(source, llvmir_dir, basename)
}

/// Determine if source file is a shared library
fn is_shared_library(path: &Path) -> bool {
    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    
    if filename.ends_with(".o") {
        return false;
    }
    
    if filename.contains(".so") {
        let parts: Vec<&str> = filename.splitn(2, ".so").collect();
        if parts.len() == 2 {
            let suffix = parts[1];
            if suffix.is_empty() || suffix.starts_with('.') {
                return true;
            }
        }
    }
    
    if filename.ends_with(".dylib") || filename.contains(".dylib.") {
        return true;
    }
    
    false
}

/// Extract soname from _cmd file content
///
/// Look for arguments in -Wl,-soname,xxx or -Wl,-h,xxx format
fn extract_soname_from_cmd_content(cmd_content: &str) -> Option<String> {
    // Iterate through each argument in command line
    for part in cmd_content.split_whitespace() {
        // Check -Wl,-soname,xxx or -Wl,-soname=xxx format
        if part.starts_with("-Wl,-soname,") || part.starts_with("-Wl,-soname=") {
            let soname = if part.starts_with("-Wl,-soname,") {
                &part[12..]  // length of "-Wl,-soname,"
            } else {
                &part[12..]  // length of "-Wl,-soname=" is also 12
            };
            if !soname.is_empty() {
                return Some(soname.to_string());
            }
        }
        
        // Check -Wl,-h,xxx or -Wl,-h=xxx format (-h is shorthand for soname)
        if part.starts_with("-Wl,-h,") || part.starts_with("-Wl,-h=") {
            let soname = if part.starts_with("-Wl,-h,") {
                &part[7..]  // length of "-Wl,-h,"
            } else {
                &part[7..]  // length of "-Wl,-h=" is also 7
            };
            if !soname.is_empty() {
                return Some(soname.to_string());
            }
        }
        
        // Check -soname xxx format (separate arguments)
        if part == "-soname" || part == "-h" {
            // Next argument is soname, but this requires more complex parsing
            // Skip for now, mainly handle -Wl,-soname,xxx format
        }
    }
    
    None
}

/// Extract soname from _cmd file
///
/// Read _cmd file content, look for -Wl,-soname,xxx format arguments
fn extract_soname_from_cmd_file(cmd_file: &Path) -> Option<String> {
    if let Ok(content) = fs::read_to_string(cmd_file) {
        extract_soname_from_cmd_content(&content)
    } else {
        None
    }
}

/// Find _cmd file from llvmir file list
fn find_cmd_file(llvmir_files: &[PathBuf]) -> Option<PathBuf> {
    for f in llvmir_files {
        let name = f.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if name.ends_with("_cmd") {
            return Some(f.clone());
        }
    }
    None
}

/// Determine if source file is an executable program
fn is_executable(path: &Path) -> bool {
    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    
    if is_shared_library(path) || filename.ends_with(".o") || filename.ends_with(".a") {
        return false;
    }
    
    let path_str = path.to_string_lossy();
    if path_str.contains("/bin/") {
        return true;
    }
    
    if path.exists() {
        if let Ok(metadata) = fs::metadata(path) {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode();
            if mode & 0o111 != 0 {
                return true;
            }
        }
    }
    
    false
}

/// Determine LLVM IR install target directory
///
/// When dest is a file path (e.g., installing libfoo.so to /usr/lib/libfoo.so.1.2.3),
/// use the parent directory. When dest is a directory, use it directly.
///
/// Note on install command options:
/// - `-d` or `--directory`: treat all arguments as directory names, create them (no file copy)
/// - `-D`: create leading components of dest (parent dirs), then copy source to dest as a file
/// - Without `-d` or `-D`:
///   - If dest exists and is a directory → dest is a directory
///   - If dest does not exist → dest is a file (install will create it as a file)
///   - If dest exists and is a file → dest is a file
fn get_llvmir_install_dir(dest: &Path, is_shared: bool, is_exec: bool) -> PathBuf {
    // Determine if dest is a file or directory
    // After install command executes:
    // - If dest exists: check if it's file or directory
    // - If dest doesn't exist yet (shouldn't happen after install): treat as file
    let dest_is_file = if dest.exists() {
        dest.is_file()
    } else {
        // If dest doesn't exist yet, install will create it as a file
        // (not a directory, since -d mode is handled separately and exits early)
        true
    };
    
    // Get the actual directory to use
    let actual_dir = if dest_is_file {
        dest.parent().unwrap_or(dest)
    } else {
        dest
    };
    
    let actual_dir_str = actual_dir.to_string_lossy();
    
    if is_shared {
        if actual_dir_str.contains("/lib/") {
            if let Some(idx) = actual_dir_str.find("/lib/") {
                let prefix = &actual_dir_str[..idx + 4];
                return PathBuf::from(format!("{}/llvmir", prefix));
            }
        }
        actual_dir.join("llvmir")
    } else if is_exec {
        if actual_dir_str.contains("/bin/") {
            if let Some(idx) = actual_dir_str.find("/bin/") {
                let prefix = &actual_dir_str[..idx];
                return PathBuf::from(format!("{}/lib/llvmir-bin", prefix));
            }
        }
        actual_dir.parent()
            .map(|p| p.join("lib").join("llvmir-bin"))
            .unwrap_or_else(|| actual_dir.join("llvmir-bin"))
    } else {
        actual_dir.join("llvmir")
    }
}

/// Execute original install command
fn execute_install(install_path: &Path, args: &[String], debug: bool) -> i32 {
    debug_log(debug, &format!("[DEBUG] Executing install: {} {}", install_path.display(), args.join(" ")));
    
    let status = Command::new(install_path)
        .args(args)
        .status()
        .expect(&format!("Failed to execute {}", install_path.display()));
    
    status.code().unwrap_or(1)
}

/// Install single LLVM IR file
fn install_llvmir_file(
    install_path: &Path,
    source: &Path,
    dest_filename: Option<&str>,
    dest_dir: &Path,
    debug: bool,
) -> i32 {
    if let Err(e) = fs::create_dir_all(dest_dir) {
        eprintln!("Warning: Failed to create directory {}: {}", dest_dir.display(), e);
        return 1;
    }
    
    let source_filename = source.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    
    let target_filename = dest_filename.unwrap_or(source_filename);
    
    let mut args = Vec::new();
    
    if target_filename.ends_with("_cmd") {
        args.push("-m".to_string());
        args.push("755".to_string());
    } else {
        args.push("-m".to_string());
        args.push("644".to_string());
    }
    
    args.push(source.to_string_lossy().to_string());
    args.push(dest_dir.join(target_filename).to_string_lossy().to_string());
    
    debug_log(debug, &format!("[DEBUG] Installing LLVM IR: {} -> {}/{}", 
              source.display(), dest_dir.display(), target_filename));
    
    let status = Command::new(install_path)
        .args(&args)
        .status();
    
    match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("Warning: Failed to install LLVM IR file {}: {}", source.display(), e);
            1
        }
    }
}

/// Compute llvmir file destination filename based on source and destination filenames
///
/// When destination filename differs from source (e.g., source is libfoo.so, target is libfoo.so.1.2.3),
/// llvmir file should use soname extracted from _cmd file as base name
/// Example: source libavcodec.so, target libavcodec.so.62.11.100
/// _cmd file contains -Wl,-soname,libavcodec.so.62
/// llvmir files should be named: libavcodec.so.62_cmd, libavcodec.so.62_verscript, libavcodec.so.62.bc
fn compute_llvmir_dest_filename(llvmir_source: &Path, llvmir_files: &[PathBuf], original_source: &Path, dest: &Path) -> Option<String> {
    let llvmir_name = llvmir_source.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    
    let source_name = original_source.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    
    let dest_name = dest.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    
    // Handle case where source filename ends with T (e.g., temp files used by gcc driver)
    if source_name.ends_with('T') {
        if llvmir_name.ends_with('T') {
            let base_name = &llvmir_name[..llvmir_name.len()-1];
            return Some(base_name.to_string());
        }
    }
    
    // If it's a shared library and target name differs from source, use soname as base
    // Example: source libavcodec.so, target libavcodec.so.62.11.100
    // Extract soname from _cmd file: libavcodec.so.62
    // llvmir files should be named: libavcodec.so.62_cmd, libavcodec.so.62_verscript, libavcodec.so.62.bc
    if is_shared_library(dest) && dest_name != source_name {
        // Extract soname from _cmd file
        let soname = if let Some(cmd_file) = find_cmd_file(llvmir_files) {
            extract_soname_from_cmd_file(&cmd_file)
        } else {
            None
        };
        
        // If soname not found from _cmd file, use target filename
        let base_name = soname.as_deref().unwrap_or(dest_name);
        
        // Determine llvmir file suffix type
        let llvmir_suffix = if llvmir_name.ends_with("_cmd") {
            "_cmd"
        } else if llvmir_name.ends_with("_verscript") {
            "_verscript"
        } else if llvmir_name.ends_with(".bc") {
            ".bc"
        } else {
            ""
        };
        
        if llvmir_suffix.is_empty() {
            // For files without special suffix (e.g., .so file itself), use soname
            return Some(base_name.to_string());
        } else if llvmir_suffix == ".bc" {
            // For .bc files, replace original extension
            // Example: libavcodec.so.bc -> libavcodec.so.62.bc
            return Some(format!("{}.bc", base_name));
        } else {
            // For _cmd and _verscript files, directly append suffix to soname
            return Some(format!("{}{}", base_name, llvmir_suffix));
        }
    }
    
    None
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    // Get program name
    let program_name = get_program_name(&args);
    
    // Determine the actual command to invoke
    let install_cmd = resolve_cmd_name(program_name, "install-wrap", "install");
    
    // Get the real install path (skip self)
    let install_path = get_exe_path(install_cmd);
    
    if args.len() < 2 {
        let status = Command::new(&install_path)
            .status()
            .expect(&format!("Failed to execute {}", install_path.display()));
        exit(status.code().unwrap_or(1));
    }
    
    let debug = is_debug_mode();
    let llvmir_dir = get_llvm_ir_dir();
    
    // Parse arguments
    let parsed = parse_install_args(&args[1..]);
    
    debug_log(debug, &format!("[DEBUG] Parsed install args:"));
    debug_log(debug, &format!("[DEBUG]   create_dirs: {}", parsed.create_dirs));
    debug_log(debug, &format!("[DEBUG]   sources: {:?}", parsed.sources));
    debug_log(debug, &format!("[DEBUG]   destination: {:?}", parsed.destination));
    
    // First execute original install command
    let install_result = execute_install(&install_path, &args[1..], debug);
    
    if install_result != 0 {
        exit(install_result);
    }
    
    // If -d mode (create directory), no need to process LLVM IR
    if parsed.create_dirs {
        exit(0);
    }
    
    // Case 1: Use -t DIRECTORY to specify target directory
    if let Some(ref target_dir) = parsed.target_dir {
        for source in &parsed.sources {
            let llvmir_files = find_llvmir_files(source, &llvmir_dir);
            
            if llvmir_files.is_empty() {
                continue;
            }
            
            let is_shared = is_shared_library(source);
            let is_exec = is_executable(source);
            
            let llvmir_install_dir = get_llvmir_install_dir(target_dir, is_shared, is_exec);
            
            debug_log(debug, &format!("[DEBUG] Source: {}", source.display()));
            debug_log(debug, &format!("[DEBUG]   LLVM IR install dir: {}", llvmir_install_dir.display()));
            
            for llvmir_file in &llvmir_files {
                let dest_filename = compute_llvmir_dest_filename(llvmir_file, &llvmir_files, source, target_dir);
                
                install_llvmir_file(
                    &install_path,
                    llvmir_file,
                    dest_filename.as_deref(),
                    &llvmir_install_dir,
                    debug,
                );
            }
        }
        exit(0);
    }
    
    // Case 2: Multiple source files, last argument is target directory
    if parsed.sources.len() > 1 && parsed.destination.is_some() {
        let dest = parsed.destination.as_ref().unwrap();
        
        if dest.is_dir() {
            for source in &parsed.sources {
                let llvmir_files = find_llvmir_files(source, &llvmir_dir);
                
                if llvmir_files.is_empty() {
                    continue;
                }
                
                let is_shared = is_shared_library(source);
                let is_exec = is_executable(source);
                
                let llvmir_install_dir = get_llvmir_install_dir(dest, is_shared, is_exec);
                
                for llvmir_file in &llvmir_files {
                    let dest_filename = compute_llvmir_dest_filename(llvmir_file, &llvmir_files, source, dest);
                    
                    install_llvmir_file(
                        &install_path,
                        llvmir_file,
                        dest_filename.as_deref(),
                        &llvmir_install_dir,
                        debug,
                    );
                }
            }
        }
        exit(0);
    }
    
    // Case 3: Single source file, target is filename or directory
    if parsed.sources.len() == 1 {
        let source = &parsed.sources[0];
        
        let llvmir_files = find_llvmir_files(source, &llvmir_dir);
        
        if llvmir_files.is_empty() {
            exit(0);
        }
        
        let is_shared = is_shared_library(source);
        let is_exec = is_executable(source);
        
        let dest = if let Some(ref destination) = parsed.destination {
            destination.clone()
        } else if let Some(ref target_dir) = parsed.target_dir {
            let filename = source.file_name()
                .expect("Source should have a filename");
            target_dir.join(filename)
        } else {
            exit(0);
        };
        
        let llvmir_install_dir = get_llvmir_install_dir(&dest, is_shared, is_exec);
        
        for llvmir_file in &llvmir_files {
            let dest_filename = compute_llvmir_dest_filename(llvmir_file, &llvmir_files, source, &dest);
            
            install_llvmir_file(
                &install_path,
                llvmir_file,
                dest_filename.as_deref(),
                &llvmir_install_dir,
                debug,
            );
        }
    }
    
    exit(0);
}