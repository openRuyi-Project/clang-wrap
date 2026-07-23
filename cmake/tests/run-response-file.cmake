if(NOT DEFINED SCRIPT OR NOT DEFINED WORK_DIR)
  message(FATAL_ERROR "SCRIPT and WORK_DIR are required.")
endif()

file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}/nested")
file(WRITE "${WORK_DIR}/outer.rsp" "@nested/inner.rsp -o fixture\n")
file(WRITE "${WORK_DIR}/nested/inner.rsp" "-O1 -Wl,--as-needed @@literal\n")
file(WRITE "${WORK_DIR}/link.txt" "clang @outer.rsp\n")

execute_process(
  COMMAND "${CMAKE_COMMAND}"
    -DTARGET_NAME=fixture -DTARGET_TYPE=EXECUTABLE -DBC_FILE=fixture.bc
    -DLINK_SCRIPT=${WORK_DIR}/fixture_cmd -DOUTPUT_NAME=fixture -DNATIVE_OUTPUT_NAME=fixture
    -DNATIVE_LINK_FILE=${WORK_DIR}/link.txt -DNINJA_BUILD_DIR=${WORK_DIR}
    -DTARGET_BINARY_DIR=${WORK_DIR} -DCMAKE_MAKE_PROGRAM=ninja
    -DCLANG_VERSION_LINE=clang\ test\ version
    -P "${SCRIPT}"
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Response-file link command did not expand:\n${_output}${_error}")
endif()
file(READ "${WORK_DIR}/fixture_cmd" _script)
foreach(_expected IN ITEMS "'-O1'" "'-Wl,--as-needed'" "'@literal'" "'--output=output/fixture'")
  if(NOT _script MATCHES "${_expected}")
    message(FATAL_ERROR "Expanded link script omitted ${_expected}:\n${_script}")
  endif()
endforeach()
if(NOT _script MATCHES "# Clang version: clang test version")
  message(FATAL_ERROR "Expanded link script omitted its required Clang version comment:\n${_script}")
endif()