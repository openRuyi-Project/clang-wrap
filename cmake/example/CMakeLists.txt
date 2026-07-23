# ============================================================================
# CMakeEmitLLVMIR - Example Project
# ============================================================================
# Example subproject containing mylib library and myapp application
#
# Usage:
#   export CMAKE_EMIT_LLVMIR=1
#   export CC=clang
#   cmake -B build -S . -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES=../cmake/CMakeEmitLLVMIR.cmake
#   cmake --build build
#   cmake --install build --prefix install
# ============================================================================

cmake_minimum_required(VERSION 3.24)
project(CMakeEmitLLVMIR_Example
  VERSION 1.0.0
  DESCRIPTION "Example project for CMakeEmitLLVMIR"
  LANGUAGES C
)

# ============================================================================
# Library sources (shared by static and shared libraries)
# ============================================================================

set(MYLIB_SOURCES
  mylib/src/math_ops.c
  mylib/src/string_ops.c
  mylib/src/utils.c
)

# ============================================================================
# Static Library
# ============================================================================

add_library(mylib_static STATIC
  ${MYLIB_SOURCES}
)

target_include_directories(mylib_static
  PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/mylib/include>
    $<INSTALL_INTERFACE:include>
)

set_target_properties(mylib_static PROPERTIES
  OUTPUT_NAME mylib
)

# ============================================================================
# Shared Library
# ============================================================================

add_library(mylib_shared SHARED
  ${MYLIB_SOURCES}
)

target_include_directories(mylib_shared
  PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/mylib/include>
    $<INSTALL_INTERFACE:include>
)

set_target_properties(mylib_shared PROPERTIES
  VERSION ${PROJECT_VERSION}
  SOVERSION 1
  OUTPUT_NAME mylib
)

# If CMAKE_INSTALL_PREFIX ends with clang_llvmir_O3, use link flags to pass version script parameter
# This allows the CMakeEmitLLVMIR module to correctly handle version script linking
if(CMAKE_INSTALL_PREFIX MATCHES "clang_llvmir_O3$")
  target_link_options(mylib_shared PRIVATE
    "-Wl,--version-script=${CMAKE_CURRENT_SOURCE_DIR}/mylib/libmylib.version"
  )
else()
  set_target_properties(mylib_shared PROPERTIES
    LINK_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/mylib/libmylib.version"
  )
endif()

# ============================================================================
# Executable
# ============================================================================

add_executable(myapp
  myapp/main.c
)

# Link to shared library
target_link_libraries(myapp PRIVATE mylib_shared)

# ============================================================================
# Installation Rules
# ============================================================================

# Install library files
install(TARGETS mylib_static mylib_shared
  ARCHIVE DESTINATION lib
  LIBRARY DESTINATION lib
  RUNTIME DESTINATION bin
)

# Install header files
install(DIRECTORY mylib/include/
  DESTINATION include
  FILES_MATCHING PATTERN "*.h"
)

# Install executable
install(TARGETS myapp
  RUNTIME DESTINATION bin
)

# LLVM IR files are installed via:
# cmake --install build --component LLVMIR

# ============================================================================
# Configuration Summary
# ============================================================================

message(STATUS "")
message(STATUS "CMakeEmitLLVMIR Example Project Configuration")
message(STATUS "====================================")
if(DEFINED ENV{CMAKE_EMIT_LLVMIR})
  message(STATUS "  LLVM IR emission: Enabled")
  message(STATUS "  Output directory: ${CMAKE_EMIT_LLVMIR_OUTPUT_DIR}")
  message(STATUS "")
  message(STATUS "  Targets (auto-processed):")
  message(STATUS "    - mylib_static (static library) -> libmylib.a")
  message(STATUS "    - mylib_shared (shared library) -> libmylib.so")
  message(STATUS "    - myapp (executable) -> myapp")
else()
  message(STATUS "  LLVM IR emission: Disabled")
  message(STATUS "  Set CMAKE_EMIT_LLVMIR=1 to enable")
endif()
message(STATUS "")
