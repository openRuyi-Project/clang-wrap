if(NOT DEFINED SCRIPT OR NOT DEFINED WORK_DIR OR NOT DEFINED LLVM_AR OR NOT DEFINED NINJA)
  message(FATAL_ERROR "SCRIPT, WORK_DIR, LLVM_AR, and NINJA are required")
endif()

file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}/CMakeFiles/archive.dir")
file(WRITE "${WORK_DIR}/source.bc" "bitcode")
file(WRITE "${WORK_DIR}/CMakeFiles/archive.dir/object.o" "native object")
get_filename_component(_llvm_ar_dir "${LLVM_AR}" DIRECTORY)
get_filename_component(_llvm_ar_name "${LLVM_AR}" NAME)
find_program(_native_llvm_ar NAMES llvm-ar llvm-ar-22 llvm-ar-23 REQUIRED)
get_filename_component(_native_llvm_ar_dir "${_native_llvm_ar}" DIRECTORY)
get_filename_component(_native_llvm_ar_name "${_native_llvm_ar}" NAME)

file(WRITE "${WORK_DIR}/build.ninja" [=[
rule archive
  command = /usr/bin/cmake -E rm -f libfixture.a && @NATIVE_LLVM_AR@ qc libfixture.a CMakeFiles/archive.dir/object.o && /usr/bin/llvm-ranlib libfixture.a
build libfixture.a: archive CMakeFiles/archive.dir/object.o
build archive: phony libfixture.a
]=])

file(READ "${WORK_DIR}/build.ninja" _ninja_file)
string(REPLACE "@NATIVE_LLVM_AR@" "${_native_llvm_ar}" _ninja_file "${_ninja_file}")
file(WRITE "${WORK_DIR}/build.ninja" "${_ninja_file}")

execute_process(
  COMMAND "${NINJA}" -C "${WORK_DIR}" -t commands archive
  RESULT_VARIABLE _ninja_result
  OUTPUT_VARIABLE _ninja_commands
  ERROR_VARIABLE _ninja_error
)
if(NOT _ninja_result EQUAL 0)
  message(FATAL_ERROR "Ninja command introspection failed: ${_ninja_error}")
endif()

# Ninja builds can omit the Makefile-only link.txt, which exposed the missing
# generator argument in the production command.
execute_process(
  COMMAND "${CMAKE_COMMAND}"
    "-DLLVM_AR=${LLVM_AR}"
    "-DARCHIVE_OUTPUT=${WORK_DIR}/llvmir/libfixture.a"
    "-DMISSING_FILE=${WORK_DIR}/llvmir/libfixture.a_missing"
    "-DNATIVE_LINK_FILE=${WORK_DIR}/CMakeFiles/archive.dir/link.txt"
    "-DTARGET_BINARY_DIR=${WORK_DIR}"
    "-DTARGET_NAME=archive"
    "-DNINJA_BUILD_DIR=${WORK_DIR}"
    "-DNATIVE_GENERATOR=Ninja"
    "-DCMAKE_MAKE_PROGRAM=${NINJA}"
    "-DBC_FILES=${WORK_DIR}/source.bc"
    -P "${SCRIPT}"
  RESULT_VARIABLE _archive_result
  OUTPUT_VARIABLE _archive_output
  ERROR_VARIABLE _archive_error
)
if(NOT _archive_result EQUAL 0)
  message(FATAL_ERROR "Ninja static archive generation failed:\n${_archive_output}\n${_archive_error}")
endif()

if(NOT EXISTS "${WORK_DIR}/llvmir/libfixture.a")
  message(FATAL_ERROR "Ninja static archive generator did not produce the LLVM archive")
endif()
if(NOT EXISTS "${WORK_DIR}/llvmir/libfixture.a_missing")
  message(FATAL_ERROR "Native fallback object was not recorded")
endif()