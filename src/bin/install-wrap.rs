// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! GNU install wrapper that synchronizes associated LLVM IR files.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use clang_wrap::install::sync_installed_llvmir;
use clang_wrap::{
    debug_log, get_exe_path, get_llvm_ir_dir, get_program_name, is_debug_mode, resolve_cmd_name,
};

struct InstallArgs {
    create_dirs: bool,
    target_dir: Option<PathBuf>,
    sources: Vec<PathBuf>,
    destination: Option<PathBuf>,
    mode: Option<String>,
}

fn parse_install_args(args: &[String]) -> InstallArgs {
    let mut result = InstallArgs {
        create_dirs: false,
        target_dir: None,
        sources: Vec::new(),
        destination: None,
        mode: None,
    };
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if arg == "-d" || arg == "--directory" {
            result.create_dirs = true;
        } else if arg == "-t" || arg == "--target-directory" {
            index += 1;
            if let Some(value) = args.get(index) {
                result.target_dir = Some(PathBuf::from(value));
            }
        } else if arg == "-m" || arg == "--mode" {
            index += 1;
            if let Some(value) = args.get(index) {
                result.mode = Some(value.clone());
            }
        } else if let Some(value) = arg.strip_prefix("--target-directory=") {
            result.target_dir = Some(PathBuf::from(value));
        } else if arg.starts_with("-t") && arg.len() > 2 {
            result.target_dir = Some(PathBuf::from(&arg[2..]));
        } else if let Some(value) = arg.strip_prefix("--mode=") {
            result.mode = Some(value.to_string());
        } else if arg.starts_with("-m") && arg.len() > 2 {
            result.mode = Some(arg[2..].to_string());
        } else if !arg.starts_with('-') {
            result.sources.push(PathBuf::from(arg));
        }
        index += 1;
    }

    if !result.create_dirs && result.target_dir.is_none() && result.sources.len() > 1 {
        result.destination = result.sources.pop();
    }
    result
}

fn installed_path(source: &Path, destination: &Path) -> PathBuf {
    if destination.is_dir() {
        source
            .file_name()
            .map(|name| destination.join(name))
            .unwrap_or_else(|| destination.to_path_buf())
    } else {
        destination.to_path_buf()
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program_name = get_program_name(&args);
    let install_path = get_exe_path(resolve_cmd_name(program_name, "install-wrap", "install"));

    let status = Command::new(&install_path)
        .args(&args[1..])
        .status()
        .unwrap_or_else(|_| panic!("Failed to execute {}", install_path.display()));
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }

    let parsed = parse_install_args(&args[1..]);
    if parsed.create_dirs {
        return;
    }

    let debug = is_debug_mode();
    let llvmir_dir = get_llvm_ir_dir();
    for source in &parsed.sources {
        let destination = match (&parsed.target_dir, &parsed.destination) {
            (Some(target_dir), _) => target_dir.join(source.file_name().unwrap_or_default()),
            (None, Some(destination)) => installed_path(source, destination),
            (None, None) => continue,
        };
        if let Err(error) = sync_installed_llvmir(
            source,
            &destination,
            &llvmir_dir,
            parsed.mode.as_deref(),
            debug,
        ) {
            debug_log(
                debug,
                &format!(
                    "[DEBUG] Failed to install LLVM IR for {}: {error}",
                    source.display()
                ),
            );
            eprintln!(
                "Warning: Failed to install LLVM IR for {}: {error}",
                source.display()
            );
        }
    }
}
