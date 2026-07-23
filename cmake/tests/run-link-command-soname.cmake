if(NOT DEFINED SCRIPT OR NOT DEFINED WORK_DIR)
  message(FATAL_ERROR "SCRIPT and WORK_DIR are required.")
endif()

if(NOT CMAKE_C_COMPILER)
  find_program(CMAKE_C_COMPILER NAMES cc clang gcc REQUIRED)
endif()

file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}")

set(_source "${WORK_DIR}/fixture.c")
set(_library "${WORK_DIR}/libsoname-fixture.so.7.3.9")
set(_soname "libsoname-fixture.so.7.3")
file(WRITE "${_source}" "int soname_fixture(void) { return 0; }\n")
execute_process(
  COMMAND "${CMAKE_C_COMPILER}" -shared -fPIC "${_source}" -Wl,-soname,${_soname} -o "${_library}"
  RESULT_VARIABLE _compile_result
  OUTPUT_VARIABLE _compile_output
  ERROR_VARIABLE _compile_error
)
if(NOT _compile_result EQUAL 0)
  message(FATAL_ERROR "Could not build SONAME fixture:\n${_compile_output}${_compile_error}")
endif()
file(CREATE_LINK "${_library}" "${WORK_DIR}/libsoname-fixture.so" SYMBOLIC)

foreach(_case IN ITEMS absolute colon bare explicit-opt)
  if(_case STREQUAL "absolute")
    set(_library_argument "${_library}")
  elseif(_case STREQUAL "colon")
    set(_library_argument "-l:libsoname-fixture.so.7.3.9")
  elseif(_case STREQUAL "explicit-opt")
    set(_library_argument "${_library}")
  else()
    set(_library_argument "-lsoname-fixture")
  endif()
  if(_case STREQUAL "explicit-opt")
    set(_optimization_argument "-O1")
  else()
    set(_optimization_argument "")
  endif()

  set(_native_link_file "${WORK_DIR}/${_case}.link.txt")
  set(_link_script "${WORK_DIR}/${_case}_cmd")
  file(WRITE "${_native_link_file}" "${CMAKE_C_COMPILER} ${_optimization_argument} -L${WORK_DIR} -o fixture ${_library_argument}\n")
  execute_process(
    COMMAND "${CMAKE_COMMAND}"
      "-DTARGET_NAME=fixture"
      "-DTARGET_TYPE=EXECUTABLE"
      "-DBC_FILE=fixture.bc"
      "-DLINK_SCRIPT=${_link_script}"
      "-DOUTPUT_NAME=fixture"
      "-DNATIVE_LINK_FILE=${_native_link_file}"
      "-DNATIVE_OUTPUT_NAME=fixture"
      "-DNINJA_BUILD_DIR=${WORK_DIR}"
      "-DTARGET_BINARY_DIR=${WORK_DIR}"
      "-DLINK_SEARCH_DIRS=${WORK_DIR}"
      "-DCLANG_VERSION_LINE=clang test version"
      "-DCMAKE_MAKE_PROGRAM=${CMAKE_MAKE_PROGRAM}"
      -P "${SCRIPT}"
    RESULT_VARIABLE _generate_result
    OUTPUT_VARIABLE _generate_output
    ERROR_VARIABLE _generate_error
  )
  if(NOT _generate_result EQUAL 0)
    message(FATAL_ERROR "Could not generate ${_case} link script:\n${_generate_output}${_generate_error}")
  endif()

  file(READ "${_link_script}" _script)
  if(NOT _script MATCHES "'-l:${_soname}'")
    message(FATAL_ERROR "${_case} input did not use fixture DT_SONAME ${_soname}:\n${_script}")
  endif()
  if(_script MATCHES "libsoname-fixture\\.so\\.7\\.3\\.9" OR
     _script MATCHES "${_library}")
    message(FATAL_ERROR "${_case} input retained the concrete shared-library filename:\n${_script}")
  endif()
  if(NOT _script MATCHES "'--output=output/fixture'")
    message(FATAL_ERROR "${_case} link script did not use the common --output= form:\n${_script}")
  endif()
  if(NOT _script MATCHES "# Clang version: clang test version")
    message(FATAL_ERROR "${_case} link script omitted its required Clang version comment:\n${_script}")
  endif()
  if(_case STREQUAL "explicit-opt")
    if(NOT _script MATCHES "'-O1'" OR _script MATCHES "'-O3'")
      message(FATAL_ERROR "Explicit optimization level was not preserved:\n${_script}")
    endif()
  elseif(NOT _script MATCHES "'-O3'")
    message(FATAL_ERROR "LLVM IR relink script did not add default -O3:\n${_script}")
  endif()
endforeach()
