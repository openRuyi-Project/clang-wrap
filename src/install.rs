// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! Shared LLVM IR installation support for install and meson-install.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{debug_log, get_absolute_path};

fn llvmir_path(path: &Path, llvmir_dir: &str) -> PathBuf {
    let absolute = get_absolute_path(path);
    PathBuf::from(llvmir_dir).join(absolute.strip_prefix("/").unwrap_or(&absolute))
}

fn add_if_exists(files: &mut Vec<PathBuf>, path: PathBuf) {
    if path.exists() && !files.contains(&path) {
        files.push(path);
    }
}

fn find_llvmir_files(source: &Path, llvmir_dir: &str) -> Vec<PathBuf> {
    let absolute = get_absolute_path(source);
    let resolved = fs::canonicalize(&absolute).unwrap_or_else(|_| absolute.clone());
    let mut bases = vec![llvmir_path(&absolute, llvmir_dir)];
    if resolved != absolute {
        bases.push(llvmir_path(&resolved, llvmir_dir));
    }

    let mut files = Vec::new();
    for base in &bases {
        add_if_exists(&mut files, base.clone());
        add_if_exists(&mut files, base.with_extension("bc"));
        for suffix in ["_cmd", "_verscript"] {
            add_if_exists(
                &mut files,
                PathBuf::from(format!("{}{suffix}", base.display())),
            );
        }
    }

    if let Some(name) = source.file_name().and_then(|name| name.to_str()) {
        if let Some(name) = name.strip_suffix('T') {
            if let Some(parent) = bases[0].parent() {
                for suffix in ["", ".bc", "_cmd", "_verscript"] {
                    add_if_exists(&mut files, parent.join(format!("{name}{suffix}")));
                }
            }
        }
    }
    files
}

fn is_shared_library(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    !name.ends_with(".o")
        && (name.contains(".so.")
            || name.ends_with(".so")
            || name.ends_with(".dylib")
            || name.contains(".dylib."))
}

fn mode_has_execute_bit(mode: &str) -> bool {
    let mode = mode.trim().strip_prefix('0').unwrap_or(mode.trim());
    !mode.is_empty()
        && mode.chars().all(|ch| ('0'..='7').contains(&ch))
        && mode
            .chars()
            .rev()
            .take(3)
            .any(|ch| matches!(ch, '1' | '3' | '5' | '7'))
}

fn is_executable(path: &Path, mode: Option<&str>) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if is_shared_library(path) || name.ends_with(".o") || name.ends_with(".a") {
        return false;
    }
    if path.to_string_lossy().contains("/bin/") || mode.is_some_and(mode_has_execute_bit) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

fn llvmir_install_dir(installed_path: &Path, shared: bool, executable: bool) -> PathBuf {
    let parent = installed_path.parent().unwrap_or(installed_path);
    let parent_string = parent.to_string_lossy();
    if shared {
        if let Some(index) = parent_string.find("/lib/") {
            return PathBuf::from(format!("{}/llvmir", &parent_string[..index + 4]));
        }
        return parent.join("llvmir");
    }
    if executable {
        if let Some(index) = parent_string.find("/bin/") {
            return PathBuf::from(format!("{}/lib/llvmir-bin", &parent_string[..index]));
        }
        return parent
            .parent()
            .map(|prefix| prefix.join("lib/llvmir-bin"))
            .unwrap_or_else(|| parent.join("llvmir-bin"));
    }
    parent.join("llvmir")
}

fn extract_soname(files: &[PathBuf]) -> Option<String> {
    let cmd = files.iter().find(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_cmd"))
    })?;
    fs::read_to_string(cmd)
        .ok()?
        .split_whitespace()
        .find_map(|arg| {
            arg.strip_prefix("-Wl,-soname,")
                .or_else(|| arg.strip_prefix("-Wl,-soname="))
                .or_else(|| arg.strip_prefix("-Wl,-h,"))
                .or_else(|| arg.strip_prefix("-Wl,-h="))
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn destination_filename(file: &Path, files: &[PathBuf], source: &Path, installed: &Path) -> String {
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if is_shared_library(installed) {
        let base = extract_soname(files).unwrap_or_else(|| {
            installed
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string()
        });
        if name.ends_with("_cmd") {
            return format!("{base}_cmd");
        }
        if name.ends_with("_verscript") {
            return format!("{base}_verscript");
        }
        if name.ends_with(".bc") {
            return format!("{base}.bc");
        }
        return base;
    }
    if source
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with('T'))
    {
        return name.strip_suffix('T').unwrap_or(name).to_string();
    }
    name.to_string()
}

fn rewrite_cmd(content: &str, source_name: &str, destination_name: &str) -> String {
    content
        .replace(
            &format!("# Original output: {source_name}"),
            &format!("# Original output: {destination_name}"),
        )
        .replace(
            &format!("./{source_name}"),
            &format!("./{destination_name}"),
        )
        .replace(
            &format!("output/{source_name}"),
            &format!("output/{destination_name}"),
        )
        .replace(
            &format!("{source_name}_verscript"),
            &format!("{destination_name}_verscript"),
        )
}

fn copy_llvmir_file(source: &Path, destination: &Path) -> io::Result<()> {
    let is_cmd = source
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("_cmd"));
    if is_cmd {
        let source_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let destination_name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        fs::write(
            destination,
            rewrite_cmd(
                &fs::read_to_string(source)?,
                source_name.strip_suffix("_cmd").unwrap_or(source_name),
                destination_name
                    .strip_suffix("_cmd")
                    .unwrap_or(destination_name),
            ),
        )?;
    } else {
        fs::copy(source, destination)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(destination)?.permissions();
        permissions.set_mode(if is_cmd { 0o755 } else { 0o644 });
        fs::set_permissions(destination, permissions)?;
    }
    Ok(())
}

/// Synchronize the LLVM IR, `_cmd`, and `_verscript` files associated
/// with a build product to the location corresponding to its installed path.
pub fn sync_installed_llvmir(
    source: &Path,
    installed_path: &Path,
    llvmir_dir: &str,
    mode: Option<&str>,
    debug: bool,
) -> io::Result<()> {
    let files = find_llvmir_files(source, llvmir_dir);
    if files.is_empty() {
        debug_log(
            debug,
            &format!("[DEBUG] No LLVM IR found for {}", source.display()),
        );
        return Ok(());
    }
    let destination_dir = llvmir_install_dir(
        installed_path,
        is_shared_library(source) || is_shared_library(installed_path),
        is_executable(source, mode) || is_executable(installed_path, mode),
    );
    fs::create_dir_all(&destination_dir)?;
    for file in &files {
        let destination =
            destination_dir.join(destination_filename(file, &files, source, installed_path));
        debug_log(
            debug,
            &format!(
                "[DEBUG] Installing LLVM IR: {} -> {}",
                file.display(),
                destination.display()
            ),
        );
        copy_llvmir_file(file, &destination)?;
    }
    Ok(())
}
