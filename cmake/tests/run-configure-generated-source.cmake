file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}/source")
file(WRITE "${WORK_DIR}/source/CMakeLists.txt" [=[
cmake_minimum_required(VERSION 3.24)
project(ConfigureGeneratedSource C)
# Like freecell-solver's lookup tables, this file exists only in the build
# directory and is not declared as a custom command OUTPUT or GENERATED.
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/lookup.c" "int lookup(void) { return 42; }\n")
add_library(lookup SHARED lookup.c)
add_library(lookup_static STATIC lookup.c)
install(TARGETS lookup lookup_static DESTINATION lib)
]=])
execute_process(
  COMMAND "${CMAKE_COMMAND}" -E env CMAKE_EMIT_LLVMIR=1
    "${CMAKE_COMMAND}" -S "${WORK_DIR}/source" -B "${WORK_DIR}/build" -G Ninja
    -DCMAKE_C_COMPILER=clang "-DCMAKE_PROJECT_INCLUDE=${MODULE_FILE}"
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Configure-generated source failed: ${_output}\n${_error}")
endif()
execute_process(
  COMMAND "${CMAKE_COMMAND}" --build "${WORK_DIR}/build"
    --target lookup_llvmir lookup_static_llvmir
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Configure-generated source IR build failed: ${_output}\n${_error}")
endif()
foreach(_file IN ITEMS liblookup.so.bc liblookup.so_cmd liblookup_static.a)
  if(NOT EXISTS "${WORK_DIR}/build/llvmir/${_file}")
    message(FATAL_ERROR "Missing LLVM IR output: ${_file}")
  endif()
endforeach()