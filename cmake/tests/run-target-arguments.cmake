file(MAKE_DIRECTORY "${WORK_DIR}/source")
file(WRITE "${WORK_DIR}/source/part;source.c" "int main(void) { return 0; }\n")
file(WRITE "${WORK_DIR}/source/CMakeLists.txt" "
cmake_minimum_required(VERSION 3.24)
project(TargetArguments C)
include(\"${MODULE_FILE}\")
# Exercise forwarding without passing semicolon paths to the IR generators.
set(CMAKE_EMIT_LLVMIR_ENABLED FALSE)
add_library(argument_library STATIC \"part\\;source.c\")
add_executable(argument_executable \"part\\;source.c\")
")
execute_process(
  COMMAND "${CMAKE_COMMAND}" -E env CMAKE_EMIT_LLVMIR=1
    "${CMAKE_COMMAND}" -S "${WORK_DIR}/source" -B "${WORK_DIR}/build" -G Ninja
    -DCMAKE_C_COMPILER=clang
  RESULT_VARIABLE _result
  OUTPUT_VARIABLE _output
  ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Target argument forwarding failed:\n${_output}\n${_error}")
endif()
