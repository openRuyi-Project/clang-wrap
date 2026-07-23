// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! Shared LLVM IR installation support for install and meson-install.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::{debug_log, find_llvm_tool, get_absolute_path};

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
        for suffix in ["_cmd", "_verscript", "_missing"] {
            add_if_exists(
                &mut files,
                PathBuf::from(format!("{}{suffix}", base.display())),
            );
        }
    }

    if let Some(name) = source.file_name().and_then(|name| name.to_str()) {
        if let Some(name) = name.strip_suffix('T') {
            if let Some(parent) = bases[0].parent() {
                for suffix in ["", ".bc", "_cmd", "_verscript", "_missing"] {
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
    if path.to_string_lossy().contains("/bin/")
        || path.to_string_lossy().contains("/sbin/")
        || path.to_string_lossy().contains("/libexec/")
        || mode.is_some_and(mode_has_execute_bit)
    {
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
        let parent_name = parent.file_name().and_then(|name| name.to_str());
        if parent_name == Some("sbin") {
            return parent
                .parent()
                .map(|prefix| prefix.join("lib/llvmir-sbin"))
                .unwrap_or_else(|| parent.join("llvmir-sbin"));
        }
        if parent_name == Some("libexec") {
            return parent
                .parent()
                .map(|prefix| prefix.join("lib/llvmir-libexec"))
                .unwrap_or_else(|| parent.join("llvmir-libexec"));
        }
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
        if name.ends_with("_missing") {
            return format!("{base}_missing");
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

fn missing_paths(path: &Path) -> io::Result<Vec<PathBuf>> {
    fs::read_to_string(path).map(|content| {
        content
            .lines()
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect()
    })
}

fn append_missing_objects_to_archive(archive: &Path, missing: &[PathBuf]) -> io::Result<()> {
    if missing.is_empty() {
        return Ok(());
    }
    for object in missing {
        if !object.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing native object {}", object.display()),
            ));
        }
    }

    let llvm_ar = find_llvm_tool("llvm-ar", "clang");
    let status = Command::new(&llvm_ar)
        .arg("rcsD")
        .arg(archive)
        .args(missing)
        .status()
        .map_err(|error| {
            io::Error::other(format!("failed to execute {}: {error}", llvm_ar.display()))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{} failed with status {status}",
            llvm_ar.display()
        )))
    }
}

fn inject_asm_objects_into_cmd(cmd_destination: &Path, missing_source: &Path) -> io::Result<()> {
    if !cmd_destination.exists() || !missing_source.exists() {
        return Ok(());
    }

    let missing_content = fs::read_to_string(missing_source)?;
    let absolute_paths: Vec<PathBuf> = missing_content
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| PathBuf::from(line.trim()))
        .filter(|p| p.is_absolute())
        .collect();

    if absolute_paths.is_empty() {
        return Ok(());
    }

    // Create asm/ subdirectory relative to the cmd file's directory.
    let asm_dir = cmd_destination
        .parent()
        .unwrap_or(Path::new("."))
        .join("asm");
    fs::create_dir_all(&asm_dir)?;

    let cmd_content = fs::read_to_string(cmd_destination)?;

    let lines: Vec<&str> = cmd_content.lines().collect();
    let cmd_open_index = lines
        .iter()
        .position(|line| line.trim_start().starts_with("cmd=("));
    let Some(cmd_open_index) = cmd_open_index else {
        return Ok(());
    };
    let start_idx = cmd_open_index + 1;
    let Some(end_idx_rel) = (start_idx..lines.len()).find(|idx| lines[*idx].trim() == ")") else {
        return Ok(());
    };
    let end_idx = end_idx_rel;

    let before: Vec<String> = lines[..start_idx]
        .iter()
        .map(|line| format!("{line}\n"))
        .collect();
    let after: Vec<String> = lines[end_idx..]
        .iter()
        .map(|line| format!("{line}\n"))
        .collect();

    // Match CMakeEmitLLVMIR: absolute object-path SHA-256 prefix plus basename.
    let mut path_to_asm: std::collections::HashMap<PathBuf, (String, String)> =
        std::collections::HashMap::new();
    for abs_path in &absolute_paths {
        if abs_path.is_file() {
            let hash = sha256_path_prefix(abs_path);
            let basename = abs_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.o");
            let asm_name = format!("{}-{}", hash, basename);
            let asm_dest = asm_dir.join(&asm_name);
            if let Err(e) = fs::copy(abs_path, &asm_dest) {
                eprintln!(
                    "Warning: failed to copy {} to asm/{}: {}",
                    abs_path.display(),
                    asm_name,
                    e
                );
                continue;
            }
            path_to_asm.insert(abs_path.clone(), (hash, basename.to_string()));
        }
    }

    // Rebuild the cmd array: skip libmixed.a references, skip absolute-path
    // object entries (they will be replaced), save --output=.
    let mut middle: Vec<String> = Vec::with_capacity(end_idx - start_idx + path_to_asm.len() + 2);
    let mut output_args: Vec<String> = Vec::new();
    for &raw in &lines[start_idx..end_idx] {
        let trimmed = raw.trim_start().trim_end();
        if trimmed.is_empty() {
            continue;
        }
        // Skip static library references.
        if trimmed.ends_with("libmixed.a") || trimmed.ends_with("libmixed.aT") {
            continue;
        }
        // Save --output= for the end.
        if trimmed.starts_with("--output=") {
            output_args.push(format!("  {trimmed}\n"));
            continue;
        }
        // Skip absolute-path -Wl,<abs_path> lines; they are replaced below.
        if trimmed.starts_with("-Wl,/") {
            continue;
        }
        middle.push(format!("{raw}\n"));
    }

    // Append the asm/ entries in deterministic order.
    let mut asm_entries: Vec<(String, String)> = path_to_asm.values().cloned().collect();
    asm_entries.sort_by_key(|(hash, basename)| (basename.clone(), hash.clone()));
    for (hash, basename) in &asm_entries {
        middle.push(format!("  ./asm/{}-{}\n", hash, basename));
    }

    if output_args.len() > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "multiple --output= arguments in cmd file: {} ({} occurrences)",
                cmd_destination.display(),
                output_args.len()
            ),
        ));
    }
    if let Some(output_arg) = output_args.into_iter().next() {
        middle.push(output_arg);
    }

    let mut output = String::new();
    output.push_str(&before.concat());
    output.push_str(&middle.concat());
    output.push_str(&after.concat());

    fs::write(cmd_destination, output)?;

    Ok(())
}

fn sha256_path_prefix(path: &Path) -> String {
    let digest = Sha256::digest(path.as_os_str().as_encoded_bytes());
    hex::encode(digest)[..12].to_string()
}

/// Synchronize the LLVM IR and auxiliary files associated
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
    let static_library = source.extension().is_some_and(|extension| extension == "a");
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
        let is_missing = file
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_missing"));

        if is_missing {
            continue;
        }
        copy_llvmir_file(file, &destination)?;
    }

    if static_library {
        if let Some(missing_file) = files.iter().find(|file| {
            file.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_missing"))
        }) {
            let archive = destination_dir.join(destination_filename(
                &llvmir_path(source, llvmir_dir),
                &files,
                source,
                installed_path,
            ));
            append_missing_objects_to_archive(&archive, &missing_paths(missing_file)?)?;
        }
    } else if let (Some(cmd_file), Some(missing_file)) = (
        files.iter().find(|file| {
            file.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_cmd"))
        }),
        files.iter().find(|file| {
            file.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_missing"))
        }),
    ) {
        let cmd_destination = destination_dir.join(destination_filename(
            cmd_file,
            &files,
            source,
            installed_path,
        ));
        inject_asm_objects_into_cmd(&cmd_destination, missing_file)?;
    }
    Ok(())
}
