set(_fixture_source "${CMAKE_CURRENT_SOURCE_DIR}/fixtures/selection")
set(_module "${PROJECT_SOURCE_DIR}/cmake/CMakeEmitLLVMIR.cmake")
add_test(
  NAME cmake-emit-llvmir-project-command-wrappers
  COMMAND ${CMAKE_COMMAND}
    -DMODULE_FILE=${_module}
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/project-command-wrappers
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-project-command-wrappers.cmake
)
add_test(
  NAME cmake-emit-llvmir-ninja-relative-inputs
  COMMAND ${CMAKE_COMMAND}
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRLinkCommand.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/ninja-relative-inputs
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-ninja-relative-inputs.cmake
)
add_test(
  NAME cmake-emit-llvmir-offload-skip
  COMMAND ${CMAKE_COMMAND}
    -DMODULE_FILE=${_module}
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/offload-skip
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-offload-skip.cmake
)

add_test(
  NAME cmake-emit-llvmir-target-argument-forwarding
  COMMAND ${CMAKE_COMMAND}
    -DMODULE_FILE=${_module}
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/target-arguments
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-target-arguments.cmake
)
add_test(
  NAME cmake-emit-llvmir-no-language-project
  COMMAND ${CMAKE_COMMAND}
    -DMODULE_FILE=${_module}
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/no-language-project
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-no-language-project.cmake
)
add_test(
  NAME cmake-emit-llvmir-subproject-language
  COMMAND ${CMAKE_COMMAND}
    -DMODULE_FILE=${_module}
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/subproject-language
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-subproject-language.cmake
)
add_test(
  NAME cmake-emit-llvmir-source-cache-versions
  COMMAND bash ${CMAKE_CURRENT_SOURCE_DIR}/run-source-cache.sh
    ${PROJECT_SOURCE_DIR}/testdeb.sh
    ${CMAKE_CURRENT_BINARY_DIR}/source-cache
)

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
  NAME cmake-emit-llvmir-Ninja-build-program-name
  COMMAND ${CMAKE_COMMAND}
    -DCASE_MODE=ON
    -DFIXTURE_SOURCE=${_fixture_source}
    -DMODULE_FILE=${_module}
    -DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}/ninja-build-name
    -DTEST_GENERATOR=Ninja
    -DTEST_NINJA_BUILD_NAME=ON
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
  NAME cmake-emit-llvmir-compile-db-option-filter
  COMMAND ${CMAKE_COMMAND}
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRFromCompileDatabase.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/compile-db-option-filter
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-compile-db-option-filter.cmake
)
find_program(_test_clang NAMES clang-23 clang-22 clang REQUIRED)
find_program(_test_llvm_link NAMES llvm-link-23 llvm-link-22 llvm-link REQUIRED)
find_program(_test_llvm_nm NAMES llvm-nm-23 llvm-nm-22 llvm-nm REQUIRED)
find_program(_test_llvm_dis NAMES llvm-dis-23 llvm-dis-22 llvm-dis REQUIRED)
find_program(_test_llvm_ar_order NAMES llvm-ar-23 llvm-ar-22 llvm-ar REQUIRED)
add_test(
  NAME cmake-emit-llvmir-long-inputs
  COMMAND ${CMAKE_COMMAND}
    -DMODULE_FILE=${_module}
    -DCLANG=${_test_clang}
    -DLLVM_AR=${_test_llvm_ar_order}
    -DLLVM_LINK=${_test_llvm_link}
    -DLLVM_NM=${_test_llvm_nm}
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/long-inputs
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-long-inputs.cmake
)
add_test(
  NAME cmake-emit-llvmir-static-archive-order
  COMMAND ${CMAKE_COMMAND}
    -DCLANG=${_test_clang}
    -DLLVM_AR=${_test_llvm_ar_order}
    -DLLVM_DIS=${_test_llvm_dis}
    -DLLVM_LINK=${_test_llvm_link}
    -DLLVM_NM=${_test_llvm_nm}
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRMerge.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/static-archive-order
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-static-archive-order.cmake
)
add_test(
  NAME cmake-emit-llvmir-ninja-link-ambiguity-rejected
  COMMAND ${CMAKE_COMMAND}
    -DCASE_NAME=ninja-link-ambiguous
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRLinkCommand.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/negative-ninja-link
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-negative-case.cmake
)

find_program(_test_llvm_ar NAMES llvm-ar-23 llvm-ar-22 llvm-ar REQUIRED)

add_test(
  NAME cmake-emit-llvmir-ninja-static-archive
  COMMAND ${CMAKE_COMMAND}
    -DSCRIPT=${PROJECT_SOURCE_DIR}/cmake/EmitLLVMIRArchive.cmake
    -DWORK_DIR=${CMAKE_CURRENT_BINARY_DIR}/ninja-static-archive
    -DLLVM_AR=${_test_llvm_ar}
    -DNINJA=${CMAKE_MAKE_PROGRAM}
    -P ${CMAKE_CURRENT_SOURCE_DIR}/run-ninja-archive.cmake
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