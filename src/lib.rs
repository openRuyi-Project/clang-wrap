//! 公共库模块 - 提供 clang-wrap 工具的共享功能
//!
//! 包含以下公共功能：
//! - 调试日志
//! - PATH 中查找可执行文件
//! - 环境变量读取
//! - LLVM IR 路径计算
//! - 查找相关文件（_log, _cmd, _verscript）

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// ============================================================================
// 调试日志功能
// ============================================================================

/// 全局调试日志文件路径
pub static DEBUG_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// 初始化调试日志
/// 如果启用了调试模式，设置日志文件路径
pub fn init_debug_log(llvmir_dir: &str) {
    let debug_log_path = PathBuf::from(llvmir_dir).join("clang-wrap-debug.log");
    // 确保目录存在
    if let Some(parent) = debug_log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = DEBUG_LOG_PATH.set(debug_log_path);
}

/// 写入调试日志（如果启用了调试模式）
pub fn debug_log(debug_mode: bool, msg: &str) {
    if !debug_mode {
        return;
    }
    if let Some(log_path) = DEBUG_LOG_PATH.get() {
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
        {
            let _ = writeln!(file, "{}", msg);
        }
    }
}

// ============================================================================
// 环境变量读取
// ============================================================================

/// 获取 LLVM IR 输出目录
/// 优先使用 LLVM_IR_DIR 环境变量，默认为 ~/tmp/llvmir
pub fn get_llvm_ir_dir() -> String {
    env::var("LLVM_IR_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let home = env::var("HOME").expect("HOME environment variable not set");
            format!("{}/tmp/llvmir", home)
        })
}

/// 检查是否启用调试模式
pub fn is_debug_mode() -> bool {
    env::var("CLANG_WRAP_DEBUG")
        .ok()
        .filter(|s| !s.is_empty() && s != "0")
        .is_some()
}

/// 检查是否启用 LLVM IR 生成功能
pub fn is_emit_llvmir_enabled() -> bool {
    env::var("EMIT_LLVMIR")
        .ok()
        .filter(|s| !s.is_empty() && s != "0")
        .is_some()
}

/// 获取 EMIT_LLVMIR 的值（可能包含额外选项）
pub fn get_emit_llvmir_opt() -> Option<String> {
    env::var("EMIT_LLVMIR")
        .ok()
        .filter(|s| !s.is_empty() && s != "0")
}

// ============================================================================
// PATH 中查找可执行文件
// ============================================================================

/// 在 PATH 中查找指定的可执行文件，跳过当前可执行文件
/// 返回找到的可执行文件的绝对路径
pub fn find_exe_in_path(exe_name: &str, current_exe: &Path) -> Option<PathBuf> {
    // 获取 PATH 环境变量
    let path_env = env::var("PATH").ok()?;
    
    // 解析当前可执行文件的真实路径（解析符号链接）
    let current_exe_real = current_exe.canonicalize().ok()?;
    
    // 遍历 PATH 中的每个目录
    for dir in path_env.split(':') {
        let candidate = PathBuf::from(dir).join(exe_name);
        
        // 检查文件是否存在且可执行
        if candidate.exists() {
            // 解析候选文件的真实路径
            if let Ok(candidate_real) = candidate.canonicalize() {
                // 跳过自己（通过真实路径比较）
                if candidate_real == current_exe_real {
                    continue;
                }
            }
            
            // 找到了真正的可执行文件
            return Some(candidate);
        }
    }
    
    None
}

/// 获取要使用的可执行文件路径（跳过自己）
pub fn get_exe_path(exe_name: &str) -> PathBuf {
    // 获取当前可执行文件的路径
    match env::current_exe() {
        Ok(current_exe) => {
            // 尝试在 PATH 中查找，跳过自己
            match find_exe_in_path(exe_name, &current_exe) {
                Some(path) => path,
                None => {
                    // 如果找不到，回退到直接使用名称（让系统在 PATH 中查找）
                    eprintln!("Warning: Could not find {} in PATH (skipping self)", exe_name);
                    PathBuf::from(exe_name)
                }
            }
        }
        Err(_) => {
            // 无法获取当前可执行文件路径，回退到直接使用名称
            PathBuf::from(exe_name)
        }
    }
}

/// 在 PATH 中查找 LLVM 工具
/// 如果 clang_cmd 有版本后缀（如 clang-22），则优先查找带版本后缀的工具（如 llvm-link-22）
pub fn find_llvm_tool(tool_name: &str, clang_cmd: &str) -> PathBuf {
    // 尝试从 clang_cmd 提取版本后缀
    // 例如: clang-22 -> -22, clang++-22 -> -22
    let version_suffix = if clang_cmd.starts_with("clang") {
        let rest = &clang_cmd[5..]; // 跳过 "clang"
        // 检查是否有版本后缀（以 - 或 + 开头）
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
    
    // 如果有版本后缀，先尝试查找带版本后缀的工具
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
    
    // 尝试查找不带版本后缀的工具
    if let Ok(path_env) = env::var("PATH") {
        for dir in path_env.split(':') {
            let candidate = PathBuf::from(dir).join(tool_name);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    
    // 如果找不到，返回工具名称，让系统处理
    PathBuf::from(tool_name)
}

// ============================================================================
// 路径处理工具
// ============================================================================

/// 获取路径的绝对路径
pub fn get_absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

/// 计算 llvmir 中路径（通用实现）
/// 文件路径映射：llvmir_dir + 原始路径的绝对路径（去掉前导 /）
pub fn compute_llvmir_path(path: &Path, llvmir_dir: &str) -> PathBuf {
    let abs_path = get_absolute_path(path);
    let mut ir_path = PathBuf::from(llvmir_dir);
    ir_path.push(abs_path.strip_prefix("/").unwrap_or(&abs_path));
    ir_path
}

/// 计算 llvmir 中目标目录的路径（别名，与 compute_llvmir_path 功能相同）
pub fn compute_llvmir_target_dir(target_dir: &Path, llvmir_dir: &str) -> PathBuf {
    compute_llvmir_path(target_dir, llvmir_dir)
}

// ============================================================================
// 查找 LLVM IR 相关文件
// ============================================================================

/// 在 llvmir 目录中查找对应的 LLVM IR 文件
/// 文件路径映射：llvmir_dir + 原始文件的绝对路径（去掉前导 /）
pub fn find_llvmir_file(file_path: &Path, llvmir_dir: &str) -> Option<PathBuf> {
    find_llvmir_file_impl(file_path, llvmir_dir, false)
}

/// 在 llvmir 目录中查找对应的 LLVM IR 文件（带调试输出）
pub fn find_llvmir_file_with_debug(file_path: &Path, llvmir_dir: &str, debug: bool) -> Option<PathBuf> {
    find_llvmir_file_impl(file_path, llvmir_dir, debug)
}

/// 查找 LLVM IR 文件的内部实现
fn find_llvmir_file_impl(file_path: &Path, llvmir_dir: &str, debug: bool) -> Option<PathBuf> {
    debug_log(debug, &format!("[DEBUG] find_llvmir_file: looking for {:?}", file_path));
    
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
    
    // 检查 .bc 扩展名（LLVM bitcode）
    debug_log(debug, &format!("[DEBUG]   Not found, trying .bc extension"));
    let ir_path_bc = ir_path.with_extension("bc");
    if ir_path_bc.exists() {
        debug_log(debug, &format!("[DEBUG]   Found at: {:?}", ir_path_bc));
        return Some(ir_path_bc);
    }
    
    debug_log(debug, "[DEBUG]   Not found");
    None
}

/// 辅助文件类型后缀
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

/// 查找对应的辅助文件（_log, _cmd, _verscript）
pub fn find_aux_file(file_path: &Path, suffix: AuxFileSuffix) -> Option<PathBuf> {
    let aux_path = PathBuf::from(format!("{}{}", file_path.display(), suffix.as_str()));
    if aux_path.exists() { Some(aux_path) } else { None }
}

/// 查找对应的 _log 文件
pub fn find_log_file(file_path: &Path) -> Option<PathBuf> {
    find_aux_file(file_path, AuxFileSuffix::Log)
}

/// 查找对应的 _cmd 文件
pub fn find_cmd_file(file_path: &Path) -> Option<PathBuf> {
    find_aux_file(file_path, AuxFileSuffix::Cmd)
}

/// 查找对应的 _verscript 文件
pub fn find_verscript_file(file_path: &Path) -> Option<PathBuf> {
    find_aux_file(file_path, AuxFileSuffix::Verscript)
}

/// 查找所有相关的辅助文件（_log, _cmd, _verscript）
pub fn find_all_aux_files(file_path: &Path) -> Vec<(AuxFileSuffix, PathBuf)> {
    [AuxFileSuffix::Log, AuxFileSuffix::Cmd, AuxFileSuffix::Verscript]
        .iter()
        .filter_map(|&suffix| {
            find_aux_file(file_path, suffix).map(|p| (suffix, p))
        })
        .collect()
}

// ============================================================================
// 程序名处理
// ============================================================================

/// 从参数中获取程序名
pub fn get_program_name(args: &[String]) -> &str {
    Path::new(&args[0])
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
}

/// 确定实际要调用的命令名
/// 如果程序名是 xxx-wrap，则使用 xxx
/// 否则使用实际的程序名
pub fn resolve_cmd_name<'a>(program_name: &'a str, wrap_suffix: &str, default_cmd: &'a str) -> &'a str {
    if program_name == wrap_suffix {
        default_cmd
    } else {
        program_name
    }
}

// ============================================================================
// 文件操作工具
// ============================================================================

/// 确保目录存在
pub fn ensure_dir_exists(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// 复制文件（用于 llvmir 目录）
pub fn copy_file(source: &Path, dest: &Path, debug: bool) -> i32 {
    debug_log(debug, &format!("[DEBUG] Copying llvmir file: {} -> {}", 
              source.display(), dest.display()));
    
    // 确保目标目录存在
    if let Err(e) = ensure_dir_exists(dest) {
        eprintln!("Warning: Failed to create llvmir directory: {}", e);
        return 1;
    }
    
    match fs::copy(source, dest) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("Warning: Failed to copy {} to {}: {}", 
                      source.display(), dest.display(), e);
            1
        }
    }
}

/// 复制并修改 _cmd 文件内容
/// 将源文件名替换为目标文件名
pub fn copy_and_modify_cmd_file(
    source: &Path,
    dest: &Path,
    source_name: &str,
    dest_name: &str,
    debug: bool,
) -> i32 {
    debug_log(debug, &format!("[DEBUG] Copying and modifying cmd file: {} -> {}", 
              source.display(), dest.display()));
    
    // 读取源文件内容
    let content = match fs::read_to_string(source) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: Failed to read cmd file {}: {}", source.display(), e);
            return 1;
        }
    };
    
    // 替换文件名
    let mut modified = content
        .replace(&format!("# Original output: {}", source_name), 
                 &format!("# Original output: {}", dest_name))
        .replace(&format!("./{}", source_name), 
                 &format!("./{}", dest_name))
        .replace(&format!("output/{}", source_name), 
                 &format!("output/{}", dest_name));
    
    // 替换 _verscript 文件引用
    modified = modified
        .replace(&format!("{}_verscript", source_name), 
                 &format!("{}_verscript", dest_name));
    
    // 确保目标目录存在
    if let Err(e) = ensure_dir_exists(dest) {
        eprintln!("Warning: Failed to create llvmir directory: {}", e);
        return 1;
    }
    
    // 写入目标文件
    match fs::write(dest, modified) {
        Ok(_) => {
            // 复制可执行权限
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
            eprintln!("Warning: Failed to write cmd file {}: {}", dest.display(), e);
            1
        }
    }
}

// ============================================================================
// 文件操作命令执行（用于 cp/mv/ln 的 llvmir 同步）
// ============================================================================

use std::process::Command;

/// 执行文件操作命令的通用实现（用于 llvmir 目录同步）
pub fn execute_file_cmd_for_llvmir(
    cmd_path: &Path,
    cmd_name: &str,  // "cp", "mv", 或 "ln"
    source: &Path,
    dest: &Path,
    other_args: &[String],
    skip_args: &[&str],  // 需要跳过的参数前缀
    debug: bool,
) -> i32 {
    let mut args: Vec<String> = other_args.iter()
        .filter(|arg| !skip_args.iter().any(|skip| arg.starts_with(skip)))
        .cloned()
        .collect();
    
    // ln 命令需要计算相对路径
    if cmd_name == "ln" {
        let link_dir = dest.parent().unwrap_or(Path::new("."));
        let relative_source = pathdiff::diff_paths(source, link_dir)
            .unwrap_or_else(|| source.to_path_buf());
        args.push(relative_source.to_string_lossy().to_string());
    } else {
        args.push(source.to_string_lossy().to_string());
    }
    args.push(dest.to_string_lossy().to_string());
    
    debug_log(debug, &format!("[DEBUG] Executing {} for llvmir: {} {}", 
              cmd_name, cmd_path.display(), args.join(" ")));
    
    match Command::new(cmd_path).args(&args).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("Warning: Failed to execute {} for llvmir: {}", cmd_name, e);
            1
        }
    }
}

/// 同步复制/移动 llvmir 文件及其辅助文件（_log, _cmd, _verscript）
pub fn sync_llvmir_with_aux_files(
    cmd_path: &Path,
    cmd_name: &str,
    llvmir_source: &Path,
    llvmir_dest: &Path,
    other_args: &[String],
    skip_args: &[&str],
    debug: bool,
) {
    // 执行主文件操作
    execute_file_cmd_for_llvmir(cmd_path, cmd_name, llvmir_source, llvmir_dest, other_args, skip_args, debug);
    
    // 同步辅助文件
    for (suffix, aux_source) in find_all_aux_files(llvmir_source) {
        let aux_dest = PathBuf::from(format!("{}{}", llvmir_dest.display(), suffix.as_str()));
        debug_log(debug, &format!("[DEBUG] Syncing aux file: {} -> {}", 
                  aux_source.display(), aux_dest.display()));
        execute_file_cmd_for_llvmir(cmd_path, cmd_name, &aux_source, &aux_dest, other_args, skip_args, debug);
    }
}

// ============================================================================
// 时间戳生成
// ============================================================================

use std::time::{SystemTime, UNIX_EPOCH};

/// 生成临时文件后缀，基于时间戳和进程 ID
pub fn generate_tmp_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!(".tmp.{}.{}", pid, timestamp)
}

/// 为路径添加临时文件后缀
pub fn append_tmp_suffix(path: &Path, suffix: &str) -> PathBuf {
    let path_str = path.to_string_lossy();
    PathBuf::from(format!("{}{}", path_str, suffix))
}

// ============================================================================
// Shell 转义
// ============================================================================

/// 对 shell 参数进行转义
pub fn shell_escape(s: &str) -> String {
    // 如果字符串包含特殊字符，用单引号包围
    if s.contains(' ') || s.contains('"') || s.contains('\'') || s.contains('\\') 
       || s.contains('$') || s.contains('`') || s.contains('!') || s.contains('*')
       || s.contains('?') || s.contains('[') || s.contains(']') || s.contains('(')
       || s.contains(')') || s.contains('{') || s.contains('}') || s.contains('|')
       || s.contains('&') || s.contains(';') || s.contains('<') || s.contains('>')
       || s.contains('~') {
        // 单引号内除了单引号本身外都是字面量
        // 单引号需要用 '\'' 来表示
        format!("'{}'", s.replace("'", "'\\''"))
    } else {
        s.to_string()
    }
}

// ============================================================================
// @file 参数展开
// ============================================================================

use std::io::{BufRead, BufReader};

/// 展开 @file 参数
/// @file 语法用于从文件中读取参数，每行一个参数
pub fn expand_at_file_args(args: &[String], debug_mode: bool) -> Vec<String> {
    let mut expanded_args: Vec<String> = Vec::new();
    
    for arg in args {
        if arg.starts_with('@') {
            // 获取文件路径（去掉 @ 前缀）
            let file_path = &arg[1..];
            
            // 尝试打开并读取文件
            match File::open(file_path) {
                Ok(file) => {
                    let reader = BufReader::new(file);
                    let mut lines_read = 0;
                    
                    for line in reader.lines() {
                        match line {
                            Ok(l) => {
                                // 去掉首尾空白字符
                                let trimmed = l.trim();
                                if !trimmed.is_empty() {
                                    // 按空白字符分割，支持一行多个参数的情况
                                    for part in trimmed.split_whitespace() {
                                        expanded_args.push(part.to_string());
                                        lines_read += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Warning: Failed to read line from {}: {}", file_path, e);
                            }
                        }
                    }
                    
                    debug_log(debug_mode, &format!("[DEBUG] Expanded @{}: {} arguments", file_path, lines_read));
                }
                Err(e) => {
                    eprintln!("Warning: Failed to open @file '{}': {}", file_path, e);
                    // 如果文件不存在，保留原始参数
                    expanded_args.push(arg.clone());
                }
            }
        } else {
            expanded_args.push(arg.clone());
        }
    }
    
    expanded_args
}