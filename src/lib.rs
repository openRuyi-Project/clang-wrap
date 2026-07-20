// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! Common library module - provides shared functionality for clang-wrap tools
//!
//! Includes the following public features:
//! - Debug logging
//! - Finding executables in PATH
//! - Environment variable reading
//! - LLVM IR path calculation
//! - Finding related files (_log, _cmd, _verscript)

pub mod install;

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// ============================================================================
// Debug logging functionality
// ============================================================================

/// Global debug log file path
pub static DEBUG_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Initialize debug logging
/// If debug mode is enabled, set the log file path
pub fn init_debug_log(llvmir_dir: &str) {
    let debug_log_path = PathBuf::from(llvmir_dir).join("clang-wrap-debug.log");
    // Ensure directory exists
    if let Some(parent) = debug_log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = DEBUG_LOG_PATH.set(debug_log_path);
}

/// Write to debug log (if debug mode is enabled)
pub fn debug_log(debug_mode: bool, msg: &str) {
    if !debug_mode {
        return;
    }
    if let Some(log_path) = DEBUG_LOG_PATH.get() {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
            let _ = writeln!(file, "{}", msg);
        }
    }
}

// ============================================================================
// Environment variable reading
// ============================================================================

/// Get LLVM IR output directory
/// Prefer LLVM_IR_DIR environment variable, defaults to ~/tmp/llvmir
pub fn get_llvm_ir_dir() -> String {
    env::var("LLVM_IR_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let home = env::var("HOME").expect("HOME environment variable not set");
            format!("{}/tmp/llvmir", home)
        })
}

/// Check if debug mode is enabled
pub fn is_debug_mode() -> bool {
    env::var("CLANG_WRAP_DEBUG")
        .ok()
        .filter(|s| !s.is_empty() && s != "0")
        .is_some()
}

/// Check if LLVM IR generation is enabled
pub fn is_emit_llvmir_enabled() -> bool {
    env::var("EMIT_LLVMIR")
        .ok()
        .filter(|s| !s.is_empty() && s != "0")
        .is_some()
}

/// Get the value of EMIT_LLVMIR (may contain extra options)
pub fn get_emit_llvmir_opt() -> Option<String> {
    env::var("EMIT_LLVMIR")
        .ok()
        .filter(|s| !s.is_empty() && s != "0")
}

// ============================================================================
// Finding executables in PATH
// ============================================================================

/// Find the specified executable in PATH, skipping the current executable
/// Returns the absolute path of the found executable
pub fn find_exe_in_path(exe_name: &str, current_exe: &Path) -> Option<PathBuf> {
    // Get PATH environment variable
    let path_env = env::var("PATH").ok()?;

    // Resolve the real path of the current executable (resolve symbolic links)
    let current_exe_real = current_exe.canonicalize().ok()?;

    // Iterate through each directory in PATH
    for dir in path_env.split(':') {
        let candidate = PathBuf::from(dir).join(exe_name);

        // Check if file exists and is executable
        if candidate.exists() {
            // Resolve the real path of the candidate file
            if let Ok(candidate_real) = candidate.canonicalize() {
                // Skip self (by comparing real paths)
                if candidate_real == current_exe_real {
                    continue;
                }
            }

            // Found the real executable
            return Some(candidate);
        }
    }

    None
}

/// Get the executable path to use (skip self)
pub fn get_exe_path(exe_name: &str) -> PathBuf {
    // Get the current executable path
    match env::current_exe() {
        Ok(current_exe) => {
            // Try to find in PATH, skipping self
            match find_exe_in_path(exe_name, &current_exe) {
                Some(path) => path,
                None => {
                    // If not found, fall back to using name directly (let system find in PATH)
                    eprintln!(
                        "Warning: Could not find {} in PATH (skipping self)",
                        exe_name
                    );
                    PathBuf::from(exe_name)
                }
            }
        }
        Err(_) => {
            // Cannot get current executable path, fall back to using name directly
            PathBuf::from(exe_name)
        }
    }
}

/// Find LLVM tool in PATH
/// If clang_cmd has a version suffix (e.g. clang-22), prefer finding tool with version suffix (e.g. llvm-link-22)
pub fn find_llvm_tool(tool_name: &str, clang_cmd: &str) -> PathBuf {
    // Try to extract version suffix from clang_cmd
    // e.g.: clang-22 -> -22, clang++-22 -> -22
    let version_suffix = if let Some(rest) = clang_cmd.strip_prefix("clang") {
        // Check if there's a version suffix (starting with - or +)
        if rest.starts_with('-') || rest.starts_with('+') {
            rest.to_string()
        } else if !rest.is_empty() && rest.chars().next().map(|c| c.is_numeric()).unwrap_or(false) {
            // clang22 -> -22
            format!("-{}", rest)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // If there's a version suffix, first try to find tool with version suffix
    if !version_suffix.is_empty() {
        let versioned_tool = format!("{}{}", tool_name, version_suffix);
        if let Ok(path_env) = env::var("PATH") {
            for dir in path_env.split(':') {
                let candidate = PathBuf::from(dir).join(&versioned_tool);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }

    // Try to find tool without version suffix
    if let Ok(path_env) = env::var("PATH") {
        for dir in path_env.split(':') {
            let candidate = PathBuf::from(dir).join(tool_name);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    // If not found, return tool name and let system handle it
    PathBuf::from(tool_name)
}

// ============================================================================
// Path processing utilities
// ============================================================================

/// Get the absolute path of a path
pub fn get_absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

/// Compute path in llvmir (generic implementation)
/// File path mapping: llvmir_dir + absolute path of original path (remove leading /)
pub fn compute_llvmir_path(path: &Path, llvmir_dir: &str) -> PathBuf {
    let abs_path = get_absolute_path(path);
    let mut ir_path = PathBuf::from(llvmir_dir);
    ir_path.push(abs_path.strip_prefix("/").unwrap_or(&abs_path));
    ir_path
}

/// Compute target directory path in llvmir (alias, same functionality as compute_llvmir_path)
pub fn compute_llvmir_target_dir(target_dir: &Path, llvmir_dir: &str) -> PathBuf {
    compute_llvmir_path(target_dir, llvmir_dir)
}

// ============================================================================
// Finding LLVM IR related files
// ============================================================================

/// Find corresponding LLVM IR file in llvmir directory
/// File path mapping: llvmir_dir + absolute path of original file (remove leading /)
pub fn find_llvmir_file(file_path: &Path, llvmir_dir: &str) -> Option<PathBuf> {
    find_llvmir_file_impl(file_path, llvmir_dir, false)
}

/// Find corresponding LLVM IR file in llvmir directory (with debug output)
pub fn find_llvmir_file_with_debug(
    file_path: &Path,
    llvmir_dir: &str,
    debug: bool,
) -> Option<PathBuf> {
    find_llvmir_file_impl(file_path, llvmir_dir, debug)
}

/// Internal implementation for finding LLVM IR files
fn find_llvmir_file_impl(file_path: &Path, llvmir_dir: &str, debug: bool) -> Option<PathBuf> {
    debug_log(
        debug,
        &format!("[DEBUG] find_llvmir_file: looking for {:?}", file_path),
    );

    let abs_path = get_absolute_path(file_path);
    debug_log(debug, &format!("[DEBUG]   absolute path: {:?}", abs_path));

    let mut ir_path = PathBuf::from(llvmir_dir);
    let rel_path = abs_path.strip_prefix("/").unwrap_or(&abs_path);
    ir_path.push(rel_path);
    debug_log(debug, &format!("[DEBUG]   llvmir path: {:?}", ir_path));

    if ir_path.exists() {
        debug_log(debug, &format!("[DEBUG]   Found at: {:?}", ir_path));
        return Some(ir_path);
    }

    // Check .bc extension (LLVM bitcode)
    debug_log(debug, "[DEBUG]   Not found, trying .bc extension");
    let ir_path_bc = ir_path.with_extension("bc");
    if ir_path_bc.exists() {
        debug_log(debug, &format!("[DEBUG]   Found at: {:?}", ir_path_bc));
        return Some(ir_path_bc);
    }

    debug_log(debug, "[DEBUG]   Not found");
    None
}

/// Auxiliary file type suffix
#[derive(Debug, Clone, Copy)]
pub enum AuxFileSuffix {
    Log,
    Cmd,
    Verscript,
}

impl AuxFileSuffix {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuxFileSuffix::Log => "_log",
            AuxFileSuffix::Cmd => "_cmd",
            AuxFileSuffix::Verscript => "_verscript",
        }
    }
}

/// Find corresponding auxiliary file (_log, _cmd, _verscript)
pub fn find_aux_file(file_path: &Path, suffix: AuxFileSuffix) -> Option<PathBuf> {
    let aux_path = PathBuf::from(format!("{}{}", file_path.display(), suffix.as_str()));
    if aux_path.exists() {
        Some(aux_path)
    } else {
        None
    }
}

/// Find corresponding _log file
pub fn find_log_file(file_path: &Path) -> Option<PathBuf> {
    find_aux_file(file_path, AuxFileSuffix::Log)
}

/// Find corresponding _cmd file
pub fn find_cmd_file(file_path: &Path) -> Option<PathBuf> {
    find_aux_file(file_path, AuxFileSuffix::Cmd)
}

/// Find corresponding _verscript file
pub fn find_verscript_file(file_path: &Path) -> Option<PathBuf> {
    find_aux_file(file_path, AuxFileSuffix::Verscript)
}

/// Find all related auxiliary files (_log, _cmd, _verscript)
pub fn find_all_aux_files(file_path: &Path) -> Vec<(AuxFileSuffix, PathBuf)> {
    [
        AuxFileSuffix::Log,
        AuxFileSuffix::Cmd,
        AuxFileSuffix::Verscript,
    ]
    .iter()
    .filter_map(|&suffix| find_aux_file(file_path, suffix).map(|p| (suffix, p)))
    .collect()
}

// ============================================================================
// Program name handling
// ============================================================================

/// Get program name from arguments
pub fn get_program_name(args: &[String]) -> &str {
    Path::new(&args[0])
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
}

/// Determine the actual command name to invoke
/// If program name is xxx-wrap, use xxx
/// Otherwise use the actual program name
pub fn resolve_cmd_name<'a>(
    program_name: &'a str,
    wrap_suffix: &str,
    default_cmd: &'a str,
) -> &'a str {
    if program_name == wrap_suffix {
        default_cmd
    } else {
        program_name
    }
}

// ============================================================================
// File operation utilities
// ============================================================================

/// Ensure directory exists
pub fn ensure_dir_exists(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Copy file (for llvmir directory)
pub fn copy_file(source: &Path, dest: &Path, debug: bool) -> i32 {
    debug_log(
        debug,
        &format!(
            "[DEBUG] Copying llvmir file: {} -> {}",
            source.display(),
            dest.display()
        ),
    );

    // Ensure destination directory exists
    if let Err(e) = ensure_dir_exists(dest) {
        eprintln!("Warning: Failed to create llvmir directory: {}", e);
        return 1;
    }

    match fs::copy(source, dest) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!(
                "Warning: Failed to copy {} to {}: {}",
                source.display(),
                dest.display(),
                e
            );
            1
        }
    }
}

/// Copy and modify _cmd file content
/// Replace source file name with destination file name
pub fn copy_and_modify_cmd_file(
    source: &Path,
    dest: &Path,
    source_name: &str,
    dest_name: &str,
    debug: bool,
) -> i32 {
    debug_log(
        debug,
        &format!(
            "[DEBUG] Copying and modifying cmd file: {} -> {}",
            source.display(),
            dest.display()
        ),
    );

    // Read source file content
    let content = match fs::read_to_string(source) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Warning: Failed to read cmd file {}: {}",
                source.display(),
                e
            );
            return 1;
        }
    };

    // Replace file names
    let mut modified = content
        .replace(
            &format!("# Original output: {}", source_name),
            &format!("# Original output: {}", dest_name),
        )
        .replace(&format!("./{}", source_name), &format!("./{}", dest_name))
        .replace(
            &format!("output/{}", source_name),
            &format!("output/{}", dest_name),
        );

    // Replace _verscript file references
    modified = modified.replace(
        &format!("{}_verscript", source_name),
        &format!("{}_verscript", dest_name),
    );

    // Ensure destination directory exists
    if let Err(e) = ensure_dir_exists(dest) {
        eprintln!("Warning: Failed to create llvmir directory: {}", e);
        return 1;
    }

    // Write to destination file
    match fs::write(dest, modified) {
        Ok(_) => {
            // Copy executable permission
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = fs::metadata(source) {
                    let mut perms = metadata.permissions();
                    perms.set_mode(perms.mode() | 0o755);
                    let _ = fs::set_permissions(dest, perms);
                }
            }
            0
        }
        Err(e) => {
            eprintln!(
                "Warning: Failed to write cmd file {}: {}",
                dest.display(),
                e
            );
            1
        }
    }
}

// ============================================================================
// File operation command execution (for llvmir sync with cp/mv/ln)
// ============================================================================

use std::process::Command;

/// Generic implementation for executing file operation commands (for llvmir directory sync)
pub fn execute_file_cmd_for_llvmir(
    cmd_path: &Path,
    cmd_name: &str, // "cp", "mv", or "ln"
    source: &Path,
    dest: &Path,
    other_args: &[String],
    skip_args: &[&str], // Parameter prefixes to skip
    debug: bool,
) -> i32 {
    let mut args: Vec<String> = other_args
        .iter()
        .filter(|arg| !skip_args.iter().any(|skip| arg.starts_with(skip)))
        .cloned()
        .collect();

    let symbolic_link = cmd_name == "ln"
        && other_args.iter().any(|arg| {
            arg == "--symbolic"
                || arg
                    .strip_prefix('-')
                    .filter(|rest| !rest.starts_with('-'))
                    .is_some_and(|rest| rest.contains('s'))
        });

    if symbolic_link {
        let link_dir = dest.parent().unwrap_or(Path::new("."));
        let relative_source =
            pathdiff::diff_paths(source, link_dir).unwrap_or_else(|| source.to_path_buf());
        args.push(relative_source.to_string_lossy().to_string());
    } else {
        args.push(source.to_string_lossy().to_string());
    }
    args.push(dest.to_string_lossy().to_string());

    debug_log(
        debug,
        &format!(
            "[DEBUG] Executing {} for llvmir: {} {}",
            cmd_name,
            cmd_path.display(),
            args.join(" ")
        ),
    );

    match Command::new(cmd_path).args(&args).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("Warning: Failed to execute {} for llvmir: {}", cmd_name, e);
            1
        }
    }
}

/// Sync copy/move llvmir files and their auxiliary files (_log, _cmd, _verscript)
pub fn sync_llvmir_with_aux_files(
    cmd_path: &Path,
    cmd_name: &str,
    llvmir_source: &Path,
    llvmir_dest: &Path,
    other_args: &[String],
    skip_args: &[&str],
    debug: bool,
) {
    // Execute main file operation
    execute_file_cmd_for_llvmir(
        cmd_path,
        cmd_name,
        llvmir_source,
        llvmir_dest,
        other_args,
        skip_args,
        debug,
    );

    // Sync auxiliary files
    for (suffix, aux_source) in find_all_aux_files(llvmir_source) {
        let aux_dest = PathBuf::from(format!("{}{}", llvmir_dest.display(), suffix.as_str()));
        debug_log(
            debug,
            &format!(
                "[DEBUG] Syncing aux file: {} -> {}",
                aux_source.display(),
                aux_dest.display()
            ),
        );
        execute_file_cmd_for_llvmir(
            cmd_path,
            cmd_name,
            &aux_source,
            &aux_dest,
            other_args,
            skip_args,
            debug,
        );
    }
}

// ============================================================================
// Timestamp generation
// ============================================================================

use std::time::{SystemTime, UNIX_EPOCH};

/// Generate temporary file suffix based on timestamp and process ID
pub fn generate_tmp_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!(".tmp.{}.{}", pid, timestamp)
}

/// Append temporary file suffix to path
pub fn append_tmp_suffix(path: &Path, suffix: &str) -> PathBuf {
    let path_str = path.to_string_lossy();
    PathBuf::from(format!("{}{}", path_str, suffix))
}

// ============================================================================
// Shell escaping
// ============================================================================

/// Escape shell argument
pub fn shell_escape(s: &str) -> String {
    // If string contains special characters, wrap with single quotes
    if s.contains(' ')
        || s.contains('"')
        || s.contains('\'')
        || s.contains('\\')
        || s.contains('$')
        || s.contains('`')
        || s.contains('!')
        || s.contains('*')
        || s.contains('?')
        || s.contains('[')
        || s.contains(']')
        || s.contains('(')
        || s.contains(')')
        || s.contains('{')
        || s.contains('}')
        || s.contains('|')
        || s.contains('&')
        || s.contains(';')
        || s.contains('<')
        || s.contains('>')
        || s.contains('~')
    {
        // Inside single quotes everything is literal except single quote itself
        // Single quote needs to be represented with '\''
        format!("'{}'", s.replace("'", "'\\''"))
    } else {
        s.to_string()
    }
}

// ============================================================================
// @file argument expansion
// ============================================================================

fn split_response_args(content: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = content.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                } else {
                    current.push(ch);
                }
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ch if ch.is_whitespace() && !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

/// Expand @file arguments
/// @file syntax reads arguments from file, one per line
pub fn expand_at_file_args(args: &[String], debug_mode: bool) -> Vec<String> {
    let mut expanded_args: Vec<String> = Vec::new();

    for arg in args {
        if let Some(file_path) = arg.strip_prefix('@') {
            // Get file path (remove @ prefix)
            match fs::read_to_string(file_path) {
                Ok(content) => {
                    let parsed_args = split_response_args(&content);
                    let args_read = parsed_args.len();
                    expanded_args.extend(parsed_args);

                    debug_log(
                        debug_mode,
                        &format!("[DEBUG] Expanded @{}: {} arguments", file_path, args_read),
                    );
                }
                Err(e) => {
                    eprintln!("Warning: Failed to open @file '{}': {}", file_path, e);
                    // If file doesn't exist, keep original argument
                    expanded_args.push(arg.clone());
                }
            }
        } else {
            expanded_args.push(arg.clone());
        }
    }

    expanded_args
}
