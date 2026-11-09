if(NOT DEFINED MODULE_FILE OR NOT DEFINED WORK_DIR)
  message(FATAL_ERROR "MODULE_FILE and WORK_DIR are required")
endif()

file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}/source/first" "${WORK_DIR}/source/second")
file(WRITE "${WORK_DIR}/source/CMakeLists.txt" [=[
cmake_minimum_required(VERSION 3.24)
project(NoLanguageRoot NONE)
add_subdirectory(first)
add_subdirectory(second)
]=])
if(DEFINED TEST_OUTPUT_DIR)
  set(_output_option "-DCMAKE_EMIT_LLVMIR_OUTPUT_DIR=${TEST_OUTPUT_DIR}")
else()
  set(TEST_OUTPUT_DIR llvmir)
endif()
if(NOT TEST_OUTPUT_DIR)
  set(TEST_OUTPUT_DIR llvmir)
endif()
get_filename_component(_output_root "${TEST_OUTPUT_DIR}" ABSOLUTE BASE_DIR "${WORK_DIR}/build")
foreach(_subdir IN ITEMS first second)
  file(WRITE "${WORK_DIR}/source/${_subdir}/CMakeLists.txt" "
project(${_subdir} C)
add_library(${_subdir} STATIC value.c)
install(TARGETS ${_subdir} ARCHIVE DESTINATION lib)
")
  file(WRITE "${WORK_DIR}/source/${_subdir}/value.c"
    "int ${_subdir}_value(void) { return 42; }\n")
endforeach()

execute_process(
  COMMAND "${CMAKE_COMMAND}" -E env CMAKE_EMIT_LLVMIR=1 CC=clang
    "${CMAKE_COMMAND}" -S "${WORK_DIR}/source" -B "${WORK_DIR}/build" -G Ninja
    "-DCMAKE_PROJECT_INCLUDE=${MODULE_FILE}"
    ${_output_option}
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Subproject configuration failed:\n${_output}\n${_error}")
endif()
execute_process(
  COMMAND "${CMAKE_COMMAND}" --build "${WORK_DIR}/build"
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Subproject build failed:\n${_output}\n${_error}")
endif()
foreach(_subdir IN ITEMS first second)
  set(_root "${_output_root}/${_subdir}")
  file(GLOB _objects "${_root}/objects/${_subdir}_*.bc")
  list(LENGTH _objects _count)
  if(NOT _count EQUAL 1 OR NOT EXISTS "${_root}/lib${_subdir}.a")
    message(FATAL_ERROR "Missing LLVM IR artifacts below ${_root}")
  endif()
endforeach()
execute_process(
  COMMAND "${CMAKE_COMMAND}" --install "${WORK_DIR}/build" --prefix "${WORK_DIR}/install"
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Subproject install failed:\n${_output}\n${_error}")
endif()
foreach(_subdir IN ITEMS first second)
  file(GLOB_RECURSE _installed_ir "${WORK_DIR}/install/*/llvmir/lib${_subdir}.a")
  if(NOT _installed_ir)
    message(FATAL_ERROR "Missing installed LLVM IR archive for ${_subdir}")
  endif()
endforeach()