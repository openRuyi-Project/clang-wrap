if(NOT DEFINED CASE_MODE OR NOT DEFINED FIXTURE_SOURCE OR NOT DEFINED MODULE_FILE OR NOT DEFINED TEST_BINARY_DIR OR NOT DEFINED TEST_GENERATOR)
  message(FATAL_ERROR "CASE_MODE, FIXTURE_SOURCE, MODULE_FILE, TEST_BINARY_DIR, and TEST_GENERATOR are required.")
endif()

file(REMOVE_RECURSE "${TEST_BINARY_DIR}")
set(_configure_args
  -DCMAKE_C_COMPILER=clang
  -DCMAKE_CXX_COMPILER=clang++
  -DCMAKE_PROJECT_TOP_LEVEL_INCLUDES=${MODULE_FILE}
)
if(DEFINED TEST_OUTPUT_DIR)
  list(APPEND _configure_args "-DCMAKE_EMIT_LLVMIR_OUTPUT_DIR=${TEST_OUTPUT_DIR}")
else()
  set(TEST_OUTPUT_DIR "${TEST_BINARY_DIR}/llvmir")
endif()
if(DEFINED TEST_BUILD_TYPE)
  list(APPEND _configure_args "-DCMAKE_BUILD_TYPE=${TEST_BUILD_TYPE}")
endif()

set(_configure_count 1)
if(RECONFIGURE)
  set(_configure_count 2)
endif()
foreach(_pass RANGE 1 ${_configure_count})
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -E env CMAKE_EMIT_LLVMIR=${CASE_MODE}
      "${CMAKE_COMMAND}" -S "${FIXTURE_SOURCE}" -B "${TEST_BINARY_DIR}" -G "${TEST_GENERATOR}"
      ${_configure_args}
    RESULT_VARIABLE _configure_result
    OUTPUT_VARIABLE _configure_output
    ERROR_VARIABLE _configure_error
  )
  if(NOT _configure_result EQUAL 0)
    message(FATAL_ERROR "Fixture configuration pass ${_pass} failed:\n${_configure_output}\n${_configure_error}")
  endif()
endforeach()

execute_process(
  COMMAND "${CMAKE_COMMAND}" --build "${TEST_BINARY_DIR}"
  RESULT_VARIABLE _build_result
  OUTPUT_VARIABLE _build_output
  ERROR_VARIABLE _build_error
)
if(NOT _build_result EQUAL 0)
  message(FATAL_ERROR "Fixture build failed:\n${_build_output}\n${_build_error}")
endif()

set(_selected_bc "${TEST_OUTPUT_DIR}/selected_app.bc")
set(_unselected_bc "${TEST_OUTPUT_DIR}/unselected_app.bc")
set(_nested_bc "${TEST_OUTPUT_DIR}/nested/nested_cpp.bc")
set(_object_bc "${TEST_OUTPUT_DIR}/object_app.bc")
set(_same_stem_archive "${TEST_OUTPUT_DIR}/same_stem_output.archive")
set(_linked_bc "${TEST_OUTPUT_DIR}/linked_app.bc")
set(_interface_linked_bc "${TEST_OUTPUT_DIR}/interface_linked_app.bc")
set(_forward_linked_bc "${TEST_OUTPUT_DIR}/forward_linked_app.bc")
set(_generated_source_bc "${TEST_OUTPUT_DIR}/generated_source_app.bc")
set(_cycle_bc "${TEST_OUTPUT_DIR}/cycle_app.bc")
set(_lib_prefixed_bc "${TEST_OUTPUT_DIR}/liblibnamed.so.4.bc")
set(_custom_suffix_bc "${TEST_OUTPUT_DIR}/custom_suffix_shared.plugin.7.bc")
set(_zero_soversion_bc "${TEST_OUTPUT_DIR}/libzero_soversion_shared.so.0.bc")
set(_whole_archive_bc "${TEST_OUTPUT_DIR}/libwhole_archive_shared.so.1.bc")
set(_external_archive_bc "${TEST_OUTPUT_DIR}/libexternal_archive_shared.so.1.bc")
set(_debug_postfix_bc "${TEST_OUTPUT_DIR}/debug_postfix_app_dbg.bc")
set(_versioned_debug_bc "${TEST_OUTPUT_DIR}/libversioned_shared_dbg.so.2.bc")
set(_versioned_script_name "libversioned_shared.so.2_cmd")
set(_versioned_shared_soname "libversioned_shared.so.2")
if(TEST_BUILD_TYPE STREQUAL "Debug")
  set(_versioned_script_name "libversioned_shared_dbg.so.2_cmd")
  set(_versioned_shared_soname "libversioned_shared_dbg.so.2")
endif()
if(CASE_MODE STREQUAL "ON")
  if(NOT EXISTS "${_selected_bc}" OR NOT EXISTS "${_unselected_bc}" OR NOT EXISTS "${_nested_bc}" OR
     NOT EXISTS "${_object_bc}" OR NOT EXISTS "${_same_stem_archive}" OR NOT EXISTS "${_linked_bc}" OR
     NOT EXISTS "${_interface_linked_bc}" OR NOT EXISTS "${_forward_linked_bc}" OR NOT EXISTS "${_generated_source_bc}" OR NOT EXISTS "${_cycle_bc}" OR
     NOT EXISTS "${_lib_prefixed_bc}" OR NOT EXISTS "${_custom_suffix_bc}" OR NOT EXISTS "${_zero_soversion_bc}" OR NOT EXISTS "${_whole_archive_bc}" OR NOT EXISTS "${_external_archive_bc}")
    message(FATAL_ERROR "Enabled mode did not generate all expected fixture artifacts.")
  endif()
  if(TEST_BUILD_TYPE STREQUAL "Debug")
    if(NOT EXISTS "${_debug_postfix_bc}" OR NOT EXISTS "${_versioned_debug_bc}" OR
       EXISTS "${TEST_OUTPUT_DIR}/debug_postfix_app.bc" OR EXISTS "${TEST_OUTPUT_DIR}/libversioned_shared.so.2.bc")
      message(FATAL_ERROR "Debug postfix artifacts do not match the native target names.")
    endif()
  endif()
  file(GLOB _same_stem_objects "${TEST_OUTPUT_DIR}/objects/same_stem_same_*\.bc")
  list(LENGTH _same_stem_objects _same_stem_object_count)
  if(NOT _same_stem_object_count EQUAL 2)
    message(FATAL_ERROR "Same-stem C and C++ sources collided; expected two object bitcode files, got ${_same_stem_object_count}.")
  endif()

  set(_install_prefix "${TEST_BINARY_DIR}/install")
  execute_process(
    COMMAND "${CMAKE_COMMAND}" --install "${TEST_BINARY_DIR}" --prefix "${_install_prefix}"
    RESULT_VARIABLE _native_install_result
    OUTPUT_VARIABLE _native_install_output
    ERROR_VARIABLE _native_install_error
  )
  if(NOT _native_install_result EQUAL 0)
    message(FATAL_ERROR "Native installation failed:\n${_native_install_output}\n${_native_install_error}")
  endif()
  execute_process(
    COMMAND "${CMAKE_COMMAND}" --install "${TEST_BINARY_DIR}" --prefix "${_install_prefix}" --component LLVMIR
    RESULT_VARIABLE _install_result
    OUTPUT_VARIABLE _install_output
    ERROR_VARIABLE _install_error
  )
  if(NOT _install_result EQUAL 0)
    message(FATAL_ERROR "LLVM IR component installation failed:\n${_install_output}\n${_install_error}")
  endif()
  if(NOT EXISTS "${_install_prefix}/lib/llvmir-bin/selected_app.bc" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-bin/selected_app_cmd" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-bin/object_app_cmd" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-bin/object_variant_one_app.bc" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-bin/object_variant_two_app.bc" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-bin/static_shared_consumer_cmd" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-bin/embedded_static_app_cmd" OR
     NOT EXISTS "${_install_prefix}/lib/asoftware/lib/llvmir-bin/nested_bin_app.bc" OR
     NOT EXISTS "${_install_prefix}/lib/asoftware/lib/llvmir-bin/nested_bin_app_cmd" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-sbin/sbin_app.bc" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-sbin/sbin_app_cmd" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-libexec/libexec_app.bc" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-libexec/libexec_app_cmd" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/same_stem_output.archive" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/libasm_shared.so.1.bc" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/libasm_shared.so.1_cmd" OR
     NOT EXISTS "${_install_prefix}/lib/llvmir/libasm_object_shared.so.1.bc" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/libasm_object_shared.so.1_cmd" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/libwhole_archive_shared.so.1.bc" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/libwhole_archive_shared.so.1_cmd" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/libexternal_archive_shared.so.1.bc" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/libexternal_archive_shared.so.1_cmd" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/libxlinker_versioned_shared.so.3_verscript" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/libasm_static.a" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir/libembedded_static.a" OR
      NOT EXISTS "${_install_prefix}/lib/llvmir-bin/asm_static_app_cmd")
    message(FATAL_ERROR "LLVM IR component installation omitted expected artifacts.")
  endif()
  file(READ "${_install_prefix}/lib/llvmir/libasm_shared.so.1_cmd" _asm_shared_script)
  if(NOT _asm_shared_script MATCHES "\\./asm/[0-9a-f]+-asm_value\\.s\\.o" OR
     NOT _asm_shared_script MATCHES "--output=output/libasm_shared\\.so\\.1")
    message(FATAL_ERROR "Shared-library relink script does not reference the installed assembly object.")
  endif()
  file(READ "${_install_prefix}/lib/llvmir/libasm_object_shared.so.1_cmd" _asm_object_shared_script)
  if(NOT _asm_object_shared_script MATCHES "\\./asm/[0-9a-f]+-asm_value\\.s\\.o" OR
     NOT _asm_object_shared_script MATCHES "--output=output/libasm_object_shared\\.so\\.1")
    message(FATAL_ERROR "OBJECT-library shared relink script does not reference the installed assembly object.")
  endif()
  execute_process(
    COMMAND bash ./nested_bin_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/asoftware/lib/llvmir-bin"
    RESULT_VARIABLE _nested_bin_relink_result
    ERROR_VARIABLE _nested_bin_relink_error
  )
  if(NOT _nested_bin_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/asoftware/lib/llvmir-bin/output/nested_bin_app")
    message(FATAL_ERROR "Nested bin executable relink failed:\n${_nested_bin_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./sbin_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-sbin"
    RESULT_VARIABLE _sbin_relink_result
    ERROR_VARIABLE _sbin_relink_error
  )
  if(NOT _sbin_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-sbin/output/sbin_app")
    message(FATAL_ERROR "sbin executable relink failed:\n${_sbin_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./libexec_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-libexec"
    RESULT_VARIABLE _libexec_relink_result
    ERROR_VARIABLE _libexec_relink_error
  )
  if(NOT _libexec_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-libexec/output/libexec_app")
    message(FATAL_ERROR "libexec executable relink failed:\n${_libexec_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./libwhole_archive_shared.so.1_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir"
    RESULT_VARIABLE _whole_archive_relink_result
    ERROR_VARIABLE _whole_archive_relink_error
  )
  if(NOT _whole_archive_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir/output/libwhole_archive_shared.so.1")
    message(FATAL_ERROR "Whole-archive shared-library relink failed:\n${_whole_archive_relink_error}")
  endif()
  execute_process(
    COMMAND nm -D "${_install_prefix}/lib/llvmir/output/libwhole_archive_shared.so.1"
    OUTPUT_VARIABLE _whole_archive_exports
    RESULT_VARIABLE _whole_archive_nm_result
  )
  if(NOT _whole_archive_nm_result EQUAL 0 OR NOT _whole_archive_exports MATCHES "whole_archive_public")
    message(FATAL_ERROR "Whole-archive member was not exported by the LLVM IR relink.")
  endif()
  file(READ "${_install_prefix}/lib/llvmir/libexternal_archive_shared.so.1_cmd" _external_archive_script)
  if(NOT _external_archive_script MATCHES "\\./asm/[0-9a-f]+-libexternal_native\\.a" OR
     _external_archive_script MATCHES "${TEST_BINARY_DIR}")
    message(FATAL_ERROR "External native archive was not relocated into asm/.")
  endif()
  execute_process(
    COMMAND bash ./libexternal_archive_shared.so.1_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir"
    RESULT_VARIABLE _external_archive_relink_result
    ERROR_VARIABLE _external_archive_relink_error
  )
  if(NOT _external_archive_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir/output/libexternal_archive_shared.so.1")
    message(FATAL_ERROR "External native archive relink failed:\n${_external_archive_relink_error}")
  endif()
  file(READ "${_install_prefix}/lib/llvmir/libxlinker_versioned_shared.so.3_cmd" _xlinker_versioned_script)
  if(NOT _xlinker_versioned_script MATCHES "-Wl,--version-script=\\./libxlinker_versioned_shared\\.so\\.3_verscript" OR
     _xlinker_versioned_script MATCHES "-Xlinker")
    message(FATAL_ERROR "Xlinker version-script option was not normalized in the installed relink command.")
  endif()
  execute_process(
    COMMAND bash ./libxlinker_versioned_shared.so.3_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir"
    RESULT_VARIABLE _xlinker_versioned_relink_result
    ERROR_VARIABLE _xlinker_versioned_relink_error
  )
  if(NOT _xlinker_versioned_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir/output/libxlinker_versioned_shared.so.3")
    message(FATAL_ERROR "Xlinker version-script relink failed:\n${_xlinker_versioned_relink_error}")
  endif()
  if(EXISTS "${_install_prefix}/lib/llvmir/libzero_soversion_shared.so.0_verscript")
    message(FATAL_ERROR "A target without a version-script option installed an unexpected verscript file.")
  endif()
  execute_process(
    COMMAND bash ./libasm_object_shared.so.1_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir"
    RESULT_VARIABLE _asm_object_shared_relink_result
    OUTPUT_VARIABLE _asm_object_shared_relink_output
    ERROR_VARIABLE _asm_object_shared_relink_error
  )
  if(NOT _asm_object_shared_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir/output/libasm_object_shared.so.1")
    message(FATAL_ERROR "OBJECT-library assembly shared relink failed:\n${_asm_object_shared_relink_output}\n${_asm_object_shared_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./libasm_shared.so.1_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir"
    RESULT_VARIABLE _asm_shared_relink_result
    OUTPUT_VARIABLE _asm_shared_relink_output
    ERROR_VARIABLE _asm_shared_relink_error
  )
  if(NOT _asm_shared_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir/output/libasm_shared.so.1")
    message(FATAL_ERROR "Assembly shared-library relink failed:\n${_asm_shared_relink_output}\n${_asm_shared_relink_error}")
  endif()
  file(READ "${_install_prefix}/lib/llvmir-bin/asm_static_app_cmd" _asm_static_script)
  if(_asm_static_script MATCHES "libasm_static\\.a" OR
     NOT _asm_static_script MATCHES "\\./asm/" OR
     NOT _asm_static_script MATCHES "-fuse-ld=lld" OR
     NOT _asm_static_script MATCHES "# Clang version: ")
    message(FATAL_ERROR "Static-library relink script still depends on an LLVM archive instead of installed asm fallback.")
  endif()
  execute_process(
    COMMAND bash ./asm_static_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
    RESULT_VARIABLE _asm_static_relink_result
    OUTPUT_VARIABLE _asm_static_relink_output
    ERROR_VARIABLE _asm_static_relink_error
  )
  if(NOT _asm_static_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/asm_static_app")
    message(FATAL_ERROR "Static-library assembly relink failed:\n${_asm_static_relink_output}\n${_asm_static_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./embedded_static_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
    RESULT_VARIABLE _embedded_static_relink_result
    ERROR_VARIABLE _embedded_static_relink_error
  )
  if(NOT _embedded_static_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/embedded_static_app")
    message(FATAL_ERROR "Embedded static-library member relink failed:\n${_embedded_static_relink_error}")
  endif()
  if(TEST_BUILD_TYPE STREQUAL "Debug")
    execute_process(
      COMMAND bash ./debug_postfix_app_dbg_cmd
      WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
      RESULT_VARIABLE _debug_relink_result
      ERROR_VARIABLE _debug_relink_error
    )
    if(NOT _debug_relink_result EQUAL 0 OR NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/debug_postfix_app_dbg")
      message(FATAL_ERROR "Debug postfix executable relink failed:\n${_debug_relink_error}")
    endif()
    execute_process(
      COMMAND bash ./libversioned_shared_dbg.so.2_cmd
      WORKING_DIRECTORY "${_install_prefix}/lib/llvmir"
      RESULT_VARIABLE _debug_shared_relink_result
      ERROR_VARIABLE _debug_shared_relink_error
    )
    if(NOT _debug_shared_relink_result EQUAL 0 OR NOT EXISTS "${_install_prefix}/lib/llvmir/output/libversioned_shared_dbg.so.2")
      message(FATAL_ERROR "Debug postfix shared-library relink failed:\n${_debug_shared_relink_error}")
    endif()
  endif()
  execute_process(
    COMMAND bash ./object_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
    RESULT_VARIABLE _relink_result
    OUTPUT_VARIABLE _relink_output
    ERROR_VARIABLE _relink_error
  )
  if(NOT _relink_result EQUAL 0 OR NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/object_app")
    message(FATAL_ERROR "Object-library relink script failed:\n${_relink_output}\n${_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./linked_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
    RESULT_VARIABLE _linked_relink_result
    ERROR_VARIABLE _linked_relink_error
  )
  if(NOT _linked_relink_result EQUAL 0 OR NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/linked_app")
    message(FATAL_ERROR "Static-library relink script failed:\n${_linked_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./interface_linked_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
    RESULT_VARIABLE _interface_linked_relink_result
    ERROR_VARIABLE _interface_linked_relink_error
  )
  if(NOT _interface_linked_relink_result EQUAL 0 OR NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/interface_linked_app")
    message(FATAL_ERROR "INTERFACE static-library relink script failed:\n${_interface_linked_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./forward_linked_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
    RESULT_VARIABLE _forward_linked_relink_result
    ERROR_VARIABLE _forward_linked_relink_error
  )
  if(NOT _forward_linked_relink_result EQUAL 0 OR NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/forward_linked_app")
    message(FATAL_ERROR "Forward-declared static-library relink script failed:\n${_forward_linked_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./static_shared_consumer_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
    RESULT_VARIABLE _static_shared_relink_result
    ERROR_VARIABLE _static_shared_relink_error
  )
  if(NOT _static_shared_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/static_shared_consumer")
    message(FATAL_ERROR "Static-library transitive shared-library relink failed:\n${_static_shared_relink_error}")
  endif()
  file(READ "${_install_prefix}/lib/llvmir-bin/static_shared_consumer_cmd" _static_shared_consumer_script)
  string(REPLACE "." "\\." _versioned_shared_soname_pattern "${_versioned_shared_soname}")
  if(NOT _static_shared_consumer_script MATCHES "'-l:${_versioned_shared_soname_pattern}'" OR
     _static_shared_consumer_script MATCHES "\\.\\./libversioned_shared")
    message(FATAL_ERROR "Project shared-library dependency was not normalized to -l:SONAME:\n${_static_shared_consumer_script}")
  endif()
  file(READ "${_install_prefix}/lib/llvmir-bin/cycle_app_cmd" _cycle_script)
  if(_cycle_script MATCHES "libcycle_a\\.a" OR
     _cycle_script MATCHES "libcycle_b\\.a" OR
     _cycle_script MATCHES "${TEST_BINARY_DIR}")
    message(FATAL_ERROR "Multi-static-library relink script retained an installed LLVM archive or build-tree path.")
  endif()
  execute_process(
    COMMAND bash ./cycle_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
    RESULT_VARIABLE _cycle_relink_result
    ERROR_VARIABLE _cycle_relink_error
  )
  if(NOT _cycle_relink_result EQUAL 0 OR NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/cycle_app")
    message(FATAL_ERROR "Multi-static-library relink script failed:\n${_cycle_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./generated_source_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
    RESULT_VARIABLE _generated_relink_result
    ERROR_VARIABLE _generated_relink_error
  )
  if(NOT _generated_relink_result EQUAL 0 OR NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/generated_source_app")
    message(FATAL_ERROR "Generated-source relink script failed:\n${_generated_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./generated_shadow_app_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir-bin"
    RESULT_VARIABLE _generated_shadow_relink_result
    ERROR_VARIABLE _generated_shadow_relink_error
  )
  if(NOT _generated_shadow_relink_result EQUAL 0 OR NOT EXISTS "${_install_prefix}/lib/llvmir-bin/output/generated_shadow_app")
    message(FATAL_ERROR "Shadowed generated-source relink script failed:\n${_generated_shadow_relink_error}")
  endif()
  execute_process(
    COMMAND bash "./${_versioned_script_name}"
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir"
    RESULT_VARIABLE _versioned_relink_result
    ERROR_VARIABLE _versioned_relink_error
  )
  if(NOT _versioned_relink_result EQUAL 0)
    message(FATAL_ERROR "Version-script relink script failed:\n${_versioned_relink_error}")
  endif()
  execute_process(
    COMMAND bash ./libzero_soversion_shared.so.0_cmd
    WORKING_DIRECTORY "${_install_prefix}/lib/llvmir"
    RESULT_VARIABLE _zero_soversion_relink_result
    ERROR_VARIABLE _zero_soversion_relink_error
  )
  if(NOT _zero_soversion_relink_result EQUAL 0 OR
     NOT EXISTS "${_install_prefix}/lib/llvmir/output/libzero_soversion_shared.so.0" OR
     EXISTS "${_install_prefix}/lib/llvmir/output/libzero_soversion_shared.so.0.8.6")
    message(FATAL_ERROR "SOVERSION 0 shared-library relink failed:\n${_zero_soversion_relink_error}")
  endif()
  file(READ "${_install_prefix}/lib/llvmir/${_versioned_script_name}" _versioned_script)
  if(_versioned_script MATCHES "${FIXTURE_SOURCE}/exports\\.map")
    message(FATAL_ERROR "Installed version-script relink command retained a source-tree path.")
  endif()
else()
  if(EXISTS "${_selected_bc}" OR EXISTS "${_unselected_bc}" OR EXISTS "${_nested_bc}" OR
     EXISTS "${_object_bc}" OR EXISTS "${_same_stem_archive}" OR EXISTS "${_linked_bc}" OR
     EXISTS "${_interface_linked_bc}" OR EXISTS "${_forward_linked_bc}" OR EXISTS "${_generated_source_bc}" OR EXISTS "${_cycle_bc}" OR
     EXISTS "${_lib_prefixed_bc}" OR EXISTS "${_custom_suffix_bc}" OR EXISTS "${_whole_archive_bc}" OR EXISTS "${_external_archive_bc}")
    message(FATAL_ERROR "Disabled mode unexpectedly generated bitcode.")
  endif()
endif()