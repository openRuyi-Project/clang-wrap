if(NOT SCRIPT OR NOT WORK_DIR)
  message(FATAL_ERROR "SCRIPT and WORK_DIR are required")
endif()
file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}/lib64" "${WORK_DIR}/tools/registrar"
  "${WORK_DIR}/src/CMakeFiles/resources.dir")
file(WRITE "${WORK_DIR}/lib64/libRegistrar.a" "archive fixture")
file(WRITE "${WORK_DIR}/src/CMakeFiles/resources.dir/resource.S.o" "object fixture")
file(WRITE "${WORK_DIR}/inputs.rsp"
  "lib64/libRegistrar.a src/CMakeFiles/resources.dir/resource.S.o")
file(WRITE "${WORK_DIR}/fake-ninja" [=[#!/bin/sh
echo 'clang @inputs.rsp -o libexec/registrar'
]=])
file(CHMOD "${WORK_DIR}/fake-ninja" PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE)
execute_process(
  COMMAND "${CMAKE_COMMAND}"
    -DTARGET_NAME=registrar -DTARGET_TYPE=EXECUTABLE -DBC_FILE=registrar.bc
    "-DLINK_SCRIPT=${WORK_DIR}/ir/registrar_cmd"
    -DOUTPUT_NAME=registrar -DNATIVE_OUTPUT_NAME=registrar
    "-DNATIVE_LINK_FILE=${WORK_DIR}/tools/registrar/CMakeFiles/registrar.dir/link.txt"
    "-DNINJA_BUILD_DIR=${WORK_DIR}" "-DTARGET_BINARY_DIR=${WORK_DIR}/tools/registrar"
    -DNATIVE_GENERATOR=Ninja "-DCMAKE_MAKE_PROGRAM=${WORK_DIR}/fake-ninja"
    "-DCLANG_VERSION_LINE=clang test version" -P "${SCRIPT}"
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Ninja relative inputs failed: ${_output}\n${_error}")
endif()
file(GLOB _archives "${WORK_DIR}/ir/asm/*-libRegistrar.a")
file(GLOB _objects "${WORK_DIR}/ir/asm/*-resource.S.o")
if(NOT _archives OR NOT _objects)
  message(FATAL_ERROR "Ninja root-relative archive or object was not copied")
endif()
file(READ "${WORK_DIR}/ir/registrar_cmd" _script)
if(NOT _script MATCHES "\\./asm/[^']*-libRegistrar.a" OR
   NOT _script MATCHES "\\./asm/[^']*-resource.S.o")
  message(FATAL_ERROR "Relink script omitted native fallback inputs: ${_script}")
endif()