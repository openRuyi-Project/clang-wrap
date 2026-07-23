# Distributed under the OSI-approved BSD 3-Clause License.  See accompanying
# file LICENSE.rst or https://cmake.org/licensing for details.
#
# CMakeEmitLLVMIR - Emit LLVM IR/Bitcode during build (Automatic Mode)
#
# This module provides functionality to preserve LLVM IR/bitcode during the
# build process. When the CMAKE_EMIT_LLVMIR environment variable is set, the
# build system automatically generates LLVM bitcode files for C and C++ source files
# and merges them into standalone .bc files matching the final output names.
#
# Usage:
#   Set the CMAKE_EMIT_LLVMIR environment variable before configuring:
#     export CMAKE_EMIT_LLVMIR=1
#     cmake -B build -S source -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES=/path/to/CMakeEmitLLVMIR.cmake
#
#   Or include it in your CMakeLists.txt:
#     include(CMakeEmitLLVMIR)
#
#   No other changes to CMakeLists.txt are required. The module automatically
#   processes all targets created with add_executable() and add_library().
#

if(DEFINED CMAKE_EMIT_LLVMIR_INCLUDED)
  return()
endif()
set(CMAKE_EMIT_LLVMIR_INCLUDED TRUE)
set(_CMAKE_EMIT_LLVMIR_MODULE_DIR "${CMAKE_CURRENT_LIST_DIR}")

# Check if LLVM IR emission is enabled via environment variable
if(NOT DEFINED ENV{CMAKE_EMIT_LLVMIR})
  return()
endif()

# Check if the value is a "falsy" value (0, OFF, FALSE, empty string)
set(_emit_llvmir_value "$ENV{CMAKE_EMIT_LLVMIR}")
if(_emit_llvmir_value MATCHES "^(0|OFF|FALSE)$" OR _emit_llvmir_value STREQUAL "")
  return()
endif()

set(CMAKE_EMIT_LLVMIR_ENABLED TRUE)

# The module emits ELF relink scripts and deliberately supports Linux only.
if(NOT CMAKE_SYSTEM_NAME STREQUAL "Linux")
  message(FATAL_ERROR "CMAKE_EMIT_LLVMIR supports Linux hosts only; detected '${CMAKE_SYSTEM_NAME}'.")
endif()

# The relink command is recovered from generator-native build metadata.  Keep
# the supported surface explicit instead of carrying partial IDE-generator
# behavior that cannot produce a portable, executable _cmd script.
if(NOT CMAKE_GENERATOR STREQUAL "Ninja" AND NOT CMAKE_GENERATOR STREQUAL "Unix Makefiles")
  message(FATAL_ERROR "CMAKE_EMIT_LLVMIR supports only the Ninja and Unix Makefiles generators; "
                      "CMake generator '${CMAKE_GENERATOR}' is not supported.")
endif()

# ============================================================================
# Track which targets have install rules
# ============================================================================

# Global property to store targets that have install rules
define_property(GLOBAL PROPERTY CMAKE_EMIT_LLVMIR_INSTALLED_TARGETS
  BRIEF_DOCS "Targets that have install() rules"
  FULL_DOCS "List of targets that have install() rules defined"
)

# Wrapper function to track install() calls for targets
# This allows us to only install LLVM IR for targets that have install rules
function(install)
  list(FIND ARGN "TARGETS" _targets_index)
  if(NOT _targets_index EQUAL -1)
    set(_runtime_destination "")
    set(_expect_runtime_destination FALSE)
    foreach(_argument IN LISTS ARGN)
      if(_expect_runtime_destination)
        if(_argument STREQUAL "DESTINATION")
          set(_expect_runtime_destination TRUE)
        else()
          set(_runtime_destination "${_argument}")
          set(_expect_runtime_destination FALSE)
        endif()
      elseif(_argument STREQUAL "RUNTIME")
        set(_expect_runtime_destination TRUE)
      endif()
    endforeach()
    # Target names are unambiguous CMake objects. Scanning for them avoids a
    # fragile mirror of install(TARGETS)' growing keyword grammar.
    foreach(_target IN LISTS ARGN)
      if(TARGET "${_target}")
        get_target_property(_target_type "${_target}" TYPE)
        if(_target_type MATCHES "^(EXECUTABLE|STATIC_LIBRARY|SHARED_LIBRARY|MODULE_LIBRARY)$")
        get_property(_installed_targets GLOBAL PROPERTY CMAKE_EMIT_LLVMIR_INSTALLED_TARGETS)
        list(APPEND _installed_targets "${_target}")
        list(REMOVE_DUPLICATES _installed_targets)
        set_property(GLOBAL PROPERTY CMAKE_EMIT_LLVMIR_INSTALLED_TARGETS "${_installed_targets}")
        if(_target_type STREQUAL "EXECUTABLE" AND _runtime_destination)
          set_property(TARGET "${_target}" PROPERTY
            EMIT_LLVMIR_RUNTIME_INSTALL_DESTINATION "${_runtime_destination}")
        endif()
        endif()
      endif()
    endforeach()
  endif()
  
  # Call original install command with all arguments
  _install(${ARGN})
endfunction()

# ============================================================================
# Configuration
# ============================================================================

if(NOT DEFINED CMAKE_EMIT_LLVMIR_OUTPUT_DIR)
  set(CMAKE_EMIT_LLVMIR_OUTPUT_DIR "${CMAKE_BINARY_DIR}/llvmir")
endif()

# The compilation database is the source of truth for compiler invocations.
# This intentionally overrides a project-level OFF value while emission is
# enabled; the module cannot reproduce real per-source options without it.
set(CMAKE_EXPORT_COMPILE_COMMANDS ON CACHE BOOL
    "Export compile commands for CMakeEmitLLVMIR" FORCE)
set(_emit_llvmir_extra_flags_file
    "${CMAKE_BINARY_DIR}${CMAKE_FILES_DIRECTORY}/cmake_emit_llvmir_extra_flags.txt")
set(_emit_llvmir_extra_flags "")
if(_emit_llvmir_value AND NOT _emit_llvmir_value MATCHES "^(1|0|ON|OFF|TRUE|FALSE)$" AND
   NOT _emit_llvmir_value MATCHES "[/]")
  set(_emit_llvmir_extra_flags "${_emit_llvmir_value}")
endif()
file(WRITE "${_emit_llvmir_extra_flags_file}" "${_emit_llvmir_extra_flags}\n")

# GNUInstallDirs must be loaded after project() enables a language.  The
# install directory is initialized in the deferred target-processing phase.

# ============================================================================
# Find LLVM Tools
# ============================================================================

# Derive LLVM tool paths from compiler path
# Use C compiler if available, otherwise use C++ compiler
if(DEFINED CMAKE_C_COMPILER AND CMAKE_C_COMPILER)
  set(_emit_llvmir_compiler "${CMAKE_C_COMPILER}")
elseif(DEFINED CMAKE_CXX_COMPILER AND CMAKE_CXX_COMPILER)
  set(_emit_llvmir_compiler "${CMAKE_CXX_COMPILER}")
else()
  message(FATAL_ERROR "CMAKE_EMIT_LLVMIR is set, but no C or C++ compiler is enabled. "
                      "Please enable at least one of C or C++ language.")
endif()

# Test if compiler supports -emit-llvm
set(_emit_llvmir_test_file "${CMAKE_BINARY_DIR}${CMAKE_FILES_DIRECTORY}/CMakeTmp/test_emit_llvmir.c")
set(_emit_llvmir_test_output_file "${CMAKE_BINARY_DIR}${CMAKE_FILES_DIRECTORY}/CMakeTmp/test_emit_llvmir.bc")
file(WRITE "${_emit_llvmir_test_file}" "int main() { return 0; }\n")

execute_process(
  COMMAND "${_emit_llvmir_compiler}" -emit-llvm -c "${_emit_llvmir_test_file}" -o "${_emit_llvmir_test_output_file}"
  OUTPUT_VARIABLE _emit_llvmir_test_output
  ERROR_VARIABLE _emit_llvmir_test_error
  RESULT_VARIABLE _emit_llvmir_test_result
)

file(REMOVE "${_emit_llvmir_test_file}" "${_emit_llvmir_test_output_file}")

if(NOT _emit_llvmir_test_result EQUAL 0)
  message(FATAL_ERROR "CMAKE_EMIT_LLVMIR is set, but the compiler does not support -emit-llvm. "
                      "Compiler: ${_emit_llvmir_compiler} "
                      "Error: ${_emit_llvmir_test_error} "
                      "Please use a Clang-based compiler or disable CMAKE_EMIT_LLVMIR.")
endif()

# E.g., /usr/bin/clang-21 -> /usr/bin/llvm-link-21
#       clang-15 -> llvm-link-15
#       clang -> llvm-link
#       cc (symlink to clang) -> llvm-link
get_filename_component(_clang_dir "${_emit_llvmir_compiler}" DIRECTORY)

# Resolve symlinks to get actual compiler name (e.g., cc -> clang-21)
get_filename_component(_clang_realpath "${_emit_llvmir_compiler}" REALPATH)
get_filename_component(_clang_name "${_clang_realpath}" NAME)

# Extract version suffix from clang name (e.g., clang-21 -> -21, clang-15 -> -15)
if(_clang_name MATCHES "^clang(\\+\\+)?(-[0-9]+(\\.[0-9]+)*)?$")
  set(_clang_suffix "${CMAKE_MATCH_2}")
else()
  set(_clang_suffix "")
endif()

# Get the actual LLVM bin directory from the resolved clang path
# This handles cases where /usr/bin/clang-22 is a symlink to /usr/lib/llvm-22/bin/clang
get_filename_component(_llvm_bin_dir "${_clang_realpath}" DIRECTORY)

# Build LLVM tool names with same suffix
# Try with suffix first, then without suffix (for brew installations where
# clang has version but llvm tools don't)
set(_llvm_tools llvm-link llvm-ar)
foreach(_tool IN LISTS _llvm_tools)
  # Search in LLVM bin directory: first with suffix, then without
  find_program(LLVM_${_tool}_EXECUTABLE
    NAMES ${_tool}${_clang_suffix} ${_tool}
    HINTS "${_llvm_bin_dir}"
    NO_DEFAULT_PATH
  )
  mark_as_advanced(LLVM_${_tool}_EXECUTABLE)

  # If not found in LLVM bin directory, search in PATH
  # This handles cases where llvm-link-22 is in /usr/bin
  if(NOT LLVM_${_tool}_EXECUTABLE)
    find_program(LLVM_${_tool}_EXECUTABLE
      NAMES ${_tool}${_clang_suffix}
    )
    mark_as_advanced(LLVM_${_tool}_EXECUTABLE)
  endif()
endforeach()

# llvm-link is required
if(NOT LLVM_llvm-link_EXECUTABLE)
  if(_clang_suffix)
    # Tried to find llvm-link with specific version
    message(FATAL_ERROR "CMAKE_EMIT_LLVMIR is set, but llvm-link matching the compiler version was not found. "
                        "Compiler: ${CMAKE_C_COMPILER} (version ${_clang_suffix}) "
                        "Required: llvm-link${_clang_suffix} (not found in ${_clang_dir}). "
                        "Please install LLVM ${_clang_suffix} tools or use a compiler that matches your installed LLVM version.")
  else()
    # Tried to find llvm-link without version suffix
    message(FATAL_ERROR "CMAKE_EMIT_LLVMIR is set, but llvm-link matching the compiler version was not found. "
                        "Compiler: ${CMAKE_C_COMPILER} "
                        "Required: llvm-link (not found in ${_clang_dir}). "
                        "Please ensure LLVM tools are installed and in your PATH.")
  endif()
endif()
if(NOT LLVM_llvm-ar_EXECUTABLE)
  message(FATAL_ERROR "CMAKE_EMIT_LLVMIR is set, but llvm-ar matching the compiler version was not found.")
endif()

# If fallback was used (no suffix), verify version matches clang major version
if(_clang_suffix AND NOT LLVM_llvm-link_EXECUTABLE MATCHES "${_clang_suffix}$")
  # Extract version from llvm-link
  execute_process(
    COMMAND "${LLVM_llvm-link_EXECUTABLE}" --version
    OUTPUT_VARIABLE _llvm_link_output
    ERROR_VARIABLE _llvm_link_error
    OUTPUT_STRIP_TRAILING_WHITESPACE
    ERROR_STRIP_TRAILING_WHITESPACE
  )
  
  if(_llvm_link_output MATCHES "version ([0-9]+)")
    set(_llvm_link_major "${CMAKE_MATCH_1}")
    string(SUBSTRING "${_clang_suffix}" 1 -1 _clang_major)
    
    if(NOT _llvm_link_major STREQUAL _clang_major)
      message(FATAL_ERROR "LLVM tool version mismatch: clang uses version ${_clang_major} "
                          "but llvm-link uses version ${_llvm_link_major}. "
                          "Please ensure clang and llvm-link versions match.")
    endif()
  endif()
endif()

# Get LLVM version from llvm-link
execute_process(
  COMMAND "${LLVM_llvm-link_EXECUTABLE}" --version
  OUTPUT_VARIABLE _llvm_link_output
  ERROR_VARIABLE _llvm_link_error
  OUTPUT_STRIP_TRAILING_WHITESPACE
  ERROR_STRIP_TRAILING_WHITESPACE
)

execute_process(
  COMMAND "${_emit_llvmir_compiler}" --version
  OUTPUT_VARIABLE _emit_llvmir_compiler_version
  OUTPUT_STRIP_TRAILING_WHITESPACE
)
string(REGEX MATCH "^[^\n\r]+" CMAKE_EMIT_LLVMIR_CLANG_VERSION_LINE "${_emit_llvmir_compiler_version}")

if(_llvm_link_output MATCHES "version ([0-9]+\\.[0-9]+)")
  set(CMAKE_EMIT_LLVMIR_LLVM_VERSION "${CMAKE_MATCH_1}")
elseif(_llvm_link_error MATCHES "version ([0-9]+\\.[0-9]+)")
  set(CMAKE_EMIT_LLVMIR_LLVM_VERSION "${CMAKE_MATCH_1}")
else()
  set(CMAKE_EMIT_LLVMIR_LLVM_VERSION "unknown")
endif()

message(STATUS "LLVM IR/Bitcode emission enabled (LLVM ${CMAKE_EMIT_LLVMIR_LLVM_VERSION})")
message(STATUS "  Output directory: ${CMAKE_EMIT_LLVMIR_OUTPUT_DIR}")
message(STATUS "  Install directory: resolved after project() enables languages")

# ============================================================================
# Helper Functions
# ============================================================================

# Verify every compiler that will emit bitcode.  Top-level include hooks run
# during project(), so this function is called while deferred target processing
# after all requested languages have been enabled.
function(_cmake_emit_llvmir_verify_language language)
  get_property(_verified GLOBAL PROPERTY "CMAKE_EMIT_LLVMIR_${language}_VERIFIED")
  if(_verified)
    return()
  endif()

  set(_compiler "${CMAKE_${language}_COMPILER}")
  if(NOT _compiler)
    message(FATAL_ERROR "CMakeEmitLLVMIR needs a ${language} compiler to emit bitcode, but none is enabled.")
  endif()

  if(language STREQUAL "CXX")
    set(_extension "cpp")
  else()
    set(_extension "c")
  endif()
  set(_test_file "${CMAKE_BINARY_DIR}${CMAKE_FILES_DIRECTORY}/CMakeTmp/test_emit_llvmir_${language}.${_extension}")
  set(_test_output "${_test_file}.bc")
  file(WRITE "${_test_file}" "int main() { return 0; }\n")
  execute_process(
    COMMAND "${_compiler}" -emit-llvm -c "${_test_file}" -o "${_test_output}"
    ERROR_VARIABLE _test_error
    RESULT_VARIABLE _test_result
  )
  file(REMOVE "${_test_file}" "${_test_output}")
  if(NOT _test_result EQUAL 0)
    message(FATAL_ERROR "CMAKE_EMIT_LLVMIR is enabled, but the ${language} compiler does not support -emit-llvm. "
                        "Compiler: ${_compiler} Error: ${_test_error}")
  endif()
  set_property(GLOBAL PROPERTY "CMAKE_EMIT_LLVMIR_${language}_VERIFIED" TRUE)
endfunction()

function(_cmake_emit_llvmir_get_verscript_name output_name result_var)
  set(${result_var} "${output_name}_verscript" PARENT_SCOPE)
endfunction()

function(_cmake_emit_llvmir_get_bc_path source_file target_name bc_path_var)
  get_filename_component(_source_name "${source_file}" NAME_WE)
  get_filename_component(_source_ext "${source_file}" EXT)

  if(_source_ext MATCHES "\\.(h|hpp|hxx|inl|inc|def)$")
    return()
  endif()

  get_filename_component(_source_dir "${source_file}" DIRECTORY)
  get_filename_component(_source_name "${source_file}" NAME_WE)

  # Calculate relative path from target's source directory
  get_target_property(_target_source_dir ${target_name} SOURCE_DIR)
  if(NOT _target_source_dir OR _target_source_dir STREQUAL "_target_source_dir-NOTFOUND")
    set(_target_source_dir "${CMAKE_SOURCE_DIR}")
  endif()

  if(IS_ABSOLUTE "${_source_dir}")
    if(_source_dir STREQUAL _target_source_dir OR _source_dir STREQUAL "")
      set(_source_dir "")
    else()
      file(RELATIVE_PATH _source_dir "${_target_source_dir}" "${_source_dir}")
    endif()
  endif()

  _cmake_emit_llvmir_get_target_output_dir(${target_name} _bc_base_dir)

  # A target can compile files with the same stem but a different extension or
  # source directory. Do not reproduce source paths here: external sources can
  # contain '..' and must never escape CMAKE_EMIT_LLVMIR_OUTPUT_DIR.
  string(SHA256 _source_hash "${source_file}")
  string(SUBSTRING "${_source_hash}" 0 12 _source_hash)
  set(_bc_output "${_bc_base_dir}/objects/${target_name}_${_source_name}_${_source_hash}.bc")

  get_filename_component(_bc_dir "${_bc_output}" DIRECTORY)
  file(MAKE_DIRECTORY "${_bc_dir}")

  set(${bc_path_var} "${_bc_output}" PARENT_SCOPE)
endfunction()

# Keep emitted artifacts below the configured output root. Targets in
# subdirectories retain their binary-directory layout, preventing collisions.
function(_cmake_emit_llvmir_get_target_output_dir target result_var)
  get_target_property(_target_binary_dir ${target} BINARY_DIR)
  if(NOT _target_binary_dir OR _target_binary_dir STREQUAL "_target_binary_dir-NOTFOUND")
    set(_target_binary_dir "${CMAKE_BINARY_DIR}")
  endif()
  file(RELATIVE_PATH _relative_binary_dir "${CMAKE_BINARY_DIR}" "${_target_binary_dir}")
  if(_relative_binary_dir STREQUAL "." OR _relative_binary_dir STREQUAL "")
    set(_output_dir "${CMAKE_EMIT_LLVMIR_OUTPUT_DIR}")
  elseif(_relative_binary_dir MATCHES "^\\.\\.")
    string(SHA256 _dir_hash "${_target_binary_dir}")
    string(SUBSTRING "${_dir_hash}" 0 12 _dir_hash)
    set(_output_dir "${CMAKE_EMIT_LLVMIR_OUTPUT_DIR}/external-${_dir_hash}")
  else()
    set(_output_dir "${CMAKE_EMIT_LLVMIR_OUTPUT_DIR}/${_relative_binary_dir}")
  endif()
  set(${result_var} "${_output_dir}" PARENT_SCOPE)
endfunction()

# Resolve the filename CMake uses for the active single-config build.  Keep
# every LLVMIR artifact derived from this one value rather than reproducing
# naming rules at each consumer.
function(_cmake_emit_llvmir_get_target_output_names target ir_name_var native_name_var)
  get_target_property(_target_type ${target} TYPE)
  get_target_property(_target_name ${target} NAME)
  string(TOUPPER "${CMAKE_BUILD_TYPE}" _config)

  if(_target_type STREQUAL "EXECUTABLE")
    set(_output_kind "RUNTIME")
  elseif(_target_type STREQUAL "STATIC_LIBRARY")
    set(_output_kind "ARCHIVE")
  else()
    set(_output_kind "LIBRARY")
  endif()

  set(_output_name "")
  if(_config)
    get_target_property(_output_name ${target} "${_output_kind}_OUTPUT_NAME_${_config}")
    if(NOT _output_name OR _output_name STREQUAL "_output_name-NOTFOUND")
      get_target_property(_output_name ${target} "OUTPUT_NAME_${_config}")
    endif()
  endif()
  if(NOT _output_name OR _output_name STREQUAL "_output_name-NOTFOUND")
    get_target_property(_output_name ${target} "${_output_kind}_OUTPUT_NAME")
  endif()
  if(NOT _output_name OR _output_name STREQUAL "_output_name-NOTFOUND")
    get_target_property(_output_name ${target} OUTPUT_NAME)
  endif()
  if(NOT _output_name OR _output_name STREQUAL "_output_name-NOTFOUND")
    set(_output_name "${_target_name}")
  endif()

  if(_config)
    get_target_property(_postfix ${target} "${_config}_POSTFIX")
    if(_postfix AND NOT _postfix STREQUAL "_postfix-NOTFOUND")
      string(APPEND _output_name "${_postfix}")
    endif()
  endif()

  get_target_property(_prefix ${target} PREFIX)
  get_target_property(_suffix ${target} SUFFIX)
  get_property(_prefix_is_set TARGET ${target} PROPERTY PREFIX SET)
  get_property(_suffix_is_set TARGET ${target} PROPERTY SUFFIX SET)
  get_target_property(_version ${target} VERSION)
  get_target_property(_soversion ${target} SOVERSION)

  if(_prefix_is_set)
    set(_base_name "${_prefix}${_output_name}")
  elseif(_target_type STREQUAL "STATIC_LIBRARY")
    set(_base_name "${CMAKE_STATIC_LIBRARY_PREFIX}${_output_name}")
  elseif(_target_type STREQUAL "SHARED_LIBRARY")
    set(_base_name "${CMAKE_SHARED_LIBRARY_PREFIX}${_output_name}")
  elseif(_target_type STREQUAL "MODULE_LIBRARY")
    set(_base_name "${CMAKE_SHARED_MODULE_PREFIX}${_output_name}")
  else()
    set(_base_name "${_output_name}")
  endif()

  if(NOT _suffix_is_set)
    if(_target_type STREQUAL "STATIC_LIBRARY")
      set(_suffix "${CMAKE_STATIC_LIBRARY_SUFFIX}")
    elseif(_target_type STREQUAL "SHARED_LIBRARY")
      set(_suffix "${CMAKE_SHARED_LIBRARY_SUFFIX}")
    elseif(_target_type STREQUAL "MODULE_LIBRARY")
      set(_suffix "${CMAKE_SHARED_MODULE_SUFFIX}")
    else()
      set(_suffix "")
    endif()
  endif()

  set(_ir_name "${_base_name}${_suffix}")
  set(_native_name "${_ir_name}")
  if(_version AND _target_type MATCHES "^(SHARED_LIBRARY|MODULE_LIBRARY)$")
    # SOVERSION may legitimately be "0", which CMake treats as false in a
    # bare if() condition. Check only whether the property was found.
    if(NOT _soversion STREQUAL "_soversion-NOTFOUND")
      set(_ir_name "${_base_name}${_suffix}.${_soversion}")
    else()
      set(_ir_name "${_base_name}${_suffix}.${_version}")
    endif()
    set(_native_name "${_base_name}${_suffix}.${_version}")
  endif()
  set(${ir_name_var} "${_ir_name}" PARENT_SCOPE)
  set(${native_name_var} "${_native_name}" PARENT_SCOPE)
endfunction()

# Static dependency bitcode uses llvm-link --only-needed, except targets named
# in a native --whole-archive scope, whose members must always be retained.
function(_cmake_emit_llvmir_normalize_link_target item result_var)
  set(_item "${item}")
  if(_item MATCHES "^\\$<LINK_ONLY:(.+)>$" OR _item MATCHES "^\\$<BUILD_INTERFACE:(.+)>$")
    set(_item "${CMAKE_MATCH_1}")
  endif()
  if(TARGET "${_item}")
    get_target_property(_aliased_target "${_item}" ALIASED_TARGET)
    if(_aliased_target)
      set(_item "${_aliased_target}")
    endif()
    set(${result_var} "${_item}" PARENT_SCOPE)
  else()
    set(${result_var} "" PARENT_SCOPE)
  endif()
endfunction()

function(_cmake_emit_llvmir_collect_static_bitcode target result_var visited_targets)
  set(_outputs "")
  list(FIND visited_targets "${target}" _cycle_index)
  if(NOT _cycle_index EQUAL -1)
    set(${result_var} "" PARENT_SCOPE)
    return()
  endif()
  list(APPEND visited_targets "${target}")
  get_target_property(_libraries ${target} LINK_LIBRARIES)
  get_target_property(_interface_libraries ${target} INTERFACE_LINK_LIBRARIES)
  list(APPEND _libraries ${_interface_libraries})
  foreach(_library IN LISTS _libraries)
    _cmake_emit_llvmir_normalize_link_target("${_library}" _library)
    if(_library)
      get_target_property(_library_type ${_library} TYPE)
      get_target_property(_library_imported ${_library} IMPORTED)
      if(_library_imported)
        continue()
      endif()
      if(_library_type STREQUAL "STATIC_LIBRARY")
        _cmake_emit_llvmir_collect_static_bitcode(${_library} _transitive_outputs "${visited_targets}")
        list(APPEND _outputs ${_transitive_outputs})
        get_target_property(_library_output ${_library} EMIT_LLVMIR_OUTPUT)
        if(NOT _library_output OR _library_output STREQUAL "_library_output-NOTFOUND")
          # Static-library cycles and forward declarations can be observed
          # before the target's processing pass registers its output.
          _cmake_emit_llvmir_get_target_output_dir(${_library} _library_output_dir)
          _cmake_emit_llvmir_get_target_output_names(${_library} _library_output_name _library_native_name)
          set(_library_output "${_library_output_dir}/${_library_output_name}")
        endif()
        list(APPEND _outputs "${_library_output}")
      elseif(_library_type STREQUAL "INTERFACE_LIBRARY")
        _cmake_emit_llvmir_collect_static_bitcode(${_library} _transitive_outputs "${visited_targets}")
        list(APPEND _outputs ${_transitive_outputs})
      endif()
    endif()
  endforeach()
  list(REMOVE_DUPLICATES _outputs)
  set(${result_var} "${_outputs}" PARENT_SCOPE)
endfunction()

# CMake preserves target_link_libraries() order in LINK_LIBRARIES, including
# raw linker flags. Track GNU ld's whole-archive delimiters so LLVM IR keeps
# exactly the static-library members that the native linker is forced to load.
function(_cmake_emit_llvmir_collect_whole_archive_bitcode target result_var)
  set(_outputs "")
  set(_whole_archive FALSE)
  get_target_property(_libraries ${target} LINK_LIBRARIES)
  foreach(_library IN LISTS _libraries)
    if(_library MATCHES "(^|,)--whole-archive(,|$)")
      set(_whole_archive TRUE)
      continue()
    elseif(_library MATCHES "(^|,)--no-whole-archive(,|$)")
      set(_whole_archive FALSE)
      continue()
    endif()
    if(NOT _whole_archive)
      continue()
    endif()
    _cmake_emit_llvmir_normalize_link_target("${_library}" _library_target)
    if(_library_target)
      get_target_property(_library_type ${_library_target} TYPE)
      get_target_property(_library_imported ${_library_target} IMPORTED)
      if(_library_type STREQUAL "STATIC_LIBRARY" AND NOT _library_imported)
        get_target_property(_library_bc_files ${_library_target} EMIT_LLVMIR_BC_FILES)
        if(NOT _library_bc_files OR _library_bc_files STREQUAL "_library_bc_files-NOTFOUND")
          message(FATAL_ERROR "Whole-archive static dependency ${_library_target} has no registered LLVM bitcode members.")
        endif()
        list(APPEND _outputs ${_library_bc_files})
      endif()
    endif()
  endforeach()
  list(REMOVE_DUPLICATES _outputs)
  set(${result_var} "${_outputs}" PARENT_SCOPE)
endfunction()

function(_cmake_emit_llvmir_collect_static_native_member_files target result_var visited_targets)
  set(_outputs "")
  list(FIND visited_targets "${target}" _cycle_index)
  if(_cycle_index EQUAL -1)
    list(APPEND visited_targets "${target}")
    get_target_property(_libraries ${target} LINK_LIBRARIES)
    get_target_property(_interface_libraries ${target} INTERFACE_LINK_LIBRARIES)
    list(APPEND _libraries ${_interface_libraries})
    foreach(_library IN LISTS _libraries)
      _cmake_emit_llvmir_normalize_link_target("${_library}" _library)
      if(_library)
        get_target_property(_library_type ${_library} TYPE)
        get_target_property(_library_imported ${_library} IMPORTED)
        if(_library_imported)
          continue()
        elseif(_library_type STREQUAL "STATIC_LIBRARY")
          _cmake_emit_llvmir_collect_static_native_member_files(${_library} _transitive_outputs "${visited_targets}")
          list(APPEND _outputs ${_transitive_outputs})
          get_target_property(_library_output ${_library} EMIT_LLVMIR_OUTPUT)
          if(NOT _library_output OR _library_output STREQUAL "_library_output-NOTFOUND")
            # Static-library cycles and forward declarations can be observed
            # before the target's processing pass registers its output.
            _cmake_emit_llvmir_get_target_output_dir(${_library} _library_output_dir)
            _cmake_emit_llvmir_get_target_output_names(${_library} _library_output_name _library_native_name)
            set(_library_output "${_library_output_dir}/${_library_output_name}")
          endif()
          list(APPEND _outputs "${_library_output}_missing")
        elseif(_library_type STREQUAL "INTERFACE_LIBRARY")
          _cmake_emit_llvmir_collect_static_native_member_files(${_library} _transitive_outputs "${visited_targets}")
          list(APPEND _outputs ${_transitive_outputs})
        endif()
      endif()
    endforeach()
  endif()
  list(REMOVE_DUPLICATES _outputs)
  set(${result_var} "${_outputs}" PARENT_SCOPE)
endfunction()

# Static-library LLVM archives are shared outputs. Consumers must depend on
# their producer targets instead of registering another custom command.
function(_cmake_emit_llvmir_collect_static_targets target result_var visited_targets)
  set(_outputs "")
  list(FIND visited_targets "${target}" _cycle_index)
  if(NOT _cycle_index EQUAL -1)
    set(${result_var} "" PARENT_SCOPE)
    return()
  endif()
  list(APPEND visited_targets "${target}")
  get_target_property(_libraries ${target} LINK_LIBRARIES)
  get_target_property(_interface_libraries ${target} INTERFACE_LINK_LIBRARIES)
  list(APPEND _libraries ${_interface_libraries})
  foreach(_library IN LISTS _libraries)
    _cmake_emit_llvmir_normalize_link_target("${_library}" _library)
    if(_library)
      get_target_property(_library_type ${_library} TYPE)
      get_target_property(_library_imported ${_library} IMPORTED)
      if(_library_imported)
        continue()
      endif()
      if(_library_type STREQUAL "STATIC_LIBRARY")
        list(APPEND _outputs "${_library}_llvmir")
        _cmake_emit_llvmir_collect_static_targets(${_library} _transitive_outputs "${visited_targets}")
        list(APPEND _outputs ${_transitive_outputs})
      elseif(_library_type STREQUAL "INTERFACE_LIBRARY")
        _cmake_emit_llvmir_collect_static_targets(${_library} _transitive_outputs "${visited_targets}")
        list(APPEND _outputs ${_transitive_outputs})
      endif()
    endif()
  endforeach()
  list(REMOVE_DUPLICATES _outputs)
  set(${result_var} "${_outputs}" PARENT_SCOPE)
endfunction()

function(_cmake_emit_llvmir_target_has_assembly_sources target result_var)
  set(_visited_targets ${ARGN})
  list(FIND _visited_targets "${target}" _cycle_index)
  if(NOT _cycle_index EQUAL -1)
    set(${result_var} FALSE PARENT_SCOPE)
    return()
  endif()
  list(APPEND _visited_targets "${target}")

  get_target_property(_sources ${target} SOURCES)
  foreach(_source IN LISTS _sources)
    if(_source MATCHES "\\.(S|s|ASM|asm)$")
      set(${result_var} TRUE PARENT_SCOPE)
      return()
    endif()
    if(_source MATCHES "^\\$<TARGET_OBJECTS:(.+)>$")
      set(_object_target "${CMAKE_MATCH_1}")
      if(TARGET "${_object_target}")
        _cmake_emit_llvmir_target_has_assembly_sources(
          "${_object_target}" _object_has_assembly ${_visited_targets})
        if(_object_has_assembly)
          set(${result_var} TRUE PARENT_SCOPE)
          return()
        endif()
      endif()
    endif()
  endforeach()

  get_target_property(_link_libraries ${target} LINK_LIBRARIES)
  foreach(_library IN LISTS _link_libraries)
    _cmake_emit_llvmir_normalize_link_target("${_library}" _library)
    if(_library)
      get_target_property(_library_type ${_library} TYPE)
      if(_library_type STREQUAL "OBJECT_LIBRARY")
        _cmake_emit_llvmir_target_has_assembly_sources(
          "${_library}" _object_has_assembly ${_visited_targets})
        if(_object_has_assembly)
          set(${result_var} TRUE PARENT_SCOPE)
          return()
        endif()
      endif()
    endif()
  endforeach()
  set(${result_var} FALSE PARENT_SCOPE)
endfunction()

# Object libraries can be declared in a different source directory from their
# consumers. Resolve their relative sources before adding them to a consumer.
function(_cmake_emit_llvmir_collect_object_sources object_target result_var)
  get_target_property(_object_sources ${object_target} SOURCES)
  get_target_property(_object_source_dir ${object_target} SOURCE_DIR)
  get_target_property(_object_binary_dir ${object_target} BINARY_DIR)
  set(_resolved_sources "")
  foreach(_object_source IN LISTS _object_sources)
    if(IS_ABSOLUTE "${_object_source}" OR _object_source MATCHES "^\$<")
      list(APPEND _resolved_sources "${_object_source}")
    elseif(EXISTS "${_object_source_dir}/${_object_source}")
      list(APPEND _resolved_sources "${_object_source_dir}/${_object_source}")
    else()
      list(APPEND _resolved_sources "${_object_binary_dir}/${_object_source}")
    endif()
  endforeach()
  set(${result_var} "${_resolved_sources}" PARENT_SCOPE)
endfunction()

function(_cmake_emit_llvmir_collect_project_shared_outputs target result_var visited_targets)
  set(_outputs "")
  list(FIND visited_targets "${target}" _cycle_index)
  if(NOT _cycle_index EQUAL -1)
    set(${result_var} "" PARENT_SCOPE)
    return()
  endif()
  list(APPEND visited_targets "${target}")
  get_target_property(_libraries ${target} LINK_LIBRARIES)
  get_target_property(_interface_libraries ${target} INTERFACE_LINK_LIBRARIES)
  list(APPEND _libraries ${_interface_libraries})
  foreach(_library IN LISTS _libraries)
    _cmake_emit_llvmir_normalize_link_target("${_library}" _library)
    if(_library)
      get_target_property(_library_type ${_library} TYPE)
      if(_library_type MATCHES "^(SHARED_LIBRARY|MODULE_LIBRARY)$")
        _cmake_emit_llvmir_get_target_output_names(${_library} _library_ir_name _library_native_name)
        list(APPEND _outputs "${_library_native_name}")
        # Native executable link commands refer to a versioned shared library
        # by SOVERSION, whereas installed LLVMIR scripts live in llvmir-bin.
        # Retain both names so that the command rewriter can map either form
        # to ../<library> in the adjacent llvmir directory.
        list(APPEND _outputs "${_library_ir_name}")
        # A shared library can expose another project shared library through
        # its own link interface. Include it so relink-script rewriting does
        # not retain a build-tree-relative path from the native command.
        _cmake_emit_llvmir_collect_project_shared_outputs(${_library} _transitive_outputs "${visited_targets}")
        list(APPEND _outputs ${_transitive_outputs})
      elseif(_library_type STREQUAL "STATIC_LIBRARY" OR _library_type STREQUAL "INTERFACE_LIBRARY")
        # Static libraries can carry a transitive shared-library dependency.
        # Their C/C++ content is merged into the consumer bitcode, but those
        # native shared libraries remain link inputs for an installed _cmd.
        _cmake_emit_llvmir_collect_project_shared_outputs(${_library} _transitive_outputs "${visited_targets}")
        list(APPEND _outputs ${_transitive_outputs})
      endif()
    endif()
  endforeach()
  list(REMOVE_DUPLICATES _outputs)
  set(${result_var} "${_outputs}" PARENT_SCOPE)
endfunction()

# ============================================================================
# Main Target Processing Function
# ============================================================================

function(cmake_emit_llvmir_process_target target)
  if(NOT CMAKE_EMIT_LLVMIR_ENABLED)
    return()
  endif()

  if(NOT TARGET ${target})
    return()
  endif()

  get_target_property(_target_type ${target} TYPE)
  get_target_property(_target_name ${target} NAME)
  get_target_property(_target_sources ${target} SOURCES)

  if(_target_type STREQUAL "INTERFACE_LIBRARY" OR _target_type STREQUAL "OBJECT_LIBRARY")
    return()
  endif()

  if(NOT _target_sources)
    return()
  endif()

  # Expand TARGET_OBJECTS generator expressions in SOURCES
  set(_expanded_sources "")
  set(_object_source_compile_targets "")
  foreach(_source IN LISTS _target_sources)
    if(_source MATCHES "^\\$<TARGET_OBJECTS:(.+)>$")
      set(_obj_lib "${CMAKE_MATCH_1}")
      if(TARGET "${_obj_lib}")
        _cmake_emit_llvmir_collect_object_sources(${_obj_lib} _obj_lib_sources)
        if(_obj_lib_sources)
          list(APPEND _expanded_sources ${_obj_lib_sources})
          foreach(_obj_lib_source IN LISTS _obj_lib_sources)
            list(APPEND _object_source_compile_targets "${_obj_lib_source}|${_obj_lib}")
          endforeach()
        endif()
      endif()
    else()
      list(APPEND _expanded_sources "${_source}")
    endif()
  endforeach()
  set(_target_sources "${_expanded_sources}")

  # Object-library sources are linked into every consuming target, not just
  # static libraries. Include them in merged bitcode before native objects are
  # removed from the generated relink command.
  get_target_property(_target_link_libs ${target} LINK_LIBRARIES)
  if(_target_link_libs)
      foreach(_lib IN LISTS _target_link_libs)
        if(TARGET "${_lib}")
          get_target_property(_lib_type ${_lib} TYPE)
          if(_lib_type STREQUAL "OBJECT_LIBRARY")
            _cmake_emit_llvmir_collect_object_sources(${_lib} _obj_lib_sources)
            if(_obj_lib_sources)
              list(APPEND _target_sources ${_obj_lib_sources})
              foreach(_obj_lib_source IN LISTS _obj_lib_sources)
                list(APPEND _object_source_compile_targets "${_obj_lib_source}|${_lib}")
              endforeach()
            endif()
          endif()
        endif()
      endforeach()
  endif()


  # Compiler support already checked at module load
  # No need to check again for each target/file

  # Resolve all names once for the active single-config build.
  _cmake_emit_llvmir_get_target_output_names(${target} _full_output_name _native_output_name)
  get_target_property(_version ${target} VERSION)

  set(_bc_files "")

  # Get the source directory for this target
  get_target_property(_target_source_dir ${target} SOURCE_DIR)
  if(NOT _target_source_dir OR _target_source_dir STREQUAL "_target_source_dir-NOTFOUND")
    set(_target_source_dir "${CMAKE_SOURCE_DIR}")
  endif()
  
  # Get the binary directory for this target (needed for generated files)
  get_target_property(_target_binary_dir ${target} BINARY_DIR)
  if(NOT _target_binary_dir OR _target_binary_dir STREQUAL "_target_binary_dir-NOTFOUND")
    set(_target_binary_dir "${CMAKE_BINARY_DIR}")
  endif()

  # Remove duplicate sources to avoid conflicts
  list(REMOVE_DUPLICATES _target_sources)

  if(CMAKE_EMIT_LLVMIR_VERBOSE)
    list(LENGTH _target_sources _source_count)
    message(STATUS "CMakeEmitLLVMIR: target ${target} has ${_source_count} source(s): ${_target_sources}")
  endif()

  foreach(_source IN LISTS _target_sources)
    # Skip other generator expressions (e.g., $<$<BOOL:OFF>:file.cpp>)
    if(_source MATCHES "^\\$<")
      continue()
    endif()

    # Skip if source contains unmatched '>' (part of a generator expression)
    if(_source MATCHES ">$" OR _source MATCHES "^<")
      continue()
    endif()

    if(TARGET "${_source}")
      continue()
    endif()

    # Do not query SOURCE properties for non-compilation inputs. In CMake,
    # querying a relative source through TARGET_DIRECTORY can materialize it
    # as a source-tree entry. This breaks custom targets whose SOURCES are
    # configure_file() outputs in their binary directory.
    get_filename_component(_source_declared_ext "${_source}" EXT)
    if(NOT _source_declared_ext MATCHES "\\.(c|C|cc|cpp|cxx|c\\+\\+|CC|CPP|CXX)$")
      continue()
    endif()

    if(IS_ABSOLUTE "${_source}")
      set(_source_path "${_source}")
    else()
      # Generated target sources can have a pre-generated source-tree fallback
      # with the same name.  CMake compiles the binary-tree output, so retain
      # that identity for compile_commands.json matching and build ordering.
      # Source properties are scoped to the directory that creates the
      # target, not to this module's top-level inclusion directory.
      get_property(_source_is_generated SOURCE "${_source}"
        TARGET_DIRECTORY "${target}" PROPERTY GENERATED)
      set(_source_dir_path "${_target_source_dir}/${_source}")
      set(_binary_dir_path "${_target_binary_dir}/${_source}")
      set(_top_level_binary_dir_path "${CMAKE_BINARY_DIR}/${_source}")
      
      if(_source_is_generated)
        set(_source_path "${_binary_dir_path}")
      # If file exists in source dir, use it
      elseif(EXISTS "${_source_dir_path}")
        set(_source_path "${_source_dir_path}")
      elseif(EXISTS "${_binary_dir_path}")
        # A target can generate sources in its own binary directory.
        set(_source_path "${_binary_dir_path}")
      elseif(EXISTS "${_top_level_binary_dir_path}")
        # Object libraries declared at the top level can contribute generated
        # sources to targets in subdirectories (for example libjpeg-turbo's
        # SIMD fallback). Resolve those paths from the top-level build tree.
        set(_source_path "${_top_level_binary_dir_path}")
      else()
        # CMake permits a top-level object library to declare an output that
        # does not exist until the build starts. Its relative source path is
        # rooted in CMAKE_BINARY_DIR, even when a subdirectory target consumes
        # its objects. This also gives Make the same prerequisite path as the
        # producer's custom command.
        set(_source_path "${_top_level_binary_dir_path}")
      endif()
    endif()

    # Skip non-source files (scripts, etc.) by checking extension
    get_filename_component(_source_ext "${_source_path}" EXT)
    if(NOT _source_ext MATCHES "\\.(c|C|cc|cpp|cxx|c\\+\\+|CC|CPP|CXX)$")
      continue()
    endif()

    # Get language from source file properties, with fallback to extension
    set(_lang "")
    get_source_file_property(_lang "${_source}" LANGUAGE)
    if(NOT _lang OR _lang STREQUAL "_lang-NOTFOUND")
      # Infer language from file extension
      if(_source MATCHES "\\.(c|C)$")
        set(_lang "C")
      elseif(_source MATCHES "\\.(cc|cpp|cxx|c\\+\\+|CC|CPP|CXX)$")
        set(_lang "CXX")
      else()
        set(_lang "C")
      endif()
    endif()

    if(NOT _lang MATCHES "^(C|CXX)$")
      continue()
    endif()

    _cmake_emit_llvmir_verify_language("${_lang}")

    # Skip if this source file has already been processed for THIS target
    # Use target:source_path as key to allow same source in different targets
    set(_processed_key "${target}:${_source_path}")
    get_property(_emit_llvmir_processed_sources GLOBAL PROPERTY CMAKE_EMIT_LLVMIR_PROCESSED_SOURCES)
    list(FIND _emit_llvmir_processed_sources "${_processed_key}" _already_processed)
    if(_already_processed GREATER -1)
      continue()
    endif()

    # Mark this source file as processed for this target
    list(APPEND _emit_llvmir_processed_sources "${_processed_key}")
    set_property(GLOBAL PROPERTY CMAKE_EMIT_LLVMIR_PROCESSED_SOURCES "${_emit_llvmir_processed_sources}")

    # Compiler support already checked at module load

    # Use target name (not output name) to ensure unique paths for each target
    # This prevents conflicts when same source is used in multiple targets
    unset(_bc_output)
    _cmake_emit_llvmir_get_bc_path("${_source_path}" "${target}" _bc_output)

    if(NOT _bc_output)
      continue()
    endif()

    # OBJECT libraries own their compiler invocations even when their object
    # files are consumed by another target.  A source can be compiled by more
    # than one object library with different options, so select the command by
    # that owner rather than by the final link target.
    set(_compile_target_name "${target}")
    foreach(_object_source_compile_target IN LISTS _object_source_compile_targets)
      string(FIND "${_object_source_compile_target}" "|" _owner_separator)
      if(_owner_separator EQUAL -1)
        continue()
      endif()
      string(SUBSTRING "${_object_source_compile_target}" 0 ${_owner_separator} _object_source_path)
      math(EXPR _owner_name_offset "${_owner_separator} + 1")
      string(SUBSTRING "${_object_source_compile_target}" ${_owner_name_offset} -1 _object_compile_target)
      if("${_source_path}" STREQUAL "${_object_source_path}")
        set(_compile_target_name "${_object_compile_target}")
        break()
      endif()
    endforeach()

    # Reuse CMake's generated compiler invocation instead of reassembling
    # target, directory, interface, and per-source options manually.
    add_custom_command(
      OUTPUT "${_bc_output}"
      COMMAND ${CMAKE_COMMAND}
        "-DCOMPILE_DATABASE=${CMAKE_BINARY_DIR}/compile_commands.json"
        "-DTARGET_NAME=${_compile_target_name}"
        "-DSOURCE_PATH=${_source_path}"
        "-DBC_OUTPUT=${_bc_output}"
        "-DEXTRA_FLAGS_FILE=${_emit_llvmir_extra_flags_file}"
        -P "${_CMAKE_EMIT_LLVMIR_MODULE_DIR}/EmitLLVMIRFromCompileDatabase.cmake"
      DEPENDS "${CMAKE_BINARY_DIR}/compile_commands.json" "${_source_path}"
      COMMENT "Generating LLVM bitcode for ${_target_name}: ${_source}"
      VERBATIM
    )
    list(APPEND _bc_files "${_bc_output}")
    continue()
  endforeach()

  if(_bc_files)
    list(REMOVE_DUPLICATES _bc_files)

    _cmake_emit_llvmir_get_target_output_dir(${target} _llvmir_output_subdir)

    if(_target_type STREQUAL "STATIC_LIBRARY")
      set(_primary_output "${_llvmir_output_subdir}/${_full_output_name}")
    else()
      set(_primary_output "${_llvmir_output_subdir}/${_full_output_name}.bc")
    endif()
    set_property(TARGET ${target} PROPERTY EMIT_LLVMIR_OUTPUT_DIR "${_llvmir_output_subdir}")
    set_property(TARGET ${target} PROPERTY EMIT_LLVMIR_OUTPUT_NAME "${_full_output_name}")

    set(_all_bc_files ${_bc_files})
    set(_dependency_bc_files "")
    if(NOT _target_type STREQUAL "STATIC_LIBRARY")
      _cmake_emit_llvmir_collect_static_bitcode(${target} _dependency_bc_files "")
    endif()
    set(_whole_archive_bc_files "")
    if(NOT _target_type STREQUAL "STATIC_LIBRARY")
      _cmake_emit_llvmir_collect_whole_archive_bitcode(${target} _whole_archive_bc_files)
      list(REMOVE_ITEM _dependency_bc_files ${_whole_archive_bc_files})
    endif()
    list(REMOVE_DUPLICATES _all_bc_files)
    set(_static_missing_files "")
    _cmake_emit_llvmir_collect_static_native_member_files(${target} _static_missing_files "")
    list(REMOVE_DUPLICATES _static_missing_files)
    list(JOIN _static_missing_files "," _static_missing_files_argument)
    set(_project_static_libraries "")
    foreach(_missing_file IN LISTS _static_missing_files)
      string(REGEX REPLACE "_missing$" "" _static_archive "${_missing_file}")
      get_filename_component(_static_archive_name "${_static_archive}" NAME)
      list(APPEND _project_static_libraries "${_static_archive_name}")
    endforeach()
    list(REMOVE_DUPLICATES _project_static_libraries)
    list(JOIN _project_static_libraries "," _project_static_libraries_argument)
    _cmake_emit_llvmir_collect_project_shared_outputs(${target} _project_shared_libraries "")
    list(JOIN _project_shared_libraries "|" _project_shared_libraries_argument)

    list(JOIN _bc_files "," _direct_bc_files_argument)
    list(JOIN _dependency_bc_files "," _static_bc_files_argument)
    list(JOIN _whole_archive_bc_files "," _whole_archive_bc_files_argument)

    _cmake_emit_llvmir_handle_exports(${target})
    get_target_property(_version_script_name ${target} EMIT_LLVMIR_VERSION_SCRIPT_NAME)
    get_target_property(_version_script_source ${target} EMIT_LLVMIR_VERSION_SCRIPT_SOURCE)

    set(_native_link_command "")
    set(_link_byproducts "")
    if(_target_type MATCHES "^(EXECUTABLE|SHARED_LIBRARY|MODULE_LIBRARY)$")
      set_property(TARGET ${target} PROPERTY EMIT_LLVMIR_OUTPUT "${_primary_output}")
      set(_link_script "${_llvmir_output_subdir}/${_full_output_name}_cmd")
      set_property(TARGET ${target} PROPERTY EMIT_LLVMIR_LINK_SCRIPT "${_link_script}")
      set_property(TARGET ${target} APPEND PROPERTY EMIT_LLVMIR_ADDITIONAL_FILES "${_link_script}")
      set(_merged_bc_relative_path "${_full_output_name}.bc")
      get_target_property(_target_binary_dir ${target} BINARY_DIR)
      if(NOT _target_binary_dir OR _target_binary_dir STREQUAL "_target_binary_dir-NOTFOUND")
        set(_target_binary_dir "${CMAKE_BINARY_DIR}")
      endif()
      # Collect all directories the linker will search for -l inputs. This is
      # used by EmitLLVMIRLinkCommand.cmake to rewrite '-lfoo' arguments into
      # '-l:libfoo.so.<version>' so that installed relink scripts do not
      # require a development-package libfoo.so symlink.
      set(_link_search_dirs "")
      if(CMAKE_C_IMPLICIT_LINK_DIRECTORIES)
        list(APPEND _link_search_dirs ${CMAKE_C_IMPLICIT_LINK_DIRECTORIES})
      endif()
      if(CMAKE_CXX_IMPLICIT_LINK_DIRECTORIES)
        list(APPEND _link_search_dirs ${CMAKE_CXX_IMPLICIT_LINK_DIRECTORIES})
      endif()
      list(REMOVE_DUPLICATES _link_search_dirs)
      list(JOIN _link_search_dirs "," _link_search_dirs_argument)

      set(_native_link_command
        ${CMAKE_COMMAND}
        "-DTARGET_NAME=${target}"
        "-DTARGET_TYPE=${_target_type}"
        "-DBC_FILE=${_merged_bc_relative_path}"
        "-DLINK_SCRIPT=${_link_script}"
        "-DOUTPUT_NAME=${_full_output_name}"
        "-DVERSION_SCRIPT_NAME=${_version_script_name}"
        "-DVERSION_SCRIPT_SOURCE=${_version_script_source}"
        "-DNATIVE_OUTPUT_NAME=${_native_output_name}"
        "-DSTATIC_MISSING_FILES=${_static_missing_files_argument}"
        "-DPROJECT_STATIC_LIBRARIES=${_project_static_libraries_argument}"
        "-DPROJECT_SHARED_LIBRARIES=${_project_shared_libraries_argument}"
        "-DNATIVE_LINK_FILE=${_target_binary_dir}/CMakeFiles/${target}.dir/link.txt"
        "-DNINJA_BUILD_DIR=${CMAKE_BINARY_DIR}"
        "-DTARGET_BINARY_DIR=${_target_binary_dir}"
        "-DLINK_SEARCH_DIRS=${_link_search_dirs_argument}"
        "-DCLANG_VERSION_LINE=${CMAKE_EMIT_LLVMIR_CLANG_VERSION_LINE}"
        "-DCMAKE_MAKE_PROGRAM=${CMAKE_MAKE_PROGRAM}"
        -P "${_CMAKE_EMIT_LLVMIR_MODULE_DIR}/EmitLLVMIRLinkCommand.cmake"
      )
      list(APPEND _link_byproducts "${_link_script}")
    endif()

    if(_target_type STREQUAL "STATIC_LIBRARY")
      set_property(TARGET ${target} PROPERTY EMIT_LLVMIR_OUTPUT "${_primary_output}")
      list(JOIN _bc_files "," _archive_bc_files_argument)
      set(_archive_command
        ${CMAKE_COMMAND}
        "-DLLVM_AR=${LLVM_llvm-ar_EXECUTABLE}"
        "-DARCHIVE_OUTPUT=${_primary_output}"
        "-DMISSING_FILE=${_primary_output}_missing"
        "-DNATIVE_LINK_FILE=${_target_binary_dir}/CMakeFiles/${target}.dir/link.txt"
        "-DTARGET_BINARY_DIR=${_target_binary_dir}"
        "-DTARGET_NAME=${target}"
        "-DNINJA_BUILD_DIR=${CMAKE_BINARY_DIR}"
        "-DCMAKE_MAKE_PROGRAM=${CMAKE_MAKE_PROGRAM}"
        "-DBC_FILES=${_archive_bc_files_argument}"
        -P "${_CMAKE_EMIT_LLVMIR_MODULE_DIR}/EmitLLVMIRArchive.cmake"
      )
    else()
      set(_archive_command "")
    endif()

    get_property(_registered_bc_outputs GLOBAL PROPERTY CMAKE_EMIT_LLVMIR_REGISTERED_OUTPUTS)
    list(FIND _registered_bc_outputs "${_primary_output}" _merged_bc_registered)
    if(_merged_bc_registered EQUAL -1)
      if(_target_type STREQUAL "STATIC_LIBRARY")
        add_custom_command(
          OUTPUT "${_primary_output}"
          COMMAND ${_archive_command}
          DEPENDS ${_all_bc_files}
          COMMENT "Creating LLVM archive for ${target}"
          VERBATIM
        )
      else()
        add_custom_command(
          OUTPUT "${_primary_output}"
          BYPRODUCTS ${_link_byproducts}
          COMMAND ${CMAKE_COMMAND}
            "-DLLVM_LINK=${LLVM_llvm-link_EXECUTABLE}"
            "-DOUTPUT=${_primary_output}"
            "-DDIRECT_BC_FILES=${_direct_bc_files_argument}"
            "-DSTATIC_BC_FILES=${_static_bc_files_argument}"
            "-DWHOLE_ARCHIVE_BC_FILES=${_whole_archive_bc_files_argument}"
            -P "${_CMAKE_EMIT_LLVMIR_MODULE_DIR}/EmitLLVMIRMerge.cmake"
          COMMAND ${_native_link_command}
          DEPENDS ${_all_bc_files}
          COMMENT "Merging LLVM bitcode for ${target}"
          VERBATIM
        )
      endif()
      list(APPEND _registered_bc_outputs "${_primary_output}")
      set_property(GLOBAL PROPERTY CMAKE_EMIT_LLVMIR_REGISTERED_OUTPUTS "${_registered_bc_outputs}")
    endif()

    set(_bc_target "${target}_llvmir")
    add_custom_target(${_bc_target} ALL
      DEPENDS "${_primary_output}"
      SOURCES ${_bc_files}
    )

    # Make llvmir target depend on the standard target
    # This ensures generated source files are built before bitcode generation
    # When building ${_bc_target}, CMake will first build ${target}
    add_dependencies(${_bc_target} ${target})

    if(NOT _target_type STREQUAL "STATIC_LIBRARY")
      _cmake_emit_llvmir_collect_static_targets(${target} _dependency_bc_targets "")
      foreach(_dependency_bc_target IN LISTS _dependency_bc_targets)
        if(TARGET ${_dependency_bc_target})
          add_dependencies(${_bc_target} ${_dependency_bc_target})
        endif()
      endforeach()
    endif()

    set_property(TARGET ${target} PROPERTY EMIT_LLVMIR_OUTPUT "${_primary_output}")
    set_property(TARGET ${target} PROPERTY EMIT_LLVMIR_BC_FILES ${_bc_files})

    _cmake_emit_llvmir_setup_install(${target})
  endif()
endfunction()

# ============================================================================
# ============================================================================
# Export and Version Script Handling
# ============================================================================

function(_cmake_emit_llvmir_handle_exports target)
  get_target_property(_target_type ${target} TYPE)
  if(NOT _target_type MATCHES "^(SHARED_LIBRARY|MODULE_LIBRARY)$")
    return()
  endif()

  get_target_property(_output_name ${target} EMIT_LLVMIR_OUTPUT_NAME)
  get_target_property(_llvmir_output_subdir ${target} EMIT_LLVMIR_OUTPUT_DIR)
  if(NOT _llvmir_output_subdir OR _llvmir_output_subdir STREQUAL "_llvmir_output_subdir-NOTFOUND")
    _cmake_emit_llvmir_get_target_output_dir(${target} _llvmir_output_subdir)
  endif()
  _cmake_emit_llvmir_get_verscript_name("${_output_name}" _verscript_name)
  string(REGEX REPLACE "_verscript$" "" _export_name "${_verscript_name}")

  get_target_property(_link_options ${target} LINK_OPTIONS)
  get_target_property(_link_flags ${target} LINK_FLAGS)
  get_target_property(_target_source_dir ${target} SOURCE_DIR)
  get_directory_property(_shared_linker_flags
    DIRECTORY "${_target_source_dir}" DEFINITION CMAKE_SHARED_LINKER_FLAGS)
  if(_shared_linker_flags)
    separate_arguments(_shared_linker_flags UNIX_COMMAND "${_shared_linker_flags}")
    list(APPEND _link_options ${_shared_linker_flags})
  endif()
  if(_link_flags AND NOT _link_flags STREQUAL "_link_flags-NOTFOUND")
    separate_arguments(_link_flags UNIX_COMMAND "${_link_flags}")
    list(APPEND _link_options ${_link_flags})
  endif()

  set(_version_script_source "")
  set(_expect_xlinker_value FALSE)
  set(_expect_version_script_value FALSE)
  foreach(_option IN LISTS _link_options)
    if(_expect_version_script_value)
      # GNU-style driver invocations commonly spell this as
      # '-Xlinker --version-script -Xlinker path'. The second -Xlinker is
      # only a separator, not the version-script path.
      if(NOT _option STREQUAL "-Xlinker")
        set(_version_script_source "${_option}")
        break()
      endif()
    elseif(_expect_xlinker_value)
      if(_option MATCHES "^--?version-script=(.+)$")
        set(_version_script_source "${CMAKE_MATCH_1}")
        break()
      elseif(_option STREQUAL "--version-script" OR _option STREQUAL "-version-script")
        set(_expect_version_script_value TRUE)
      endif()
      set(_expect_xlinker_value FALSE)
    elseif(_option STREQUAL "-Xlinker")
      set(_expect_xlinker_value TRUE)
    elseif(_option MATCHES "^-Wl,--?version-script[=,](.+)$")
      set(_version_script_source "${CMAKE_MATCH_1}")
      break()
    endif()
  endforeach()

  if(_version_script_source)
    set(_version_output "${_llvmir_output_subdir}/${_verscript_name}")
    if(EXISTS "${_version_script_source}")
      configure_file("${_version_script_source}" "${_version_output}" COPYONLY)
    endif()
    set_property(TARGET ${target} APPEND PROPERTY EMIT_LLVMIR_ADDITIONAL_FILES "${_version_output}")
    set_property(TARGET ${target} PROPERTY EMIT_LLVMIR_VERSION_SCRIPT_NAME "${_verscript_name}")
    set_property(TARGET ${target} PROPERTY EMIT_LLVMIR_VERSION_SCRIPT_SOURCE "${_version_script_source}")
  endif()

  get_target_property(_version ${target} VERSION)
  if(_version)
    set(_version_output "${_llvmir_output_subdir}/${_export_name}.version.txt")
    file(WRITE "${_version_output}" "${_version}\n")
    set_property(TARGET ${target} APPEND PROPERTY EMIT_LLVMIR_ADDITIONAL_FILES "${_version_output}")
  endif()
  get_target_property(_soversion ${target} SOVERSION)
  if(_soversion)
    set(_soversion_output "${_llvmir_output_subdir}/${_export_name}.soversion.txt")
    file(WRITE "${_soversion_output}" "${_soversion}\n")
    set_property(TARGET ${target} APPEND PROPERTY EMIT_LLVMIR_ADDITIONAL_FILES "${_soversion_output}")
  endif()
endfunction()

# ============================================================================
# Install Rules
# ============================================================================

function(_cmake_emit_llvmir_setup_install target)
  get_target_property(_bc_output ${target} EMIT_LLVMIR_OUTPUT)
  get_target_property(_additional_files ${target} EMIT_LLVMIR_ADDITIONAL_FILES)
  get_target_property(_target_type ${target} TYPE)
  get_target_property(_target_name ${target} NAME)
  get_target_property(_output_name ${target} OUTPUT_NAME)
  get_target_property(_soversion ${target} SOVERSION)
  get_target_property(_version ${target} VERSION)
  get_target_property(_exclude_from_all ${target} EXCLUDE_FROM_ALL)

  # Derive executable IR placement from an explicit RUNTIME DESTINATION in
  # the same way install-wrap derives it from the installed native path: drop
  # the final executable directory and append lib/llvmir-bin. This preserves
  # nested layouts such as lib/asoftware/bin -> lib/asoftware/lib/llvmir-bin.
  if(_target_type STREQUAL "EXECUTABLE")
    get_target_property(_runtime_destination ${target}
      EMIT_LLVMIR_RUNTIME_INSTALL_DESTINATION)
    if(_runtime_destination AND
       NOT _runtime_destination STREQUAL "_runtime_destination-NOTFOUND")
      string(REGEX REPLACE "/+$" "" _runtime_destination "${_runtime_destination}")
      get_filename_component(_runtime_parent "${_runtime_destination}" DIRECTORY)
      get_filename_component(_runtime_kind "${_runtime_destination}" NAME)
      if(_runtime_parent STREQUAL ".")
        set(_runtime_parent "")
      endif()
      if(_runtime_kind STREQUAL "sbin")
        set(_llvmir_leaf "llvmir-sbin")
      elseif(_runtime_kind STREQUAL "libexec")
        set(_llvmir_leaf "llvmir-libexec")
      else()
        set(_llvmir_leaf "llvmir-bin")
      endif()
      if(_runtime_parent)
        set(_llvmir_install_dir "${_runtime_parent}/lib/${_llvmir_leaf}")
      else()
        set(_llvmir_install_dir "lib/${_llvmir_leaf}")
      endif()
    else()
      set(_llvmir_install_dir "${CMAKE_INSTALL_LIBDIR}/llvmir-bin")
    endif()
  else()
    set(_llvmir_install_dir "${CMAKE_EMIT_LLVMIR_INSTALL_DIR}")
  endif()

  # Skip installation for targets excluded from all (like disabled examples)
  if(_exclude_from_all)
    return()
  endif()

  # Check if this target has an install() rule defined
  # Only install LLVM IR for targets that have install rules
  get_property(_installed_targets GLOBAL PROPERTY CMAKE_EMIT_LLVMIR_INSTALLED_TARGETS)
  list(FIND _installed_targets "${target}" _target_has_install)
  if(_target_has_install EQUAL -1)
    # Target has no install rule - skip LLVM IR installation
    message(STATUS "CMakeEmitLLVMIR: Skipping LLVM IR installation for ${target} (no install rule)")
    return()
  endif()

  # Install bitcode file
  install(FILES "${_bc_output}"
          DESTINATION "${_llvmir_install_dir}"
          COMPONENT LLVMIR
          OPTIONAL)

  # Install additional files (version scripts, etc.)
  if(_additional_files)
    list(REMOVE_DUPLICATES _additional_files)
    foreach(_file IN LISTS _additional_files)
      if(_file)
        install(FILES "${_file}"
                DESTINATION "${_llvmir_install_dir}"
                COMPONENT LLVMIR
                OPTIONAL)
      endif()
    endforeach()
  endif()

  # Native fallback objects from this target and merged static dependencies
  # are copied beside the consumer's _cmd during link-script generation.
  if(NOT _target_type STREQUAL "STATIC_LIBRARY")
    get_filename_component(_llvmir_source_dir "${_bc_output}" DIRECTORY)
    install(DIRECTORY "${_llvmir_source_dir}/asm"
            DESTINATION "${_llvmir_install_dir}"
            COMPONENT LLVMIR
            OPTIONAL)
  endif()

endfunction()

# ============================================================================
# Global Install Target
# ============================================================================

# ============================================================================
# Process all targets at the end using deferred call
# ============================================================================

# Collect all project targets before registering LLVMIR commands. Static-library
# outputs are shared by their consumers, so they must be registered first.
function(_cmake_emit_llvmir_collect_directory_targets dir result_var)
  get_property(_targets DIRECTORY ${dir} PROPERTY BUILDSYSTEM_TARGETS)
  get_property(_subdirs DIRECTORY ${dir} PROPERTY SUBDIRECTORIES)
  foreach(_subdir IN LISTS _subdirs)
    _cmake_emit_llvmir_collect_directory_targets(${_subdir} _subdir_targets)
    list(APPEND _targets ${_subdir_targets})
  endforeach()
  set(${result_var} "${_targets}" PARENT_SCOPE)
endfunction()

function(_cmake_emit_llvmir_process_all_targets)
  if(NOT CMAKE_EMIT_LLVMIR_ENABLED)
    return()
  endif()

  # CMAKE_PROJECT_TOP_LEVEL_INCLUDES is evaluated before project() enables
  # languages, so GNUInstallDirs must be initialized here rather than at load.
  include(GNUInstallDirs)
  if(NOT DEFINED CMAKE_EMIT_LLVMIR_INSTALL_DIR)
    set(CMAKE_EMIT_LLVMIR_INSTALL_DIR "${CMAKE_INSTALL_LIBDIR}/llvmir")
  endif()
  install(CODE "
    message(STATUS \"Installing LLVM IR files to \${CMAKE_INSTALL_PREFIX}/${CMAKE_EMIT_LLVMIR_INSTALL_DIR}\")
  " COMPONENT LLVMIR)
  message(STATUS "  Install directory: ${CMAKE_EMIT_LLVMIR_INSTALL_DIR}")
  _cmake_emit_llvmir_collect_directory_targets(${CMAKE_SOURCE_DIR} _all_targets)

  # Register every static library before a consumer can compute its merged
  # bitcode as an input. This makes each static-library .bc file have exactly
  # one custom-command producer under parallel Make builds.
  foreach(_target IN LISTS _all_targets)
    get_target_property(_target_type ${_target} TYPE)
    if(_target_type STREQUAL "STATIC_LIBRARY")
      cmake_emit_llvmir_process_target(${_target})
    endif()
  endforeach()
  foreach(_target IN LISTS _all_targets)
    get_target_property(_target_type ${_target} TYPE)
    if(NOT _target_type STREQUAL "STATIC_LIBRARY")
      cmake_emit_llvmir_process_target(${_target})
    endif()
  endforeach()
endfunction()

# Use deferred call at source directory level to ensure all subdirectories are processed
cmake_language(DEFER DIRECTORY ${CMAKE_SOURCE_DIR} CALL _cmake_emit_llvmir_process_all_targets)

message(STATUS "CMakeEmitLLVMIR module loaded - automatic processing enabled")
