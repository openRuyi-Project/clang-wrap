if(NOT DEFINED SCRIPT OR NOT DEFINED WORK_DIR)
  message(FATAL_ERROR "SCRIPT and WORK_DIR are required.")
endif()

file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}")

set(_compiler "${WORK_DIR}/fake-clang")
set(_source "${WORK_DIR}/source.c")
set(_database "${WORK_DIR}/compile_commands.json")
set(_output "${WORK_DIR}/source.bc")
set(_arguments_log "${WORK_DIR}/arguments.log")

file(WRITE "${_compiler}" "#!/bin/sh\nprintf '%s\\n' \"$@\" > '${_arguments_log}'\nprintf 'fake bitcode' > \"${_output}\"\n")
file(CHMOD "${_compiler}" PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE)
file(WRITE "${_source}" "int value(void) { return 0; }\n")
file(WRITE "${_database}" "[{\"directory\":\"${WORK_DIR}\",\"command\":\"${_compiler} -c source.c -ffat-lto-objects -O2 -o source.o\",\"file\":\"${_source}\"}]")

execute_process(
  COMMAND "${CMAKE_COMMAND}"
    -DCOMPILE_DATABASE=${_database}
    -DTARGET_NAME=app
    -DSOURCE_PATH=${_source}
    -DBC_OUTPUT=${_output}
    -P "${SCRIPT}"
  RESULT_VARIABLE _result
  ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Compile database replay failed:\n${_error}")
endif()

file(READ "${_arguments_log}" _arguments)
if(_arguments MATCHES "-ffat-lto-objects")
  message(FATAL_ERROR "IR generation retained -ffat-lto-objects:\n${_arguments}")
endif()
if(NOT _arguments MATCHES "-emit-llvm")
  message(FATAL_ERROR "IR generation did not add -emit-llvm:\n${_arguments}")
endif()