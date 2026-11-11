if(NOT DEFINED SCRIPT OR NOT DEFINED WORK_DIR OR NOT DEFINED LLVM_AR OR
   NOT DEFINED LLVM_LINK OR NOT DEFINED LLVM_NM OR NOT DEFINED LLVM_DIS)
  message(FATAL_ERROR "SCRIPT, WORK_DIR, LLVM_AR, LLVM_LINK, LLVM_NM, and LLVM_DIS are required")
endif()

file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}/src" "${WORK_DIR}/override" "${WORK_DIR}/base")
file(WRITE "${WORK_DIR}/src/main.c" [=[
extern int qh_fprintf(void);
extern int base_helper(void);
int main(void) { return qh_fprintf() + base_helper(); }
]=])
file(WRITE "${WORK_DIR}/src/override.c" [=[
int base_helper(void);
int qh_fprintf(void) { return base_helper(); }
]=])
file(WRITE "${WORK_DIR}/src/default.c" [=[
int qh_fprintf(void) { return -100; }
]=])
file(WRITE "${WORK_DIR}/src/helper.c" [=[
int base_helper(void) { return 1; }
]=])

foreach(_source IN ITEMS main override default helper)
  if(_source STREQUAL "main")
    set(_directory "${WORK_DIR}")
  elseif(_source STREQUAL "override")
    set(_directory "${WORK_DIR}/override")
  else()
    set(_directory "${WORK_DIR}/base")
  endif()
  execute_process(
    COMMAND "${CLANG}" -emit-llvm -c "${WORK_DIR}/src/${_source}.c"
      -o "${_directory}/${_source}.bc"
    RESULT_VARIABLE _compile_result
    ERROR_VARIABLE _compile_error
  )
  if(NOT _compile_result EQUAL 0)
    message(FATAL_ERROR "Failed to compile ${_source}.c: ${_compile_error}")
  endif()
endforeach()
execute_process(
  COMMAND "${LLVM_AR}" qc "${WORK_DIR}/override/liboverride.a" "${WORK_DIR}/override/override.bc"
  RESULT_VARIABLE _archive_result ERROR_VARIABLE _archive_error
)
if(NOT _archive_result EQUAL 0)
  message(FATAL_ERROR "Failed to create override archive: ${_archive_error}")
endif()
execute_process(
  COMMAND "${LLVM_AR}" qc "${WORK_DIR}/base/libbase.a"
    "${WORK_DIR}/base/default.bc" "${WORK_DIR}/base/helper.bc"
  RESULT_VARIABLE _archive_result ERROR_VARIABLE _archive_error
)
if(NOT _archive_result EQUAL 0)
  message(FATAL_ERROR "Failed to create base archive: ${_archive_error}")
endif()

# Put the replacing archive before the archive with the default implementation,
# matching qhull's user_eg3 link order.
execute_process(
  COMMAND "${CMAKE_COMMAND}"
    "-DLLVM_LINK=${LLVM_LINK}"
    "-DLLVM_NM=${LLVM_NM}"
    "-DLLVM_AR=${LLVM_AR}"
    "-DOUTPUT=${WORK_DIR}/app.bc"
    "-DDIRECT_BC_FILES=${WORK_DIR}/main.bc"
    "-DSTATIC_BC_FILES=${WORK_DIR}/override/liboverride.a,${WORK_DIR}/base/libbase.a"
    -P "${SCRIPT}"
  RESULT_VARIABLE _merge_result
  OUTPUT_VARIABLE _merge_output
  ERROR_VARIABLE _merge_error
)
if(NOT _merge_result EQUAL 0)
  message(FATAL_ERROR "Ordered static archive merge failed:\n${_merge_output}\n${_merge_error}")
endif()

execute_process(
  COMMAND "${LLVM_DIS}" "${WORK_DIR}/app.bc" -o "${WORK_DIR}/app.ll"
  RESULT_VARIABLE _dis_result
  ERROR_VARIABLE _dis_error
)
if(NOT _dis_result EQUAL 0)
  message(FATAL_ERROR "Failed to disassemble merged bitcode: ${_dis_error}")
endif()
file(READ "${WORK_DIR}/app.ll" _ir)
string(REGEX MATCHALL "define[^\n]*@qh_fprintf\\(" _qh_fprintf_definitions "${_ir}")
list(LENGTH _qh_fprintf_definitions _qh_fprintf_count)
if(NOT _qh_fprintf_count EQUAL 1)
  message(FATAL_ERROR "Expected one qh_fprintf override definition, found ${_qh_fprintf_count}")
endif()
string(FIND "${_ir}" "define dso_local i32 @qh_fprintf()" _override_index)
if(_override_index EQUAL -1)
  string(FIND "${_ir}" "define i32 @qh_fprintf()" _override_index)
endif()
if(_override_index EQUAL -1)
  message(FATAL_ERROR "Merged bitcode does not contain the earlier archive's qh_fprintf override")
endif()
if(NOT _ir MATCHES "define[^\n]*@base_helper\\(")
  message(FATAL_ERROR "A later archive member required by the override was not selected")
endif()