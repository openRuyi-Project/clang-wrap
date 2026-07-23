if(NOT DEFINED CASE_NAME OR NOT DEFINED SCRIPT OR NOT DEFINED WORK_DIR)
  message(FATAL_ERROR "CASE_NAME, SCRIPT, and WORK_DIR are required.")
endif()

file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}")

if(CASE_NAME STREQUAL "response-file-missing" OR CASE_NAME STREQUAL "response-file-cycle" OR CASE_NAME STREQUAL "response-file-unterminated-quote")
  set(_native_link_file "${WORK_DIR}/link.txt")
  if(CASE_NAME STREQUAL "response-file-missing")
    file(WRITE "${_native_link_file}" "clang @missing.rsp -o app\n")
    set(_expected_error "Response file does not exist")
  elseif(CASE_NAME STREQUAL "response-file-cycle")
    file(WRITE "${WORK_DIR}/first.rsp" "@second.rsp\n")
    file(WRITE "${WORK_DIR}/second.rsp" "@first.rsp\n")
    file(WRITE "${_native_link_file}" "clang @first.rsp -o app\n")
    set(_expected_error "inclusion cycle")
  else()
    file(WRITE "${WORK_DIR}/bad.rsp" "'unterminated\n")
    file(WRITE "${_native_link_file}" "clang @bad.rsp -o app\n")
    set(_expected_error "unterminated quote")
  endif()
  execute_process(
    COMMAND "${CMAKE_COMMAND}"
      -DTARGET_NAME=app -DTARGET_TYPE=EXECUTABLE -DBC_FILE=app.bc
      -DLINK_SCRIPT=${WORK_DIR}/app_cmd -DOUTPUT_NAME=app -DNATIVE_OUTPUT_NAME=app
      -DNATIVE_LINK_FILE=${_native_link_file} -DNINJA_BUILD_DIR=${WORK_DIR}
      -DTARGET_BINARY_DIR=${WORK_DIR}
      -DCLANG_VERSION_LINE=clang\ test\ version -DCMAKE_MAKE_PROGRAM=ninja -P "${SCRIPT}"
    RESULT_VARIABLE _result ERROR_VARIABLE _error
  )
  if(_result EQUAL 0 OR NOT _error MATCHES "${_expected_error}")
    message(FATAL_ERROR "Response-file failure was not reported:\n${_error}")
  endif()
elseif(CASE_NAME STREQUAL "compile-db-ambiguous")
  set(_source "${WORK_DIR}/source.c")
  file(WRITE "${_source}" "int value(void) { return 0; }\n")
  file(WRITE "${WORK_DIR}/compile_commands.json" "[{\"directory\":\"${WORK_DIR}\",\"command\":\"clang -c source.c -o first.o\",\"file\":\"${_source}\"},{\"directory\":\"${WORK_DIR}\",\"command\":\"clang -c source.c -o second.o\",\"file\":\"${_source}\"}]")
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -DCOMPILE_DATABASE=${WORK_DIR}/compile_commands.json
      -DTARGET_NAME=app -DSOURCE_PATH=${_source} -DBC_OUTPUT=${WORK_DIR}/source.bc -P "${SCRIPT}"
    RESULT_VARIABLE _result ERROR_VARIABLE _error
  )
  if(_result EQUAL 0 OR NOT _error MATCHES "Multiple compile commands")
    message(FATAL_ERROR "Ambiguous compile database rejection was not reported:\n${_error}")
  endif()
elseif(CASE_NAME STREQUAL "ninja-link-ambiguous")
  set(_fake_ninja "${WORK_DIR}/ninja")
  file(WRITE "${_fake_ninja}" "#!/bin/sh\nprintf '%s\\n' 'clang first.o -o app' 'clang second.o -o app'\n")
  file(CHMOD "${_fake_ninja}" PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE)
  execute_process(
    COMMAND "${CMAKE_COMMAND}"
      -DTARGET_NAME=app -DTARGET_TYPE=EXECUTABLE -DBC_FILE=app.bc
      -DLINK_SCRIPT=${WORK_DIR}/app_cmd -DOUTPUT_NAME=app -DNATIVE_OUTPUT_NAME=app
      -DNATIVE_LINK_FILE=${WORK_DIR}/missing-link.txt -DNINJA_BUILD_DIR=${WORK_DIR}
      -DTARGET_BINARY_DIR=${WORK_DIR}
      -DCLANG_VERSION_LINE=clang\ test\ version -DCMAKE_MAKE_PROGRAM=${_fake_ninja} -P "${SCRIPT}"
    RESULT_VARIABLE _result ERROR_VARIABLE _error
  )
  if(_result EQUAL 0 OR NOT _error MATCHES "Expected exactly one Ninja link command")
    message(FATAL_ERROR "Ambiguous Ninja link-command rejection was not reported:\n${_error}")
  endif()
else()
  message(FATAL_ERROR "Unknown negative test case: ${CASE_NAME}")
endif()