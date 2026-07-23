set(_fixture_source "${CMAKE_CURRENT_SOURCE_DIR}/fixtures/selection")
set(_module "${PROJECT_SOURCE_DIR}/cmake/CMakeEmitLLVMIR.cmake")

foreach(_generator IN ITEMS Ninja "Unix Makefiles")
  string(REPLACE " " "_" _generator_id "${_generator}")
  foreach(_mode IN ITEMS ON OFF)
    add_test(
      NAME cmake-emit-llvmir-${_generator_id}-${_mode}
      COMMAND ${CMAKE_COMMAND}
        -DCASE_MODE=${_mode}
        -DFIXTURE_SOURCE=${_fixture_source}
        -DMODULE_FILE=${_module}
        -DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}/${_generator_id}-${_mode}\ build
        -DTEST_GENERATOR=${_generator}
        -P ${CMAKE_CURRENT_SOURCE_DIR}/run-fixture.cmake
    )
  endforeach()
endforeach()

foreach(_generator IN ITEMS Ninja "Unix Makefiles")
  string(REPLACE " " "_" _generator_id "${_generator}")
  add_test(
    NAME cmake-emit-llvmir-${_generator_id}-Debug-postfix
    COMMAND ${CMAKE_COMMAND}
      -DCASE_MODE=ON
      -DTEST_BUILD_TYPE=Debug
      -DFIXTURE_SOURCE=${_fixture_source}
      -DMODULE_FILE=${_module}
      -DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}/${_generator_id}-Debug-postfix\ build
      -DTEST_GENERATOR=${_generator}
      -P ${CMAKE_CURRENT_SOURCE_DIR}/run-fixture.cmake
  )
endforeach()

# This covers the public output-dir option and a repeated configuration in the
# same build tree, which must not lose generated LLVM IR targets.
add_test(
  NAME cmake-emit-llvmir-Ninja-custom-output-reconfigure
  COMMAND ${CMAKE_COMMAND}
    -DCASE_MODE=ON
    -DFIXTURE_SOURCE=${_fixture_source}
    -DMODULE_FILE=${_module}
    -DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}/Ninja-custom-output\ build
    -DTEST_OUTPUT_DIR=${CMAKE_CURRENT_BINARY_DIR}/Ninja-custom-output\ llvmir
    -DRECONFIGURE=ON
    -DTEST_GENERATOR=Ninja
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-fixture.cmake
)

add_test(
  NAME cmake-emit-llvmir-response-file-expanded
  COMMAND ${CMAKE_COMMAND}
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRLinkCommand.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/response-file-expanded
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-response-file.cmake
)
add_test(
  NAME cmake-emit-llvmir-response-file-missing-rejected
  COMMAND ${CMAKE_COMMAND}
    -DCASE_NAME=response-file-missing
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRLinkCommand.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/negative-response-file-missing
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-negative-case.cmake
)
add_test(
  NAME cmake-emit-llvmir-response-file-cycle-rejected
  COMMAND ${CMAKE_COMMAND}
    -DCASE_NAME=response-file-cycle
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRLinkCommand.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/negative-response-file-cycle
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-negative-case.cmake
)
add_test(
  NAME cmake-emit-llvmir-response-file-unterminated-quote-rejected
  COMMAND ${CMAKE_COMMAND}
    -DCASE_NAME=response-file-unterminated-quote
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRLinkCommand.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/negative-response-file-unterminated-quote
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-negative-case.cmake
)
add_test(
  NAME cmake-emit-llvmir-compile-db-ambiguity-rejected
  COMMAND ${CMAKE_COMMAND}
    -DCASE_NAME=compile-db-ambiguous
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRFromCompileDatabase.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/negative-compile-db
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-negative-case.cmake
)
add_test(
  NAME cmake-emit-llvmir-ninja-link-ambiguity-rejected
  COMMAND ${CMAKE_COMMAND}
    -DCASE_NAME=ninja-link-ambiguous
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRLinkCommand.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/negative-ninja-link
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-negative-case.cmake
)

add_test(
  NAME cmake-emit-llvmir-link-command-shared-library-soname
  COMMAND ${CMAKE_COMMAND}
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRLinkCommand.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/link-command-soname
    -DCMAKE_MAKE_PROGRAM=${CMAKE_MAKE_PROGRAM}
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-link-command-soname.cmake
)

add_test(
  NAME cmake-emit-llvmir-link-command-portability
  COMMAND ${CMAKE_COMMAND}
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRLinkCommand.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/link-command-portability
    -DCMAKE_MAKE_PROGRAM=${CMAKE_MAKE_PROGRAM}
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-link-command-portability.cmake
)