if(NOT MODULE_FILE OR NOT WORK_DIR)
  message(FATAL_ERROR "MODULE_FILE and WORK_DIR are required")
endif()
file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}/source")
file(WRITE "${WORK_DIR}/source/CMakeLists.txt" [=[
cmake_minimum_required(VERSION 3.24)
project(HeaderOnlySource CXX)
file(WRITE "${CMAKE_CURRENT_SOURCE_DIR}/included.cpp" "int included() { return 21; }\n")
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/database.cpp" "int database() { return 21; }\n")
file(WRITE "${CMAKE_CURRENT_SOURCE_DIR}/main.cpp"
  "#include \"included.cpp\"\n#include \"database.cpp\"\nint value() { return included() + database(); }\n")
add_library(value SHARED main.cpp included.cpp "${CMAKE_CURRENT_BINARY_DIR}/database.cpp")
set_source_files_properties(included.cpp PROPERTIES HEADER_FILE_ONLY ON)
set_source_files_properties("${CMAKE_CURRENT_BINARY_DIR}/database.cpp" PROPERTIES
  GENERATED TRUE HEADER_FILE_ONLY ON)
target_include_directories(value PRIVATE "${CMAKE_CURRENT_BINARY_DIR}")
install(TARGETS value DESTINATION lib)
]=])
execute_process(
  COMMAND "${CMAKE_COMMAND}" -E env CMAKE_EMIT_LLVMIR=1
    "${CMAKE_COMMAND}" -S "${WORK_DIR}/source" -B "${WORK_DIR}/build" -G Ninja
    -DCMAKE_CXX_COMPILER=clang++ "-DCMAKE_PROJECT_INCLUDE=${MODULE_FILE}"
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Configure failed: ${_output}\n${_error}")
endif()
execute_process(
  COMMAND "${CMAKE_COMMAND}" --build "${WORK_DIR}/build" --target value_llvmir
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "HEADER_FILE_ONLY IR build failed: ${_output}\n${_error}")
endif()
file(GLOB _objects "${WORK_DIR}/build/llvmir/objects/*.bc")
list(LENGTH _objects _count)
if(NOT _count EQUAL 1 OR NOT EXISTS "${WORK_DIR}/build/llvmir/libvalue.so.bc")
  message(FATAL_ERROR "Expected one translation unit and merged bitcode: ${_objects}")
endif()