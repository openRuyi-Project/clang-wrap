cmake_minimum_required(VERSION 3.24)
project(CMakeEmitLLVMIRTests NONE)

include(CTest)
if(BUILD_TESTING)
  add_subdirectory(tests)
endif()