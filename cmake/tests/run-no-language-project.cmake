if(NOT DEFINED MODULE_FILE OR NOT DEFINED WORK_DIR)
  message(FATAL_ERROR "MODULE_FILE and WORK_DIR are required")
endif()

file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}/source/plain")
file(WRITE "${WORK_DIR}/source/CMakeLists.txt"
  "cmake_minimum_required(VERSION 3.24)\nproject(NoLanguageProject NONE)\nadd_subdirectory(plain)\n")
file(WRITE "${WORK_DIR}/source/plain/CMakeLists.txt" [=[
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/configured.txt" "configured")
]=])

execute_process(
  COMMAND "${CMAKE_COMMAND}" -E env CMAKE_EMIT_LLVMIR=1 CC=clang
    "${CMAKE_COMMAND}" -S "${WORK_DIR}/source" -B "${WORK_DIR}/build" -G Ninja
    "-DCMAKE_PROJECT_INCLUDE=${MODULE_FILE}"
  RESULT_VARIABLE _configure_result
  OUTPUT_VARIABLE _configure_output
  ERROR_VARIABLE _configure_error
)
if(NOT _configure_result EQUAL 0)
  message(FATAL_ERROR "No-language project configuration failed:\n${_configure_output}\n${_configure_error}")
endif()

if(NOT EXISTS "${WORK_DIR}/build/plain/configured.txt")
  message(FATAL_ERROR "No-language directory was not configured")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" --build "${WORK_DIR}/build"
  RESULT_VARIABLE _build_result
  OUTPUT_VARIABLE _build_output
  ERROR_VARIABLE _build_error
)
if(NOT _build_result EQUAL 0)
  message(FATAL_ERROR "Native subproject build failed:\n${_build_output}\n${_build_error}")
endif()

if(EXISTS "${WORK_DIR}/build/llvmir")
  message(FATAL_ERROR "No-language project should not create LLVM IR artifacts")
endif()