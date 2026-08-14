if(NOT DEFINED SCRIPT OR NOT DEFINED WORK_DIR)
  message(FATAL_ERROR "SCRIPT and WORK_DIR are required.")
endif()

if(NOT CMAKE_C_COMPILER)
  find_program(CMAKE_C_COMPILER NAMES clang cc REQUIRED)
endif()

file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}")

set(_source "${WORK_DIR}/dep.c")
set(_library "${WORK_DIR}/libportable-dep.so.1")
file(WRITE "${_source}" "int portable_dep(void) { return 0; }\n")
execute_process(
  COMMAND "${CMAKE_C_COMPILER}" -shared -fPIC "${_source}" -Wl,-soname,libportable-dep.so.1 -o "${_library}"
  RESULT_VARIABLE _compile_result
  ERROR_VARIABLE _compile_error
)
if(NOT _compile_result EQUAL 0)
  message(FATAL_ERROR "Could not build portability fixture: ${_compile_error}")
endif()
file(CREATE_LINK "${_library}" "${WORK_DIR}/libportable-dep.so" SYMBOLIC)

function(_generate name command expect_failure version_script_name version_script_source)
  set(_native_link_file "${WORK_DIR}/${name}.link.txt")
  set(_link_script "${WORK_DIR}/${name}_cmd")
  file(WRITE "${_native_link_file}" "${command}\n")
  set(_version_script_definitions "")
  if(version_script_name)
    list(APPEND _version_script_definitions
      "-DVERSION_SCRIPT_NAME=${version_script_name}"
      "-DVERSION_SCRIPT_SOURCE=${version_script_source}"
    )
  endif()
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
      ${_version_script_definitions}
      -P "${SCRIPT}"
    RESULT_VARIABLE _generate_result
    OUTPUT_VARIABLE _generate_output
    ERROR_VARIABLE _generate_error
  )
  if(expect_failure)
    if(_generate_result EQUAL 0)
      message(FATAL_ERROR "${name} unexpectedly generated a link script.")
    endif()
    if(EXISTS "${_link_script}")
      message(FATAL_ERROR "${name} left an invalid _cmd file behind.")
    endif()
    set(${name}_OUTPUT "${_generate_output}${_generate_error}" PARENT_SCOPE)
    return()
  endif()
  if(NOT _generate_result EQUAL 0)
    message(FATAL_ERROR "Could not generate ${name} link script:\n${_generate_output}${_generate_error}")
  endif()
  file(READ "${_link_script}" _script)
  set(${name}_SCRIPT "${_script}" PARENT_SCOPE)
endfunction()

_generate(portable
  "${CMAKE_C_COMPILER} -isysroot /private/build/sysroot -I/private/build/include -Xclang -ast-dump -ffat-lto-objects -L${WORK_DIR} -lportable-dep -Xlinker --dependency-file=CMakeFiles/fixture.dir/link.d -Xlinker --dependency-file -Xlinker CMakeFiles/fixture.dir/split-link.d -Xlinker -rpath-link -Xlinker /private/build/xlinker-lib -Xlinker --rpath-link=/private/build/xlinker-equals-lib -rpath-link /private/build/direct-lib -Wl,--dependency-file,CMakeFiles/fixture.dir/wl-link.d -Wl,-rpath-link,/private/build/wl-lib -Wl,--start-group -Wl,--whole-archive -Wl,--no-whole-archive -Wl,--end-group -Wl,-rpath,\\$ORIGIN/../lib -o fixture"
  FALSE "" "")

foreach(_removed IN ITEMS -isysroot /private/build/sysroot -I/private/build/include -Xclang -ast-dump -ffat-lto-objects "-L${WORK_DIR}")
  string(FIND "${portable_SCRIPT}" "${_removed}" _position)
  if(NOT _position EQUAL -1)
    message(FATAL_ERROR "Portable script retained compiler/build option '${_removed}':\n${portable_SCRIPT}")
  endif()
endforeach()
foreach(_removed IN ITEMS dependency-file "link.d" rpath-link /private/build/xlinker-lib /private/build/xlinker-equals-lib /private/build/direct-lib /private/build/wl-lib)
  string(FIND "${portable_SCRIPT}" "${_removed}" _position)
  if(NOT _position EQUAL -1)
    message(FATAL_ERROR "Portable script retained build dependency metadata '${_removed}':\n${portable_SCRIPT}")
  endif()
endforeach()

set(_libtool_version_script "${WORK_DIR}/fixture.ver")
file(WRITE "${_libtool_version_script}" "{ global: portable_dep; local: *; };\n")
_generate(split-version-script
  "${CMAKE_C_COMPILER} -Wl,-version-script -Wl,${_libtool_version_script} -o fixture"
  FALSE "fixture_verscript" "${_libtool_version_script}")
foreach(_expected IN ITEMS "-Wl,--version-script=./fixture_verscript")
  string(FIND "${split-version-script_SCRIPT}" "${_expected}" _position)
  if(_position EQUAL -1)
    message(FATAL_ERROR "Split version-script was not normalized:\n${split-version-script_SCRIPT}")
  endif()
endforeach()
string(FIND "${split-version-script_SCRIPT}" "${_libtool_version_script}" _source_path)
if(NOT _source_path EQUAL -1)
  message(FATAL_ERROR "Split version-script retained its build-tree source path:\n${split-version-script_SCRIPT}")
endif()
foreach(_preserved IN ITEMS "-L.." "-l:libportable-dep.so.1" "-Wl,--start-group" "-Wl,--whole-archive" "-Wl,--no-whole-archive" "-Wl,--end-group" "\$ORIGIN/../lib")
  string(FIND "${portable_SCRIPT}" "${_preserved}" _position)
  if(_position EQUAL -1)
    message(FATAL_ERROR "Portable script lost linker state '${_preserved}':\n${portable_SCRIPT}")
  endif()
endforeach()

foreach(_case IN ITEMS sysroot linker-script xlinker-path wl-path)
  if(_case STREQUAL "sysroot")
    set(_option "--sysroot /private/build/sysroot")
  elseif(_case STREQUAL "linker-script")
    set(_option "-Wl,-T,/private/build/layout.ld")
  elseif(_case STREQUAL "xlinker-path")
    set(_option "-Xlinker /private/build/input.o")
  else()
    set(_option "-Wl,/private/build/input.o")
  endif()
  _generate(${_case} "${CMAKE_C_COMPILER} ${_option} -o fixture" TRUE "" "")
  string(FIND "${${_case}_OUTPUT}" "Cannot generate _cmd" _diagnostic)
  if(_diagnostic EQUAL -1)
    message(FATAL_ERROR "${_case} did not fail _cmd generation:\n${${_case}_OUTPUT}")
  endif()
  string(FIND "${${_case}_OUTPUT}" "/private/build" _leak)
  if(NOT _leak EQUAL -1)
    message(FATAL_ERROR "${_case} leaked its build path:\n${${_case}_OUTPUT}")
  endif()
endforeach()