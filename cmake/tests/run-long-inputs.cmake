if(NOT MODULE_FILE OR NOT WORK_DIR OR NOT CLANG OR NOT LLVM_AR OR NOT LLVM_LINK OR NOT LLVM_NM)
  message(FATAL_ERROR "MODULE_FILE, WORK_DIR, CLANG, LLVM_AR, LLVM_LINK, and LLVM_NM are required")
endif()

function(_run)
  execute_process(COMMAND ${ARGV}
    RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error)
  if(NOT _result EQUAL 0)
    message(FATAL_ERROR "Command failed (${_result}): ${ARGV}\n${_output}\n${_error}")
  endif()
endfunction()

file(REMOVE_RECURSE "${WORK_DIR}")
# Exercise quoting in both the generated manifest and LLVM response files.
set(_build "${WORK_DIR}/build space,comma")
file(MAKE_DIRECTORY "${WORK_DIR}/source")
set(_project "cmake_minimum_required(VERSION 3.24)\nproject(LongInputs C)\nset(sources)\n")
string(REPEAT "long_source_name_" 8 _prefix)
foreach(_index RANGE 1 900)
  set(_source "${_prefix}${_index}.c")
  file(WRITE "${WORK_DIR}/source/${_source}" "/* empty module */\n")
  string(APPEND _project "list(APPEND sources ${_source})\n")
endforeach()
string(APPEND _project
  "add_library(large SHARED \${sources})\nadd_library(large_archive STATIC \${sources})\n")
file(WRITE "${WORK_DIR}/source/CMakeLists.txt" "${_project}")
_run("${CMAKE_COMMAND}" -E env CMAKE_EMIT_LLVMIR=1
  "${CMAKE_COMMAND}" -S "${WORK_DIR}/source" -B "${_build}" -G Ninja
  "-DCMAKE_C_COMPILER=${CLANG}" "-DCMAKE_PROJECT_INCLUDE=${MODULE_FILE}")
file(READ "${_build}/build.ninja" _ninja)
if(_ninja MATCHES "-D(DIRECT_BC_FILES|STATIC_BC_FILES|WHOLE_ARCHIVE_BC_FILES|BC_FILES)=" OR
   NOT _ninja MATCHES "-DINPUTS_FILE=")
  message(FATAL_ERROR "Generated commands must pass input manifests instead of expanded lists")
endif()

file(WRITE "${WORK_DIR}/empty.c" "/* empty module */\n")
_run("${CLANG}" -emit-llvm -c "${WORK_DIR}/empty.c" -o "${WORK_DIR}/empty.bc")
get_filename_component(_module_dir "${MODULE_FILE}" DIRECTORY)
foreach(_kind IN ITEMS merge archive)
  if(_kind STREQUAL "merge")
    set(_output "${_build}/llvmir/liblarge.so.bc")
  else()
    set(_output "${_build}/llvmir/liblarge_archive.a")
  endif()
  set(_manifest "${_output}.inputs.cmake")
  include("${_manifest}")
  list(LENGTH _direct_bc_files _count)
  string(LENGTH "${_direct_bc_files}" _length)
  if(NOT _count EQUAL 900 OR _length LESS 131072)
    message(FATAL_ERROR "Expected 900 inputs exceeding 128 KiB, got ${_count} inputs / ${_length} bytes")
  endif()
  foreach(_input IN LISTS _direct_bc_files)
    get_filename_component(_directory "${_input}" DIRECTORY)
    file(MAKE_DIRECTORY "${_directory}")
    file(CREATE_LINK "${WORK_DIR}/empty.bc" "${_input}" COPY_ON_ERROR)
  endforeach()
  if(_kind STREQUAL "merge")
    _run("${CMAKE_COMMAND}" "-DLLVM_LINK=${LLVM_LINK}" "-DLLVM_NM=${LLVM_NM}"
      "-DLLVM_AR=${LLVM_AR}" "-DOUTPUT=${_output}" "-DINPUTS_FILE=${_manifest}"
      -P "${_module_dir}/EmitLLVMIRMerge.cmake")
    _run("${LLVM_NM}" "${_output}")
  else()
    file(WRITE "${WORK_DIR}/link.txt" "llvm-ar qc liblarge_archive.a\n")
    _run("${CMAKE_COMMAND}" "-DLLVM_AR=${LLVM_AR}" "-DARCHIVE_OUTPUT=${_output}"
      "-DMISSING_FILE=${_output}_missing" "-DNATIVE_LINK_FILE=${WORK_DIR}/link.txt"
      "-DTARGET_BINARY_DIR=${_build}" -DTARGET_NAME=large_archive
      "-DNINJA_BUILD_DIR=${_build}" -DCMAKE_MAKE_PROGRAM=ninja
      "-DINPUTS_FILE=${_manifest}" -P "${_module_dir}/EmitLLVMIRArchive.cmake")
    execute_process(COMMAND "${LLVM_AR}" t "${_output}"
      RESULT_VARIABLE _result OUTPUT_VARIABLE _members ERROR_VARIABLE _error)
    string(REGEX MATCHALL "[^\n]+" _members "${_members}")
    list(LENGTH _members _member_count)
    if(NOT _result EQUAL 0 OR NOT _member_count EQUAL 900)
      message(FATAL_ERROR "Expected 900 archive members, got ${_member_count}: ${_error}")
    endif()
  endif()
endforeach()