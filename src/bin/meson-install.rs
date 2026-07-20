// SPDX-FileCopyrightText: (C) 2026 Institute of Software, Chinese Academy of Sciences (ISCAS)
// SPDX-FileCopyrightText: (C) 2026 openRuyi Project Contributors
// SPDX-FileContributor: YunQiang Su <yunqiang@isrc.iscas.ac.cn>
// SPDX-License-Identifier: MulanPSL-2.0

//! Install LLVM IR files for products installed by Meson.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use clang_wrap::install::sync_installed_llvmir;
use clang_wrap::{debug_log, get_absolute_path, get_llvm_ir_dir, is_debug_mode};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <intro-installed.json> <destdir>", args[0]);
        exit(2);
    }

    let manifest_path = PathBuf::from(&args[1]);
    let manifest = match fs::read_to_string(&manifest_path) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("Failed to read {}: {error}", manifest_path.display());
            exit(1);
        }
    };
    let installed: BTreeMap<String, String> = match serde_json::from_str(&manifest) {
        Ok(installed) => installed,
        Err(error) => {
            eprintln!("Failed to parse {}: {error}", manifest_path.display());
            exit(1);
        }
    };

    let destdir = get_absolute_path(Path::new(&args[2]));
    let llvmir_dir = get_llvm_ir_dir();
    let debug = is_debug_mode();
    let mut failed = false;

    for (source, meson_destination) in installed {
        let source = PathBuf::from(source);
        let meson_destination = PathBuf::from(meson_destination);

        // Meson represents installed symlinks by a relative link name. Such an
        // entry has no separate build artifact or LLVM IR file to synchronize.
        if !source.is_absolute() {
            debug_log(
                debug,
                &format!(
                    "[DEBUG] Skipping Meson installed symlink entry: {} -> {}",
                    source.display(),
                    meson_destination.display()
                ),
            );
            continue;
        }
        if !meson_destination.is_absolute() {
            eprintln!(
                "Meson installation path must be absolute: {}",
                meson_destination.display()
            );
            failed = true;
            continue;
        }

        let destination = destdir.join(
            meson_destination
                .strip_prefix("/")
                .expect("absolute path has a root component"),
        );
        if let Err(error) = sync_installed_llvmir(&source, &destination, &llvmir_dir, None, debug) {
            eprintln!(
                "Failed to install LLVM IR for {}: {error}",
                source.display()
            );
            failed = true;
        }
    }

    if failed {
        exit(1);
    }
}
