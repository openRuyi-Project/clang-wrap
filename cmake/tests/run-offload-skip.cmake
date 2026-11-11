if(NOT MODULE_FILE OR NOT WORK_DIR)
  message(FATAL_ERROR "MODULE_FILE and WORK_DIR are required")
endif()
file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}/source")
file(WRITE "${WORK_DIR}/source/value.cpp" "int value() { return 1; }\n")
file(WRITE "${WORK_DIR}/source/CMakeLists.txt" [=[
cmake_minimum_required(VERSION 3.24)
project(OffloadSkip CXX)
# libjpeg-turbo lists configure_file outputs as relative custom-target sources.
# Offload classification must not resolve them against the source directory.
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/package.in" "#!/bin/sh\n")
configure_file("${CMAKE_CURRENT_BINARY_DIR}/package.in" pkgscripts/makerpm COPYONLY)
add_custom_target(rpm pkgscripts/makerpm SOURCES pkgscripts/makerpm)
add_library(gpu STATIC EXCLUDE_FROM_ALL value.cpp)
target_compile_options(gpu PRIVATE -x hip --offload-arch=gfx1100)
add_library(consumer STATIC EXCLUDE_FROM_ALL value.cpp)
target_link_libraries(consumer PRIVATE gpu)
# Model MAGMA's imported hip::device and transitive shared-library usage.
add_library(hip::device INTERFACE IMPORTED)
set_property(TARGET hip::device PROPERTY INTERFACE_HIP_DEVICE_COMPILE ON)
set_property(TARGET hip::device PROPERTY INTERFACE_COMPILE_OPTIONS
  "$<$<COMPILE_LANGUAGE:CXX>:-x;hip;--offload-arch=gfx1100>")
add_library(magma SHARED EXCLUDE_FROM_ALL value.cpp)
target_link_libraries(magma PUBLIC hip::device)
add_library(tester SHARED EXCLUDE_FROM_ALL value.cpp)
target_link_libraries(tester PUBLIC magma)
add_library(magma_sparse SHARED EXCLUDE_FROM_ALL value.cpp)
target_link_libraries(magma_sparse PUBLIC magma hip::device)
# Also cover imported interfaces that expose plain options without the marker.
add_library(device_options INTERFACE IMPORTED)
set_property(TARGET device_options PROPERTY INTERFACE_COMPILE_OPTIONS "-x;hip;--offload-arch=gfx1101")
add_library(imported_options SHARED EXCLUDE_FROM_ALL value.cpp)
target_link_libraries(imported_options PRIVATE device_options)
# The HIP package can hide its options inside generator expressions. The
# device-compile interface marker must also be sufficient on its own.
add_library(device_marker INTERFACE IMPORTED)
set_property(TARGET device_marker PROPERTY INTERFACE_HIP_DEVICE_COMPILE ON)
add_library(imported_marker SHARED EXCLUDE_FROM_ALL value.cpp)
target_link_libraries(imported_marker PRIVATE device_marker)
# Neither private GPU options on a shared dependency nor LINK_ONLY interfaces
# change the consumer's compilation mode. Both consumers must retain IR.
add_library(private_gpu SHARED EXCLUDE_FROM_ALL value.cpp)
target_compile_options(private_gpu PRIVATE -x hip --offload-arch=gfx1100)
add_library(host_consumer SHARED EXCLUDE_FROM_ALL value.cpp)
target_link_libraries(host_consumer PRIVATE private_gpu)
add_library(link_only_consumer SHARED EXCLUDE_FROM_ALL value.cpp)
target_link_libraries(link_only_consumer PRIVATE "$<LINK_ONLY:hip::device>")
add_library(plain SHARED value.cpp)
# yaml-cpp contributes several sources through one conditional expression.
file(WRITE "${CMAKE_CURRENT_SOURCE_DIR}/first.cpp" "/* first conditional source */\n")
file(WRITE "${CMAKE_CURRENT_SOURCE_DIR}/second.cpp" "/* second conditional source */\n")
target_sources(plain PRIVATE
  "$<$<BOOL:ON>:${CMAKE_CURRENT_SOURCE_DIR}/first.cpp;${CMAKE_CURRENT_SOURCE_DIR}/second.cpp>")
target_compile_definitions(plain PRIVATE "HIP_COMPILER_FLAGS=\" -x hip --offload-arch=gfx1100 \"")
install(TARGETS gpu consumer plain DESTINATION lib)
install(TARGETS magma tester magma_sparse imported_options imported_marker DESTINATION lib)
]=])
execute_process(
  COMMAND "${CMAKE_COMMAND}" -E env CMAKE_EMIT_LLVMIR=1
    "${CMAKE_COMMAND}" -S "${WORK_DIR}/source" -B "${WORK_DIR}/build" -G Ninja
    -DCMAKE_CXX_COMPILER=clang++
    "-DCMAKE_PROJECT_INCLUDE=${MODULE_FILE}"
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Configure failed: ${_output}\n${_error}")
endif()
file(READ "${WORK_DIR}/build/build.ninja" _ninja)
foreach(_target IN ITEMS gpu consumer magma tester magma_sparse imported_options imported_marker private_gpu)
  if(_ninja MATCHES "build ${_target}_llvmir:" OR
     _ninja MATCHES "-DTARGET_NAME=${_target} " OR
     NOT _output MATCHES "skipping LLVM IR for ${_target}:")
    message(FATAL_ERROR "Offload target ${_target} was not fully skipped: ${_output}")
  endif()
endforeach()
foreach(_target IN ITEMS plain host_consumer link_only_consumer)
  if(NOT _ninja MATCHES "build ${_target}_llvmir:")
    message(FATAL_ERROR "Host target ${_target} incorrectly lost IR: ${_output}")
  endif()
endforeach()
execute_process(
  COMMAND "${CMAKE_COMMAND}" --build "${WORK_DIR}/build" --target plain_llvmir
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Ordinary C++ IR build failed: ${_output}\n${_error}")
endif()
file(READ "${WORK_DIR}/build/cmake_install.cmake" _install)
if(_install MATCHES "llvmir/lib(gpu|consumer|magma|tester|magma_sparse|imported_options|imported_marker)\\.(a|so)")
  message(FATAL_ERROR "Skipped targets still have IR install rules")
endif()