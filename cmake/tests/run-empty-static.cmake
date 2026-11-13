if(NOT MODULE_FILE OR NOT WORK_DIR)
  message(FATAL_ERROR "MODULE_FILE and WORK_DIR are required")
endif()
file(REMOVE_RECURSE "${WORK_DIR}")
file(MAKE_DIRECTORY "${WORK_DIR}/source")
file(WRITE "${WORK_DIR}/source/CMakeLists.txt" [=[
cmake_minimum_required(VERSION 3.24)
project(EmptyStatic C)
file(WRITE "${CMAKE_CURRENT_SOURCE_DIR}/empty.h" "/* header-only dependency */\n")
file(WRITE "${CMAKE_CURRENT_SOURCE_DIR}/value.c" "int value(void) { return 42; }\n")
file(WRITE "${CMAKE_CURRENT_SOURCE_DIR}/consumer.c" "extern int value(void); int consumer(void) { return value(); }\n")
# Like XNNPACK's system fp16/fxdiv shims, these are real static targets
# containing headers only. The native archives have no object members.
add_library(fp16 STATIC empty.h)
add_library(fxdiv STATIC empty.h)
set_target_properties(fp16 fxdiv PROPERTIES LINKER_LANGUAGE C)
add_library(value STATIC value.c)
target_link_libraries(fxdiv PUBLIC value)
add_library(consumer SHARED consumer.c)
target_link_libraries(consumer PRIVATE fp16 fxdiv)
add_library(whole_consumer SHARED consumer.c)
target_link_libraries(whole_consumer PRIVATE "-Wl,--whole-archive" fp16 "-Wl,--no-whole-archive" fxdiv)
install(TARGETS consumer whole_consumer DESTINATION lib)
]=])
execute_process(
  COMMAND "${CMAKE_COMMAND}" -E env CMAKE_EMIT_LLVMIR=1
    "${CMAKE_COMMAND}" -S "${WORK_DIR}/source" -B "${WORK_DIR}/build" -G Ninja
    -DCMAKE_C_COMPILER=clang "-DCMAKE_PROJECT_INCLUDE=${MODULE_FILE}"
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Configure failed: ${_output}\n${_error}")
endif()
execute_process(
  COMMAND "${CMAKE_COMMAND}" --build "${WORK_DIR}/build"
    --target consumer_llvmir whole_consumer_llvmir
  RESULT_VARIABLE _result OUTPUT_VARIABLE _output ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "Empty static dependency IR build failed: ${_output}\n${_error}")
endif()
foreach(_target IN ITEMS consumer whole_consumer)
  if(NOT EXISTS "${WORK_DIR}/build/llvmir/lib${_target}.so.bc")
    message(FATAL_ERROR "Missing ${_target} bitcode")
  endif()
  file(READ "${WORK_DIR}/build/llvmir/lib${_target}.so.bc.inputs.cmake" _inputs)
  if(_inputs MATCHES "lib(fp16|fxdiv)\\.a" OR NOT _inputs MATCHES "libvalue\\.a")
    message(FATAL_ERROR "Incorrect empty/transitive static inputs: ${_inputs}")
  endif()
endforeach()