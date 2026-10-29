cmake_minimum_required(VERSION 3.24)
project(LLVMIRSelectionFixture LANGUAGES C CXX ASM)

file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/pymain.c" "This is a non-compiled CMake support file.\n")
add_custom_target(support_files SOURCES "${CMAKE_CURRENT_BINARY_DIR}/pymain.c")

add_executable(selected_app selected.c)
target_compile_definitions(selected_app PRIVATE FIXTURE_COMPILE_DATABASE=1)
set_source_files_properties(selected.c PROPERTIES COMPILE_OPTIONS "-DFIXTURE_PER_SOURCE_OPTION=1")
target_sources(selected_app PUBLIC FILE_SET fixture_headers TYPE HEADERS FILES fixture.h)

add_executable(unselected_app unselected.c)

add_library(object_sources OBJECT object_source.c)
add_executable(object_app object_main.c $<TARGET_OBJECTS:object_sources>)

# The same source is compiled by two object libraries with different options.
# Consumers must select the command of the object library they actually use.
add_library(object_variant_one OBJECT object_variant.c)
target_compile_definitions(object_variant_one PRIVATE OBJECT_VARIANT=1)
add_library(object_variant_two OBJECT object_variant.c)
target_compile_definitions(object_variant_two PRIVATE OBJECT_VARIANT=2)
add_executable(object_variant_one_app object_variant_main.c $<TARGET_OBJECTS:object_variant_one>)
add_executable(object_variant_two_app object_variant_main.c $<TARGET_OBJECTS:object_variant_two>)

add_library(same_stem STATIC same.c same.cpp)
set_target_properties(same_stem PROPERTIES OUTPUT_NAME same_stem_output PREFIX "" SUFFIX ".archive")

add_library(fixture_dep STATIC nested/dep.c)
add_library(fixture_dep_alias ALIAS fixture_dep)
add_executable(linked_app linked_main.c)
target_link_libraries(linked_app PRIVATE fixture_dep)

add_library(interface_dep INTERFACE)
target_link_libraries(interface_dep INTERFACE "$<BUILD_INTERFACE:fixture_dep_alias>")
add_executable(interface_linked_app linked_main.c)
target_link_libraries(interface_linked_app PRIVATE interface_dep)

add_executable(forward_linked_app linked_main.c)
target_link_libraries(forward_linked_app PRIVATE forward_dep)
add_library(forward_dep STATIC nested/dep.c)

add_library(versioned_shared SHARED versioned.c)
target_link_options(versioned_shared PRIVATE "-Wl,--version-script=${CMAKE_CURRENT_SOURCE_DIR}/exports.map")
set_target_properties(versioned_shared PROPERTIES VERSION 2.3.4 SOVERSION 2 DEBUG_POSTFIX _dbg)

add_library(xlinker_versioned_shared SHARED versioned.c)
target_link_options(xlinker_versioned_shared PRIVATE
  "-Xlinker" "--version-script" "-Xlinker" "${CMAKE_CURRENT_SOURCE_DIR}/exports.map")
set_target_properties(xlinker_versioned_shared PROPERTIES VERSION 3.0.0 SOVERSION 3)

add_library(static_shared_dependency STATIC static_shared_dependency.c)
target_link_libraries(static_shared_dependency PUBLIC versioned_shared)
add_executable(static_shared_consumer static_shared_consumer.c)
target_link_libraries(static_shared_consumer PRIVATE static_shared_dependency)

add_library(lib_prefixed_output SHARED versioned.c)
set_target_properties(lib_prefixed_output PROPERTIES OUTPUT_NAME libnamed VERSION 4.1 SOVERSION 4)

add_library(custom_suffix_shared SHARED versioned.c)
set_target_properties(custom_suffix_shared PROPERTIES PREFIX "" SUFFIX ".plugin" VERSION 7.2 SOVERSION 7)

add_library(zero_soversion_shared SHARED versioned.c)
set_target_properties(zero_soversion_shared PROPERTIES VERSION 0.8.6 SOVERSION 0)

add_library(asm_shared SHARED asm_shared.c asm_value.s)
set_target_properties(asm_shared PROPERTIES VERSION 1.0.0 SOVERSION 1)

add_library(asm_object_sources OBJECT asm_value.s)
add_library(asm_object_shared SHARED asm_shared.c $<TARGET_OBJECTS:asm_object_sources>)
set_target_properties(asm_object_shared PROPERTIES VERSION 1.0.0 SOVERSION 1)

add_library(asm_static STATIC asm_static.c asm_static_value.s)
add_executable(asm_static_app asm_static_main.c)
target_link_libraries(asm_static_app PRIVATE asm_static)

# Mirrors libuv test-idna.c: the executable embeds one implementation that is
# also present in its static dependency, while still requiring another member.
add_library(embedded_static STATIC embedded_duplicate.c embedded_dependency.c)
add_executable(embedded_static_app embedded_static_main.c)
target_link_libraries(embedded_static_app PRIVATE embedded_static)

# The member is deliberately unreferenced by the shared library. Native
# --whole-archive linking still exports it, so LLVM IR must retain it too.
add_library(whole_archive_static STATIC whole_archive_member.c)
add_library(whole_archive_shared SHARED whole_archive_shared.c)
target_link_libraries(whole_archive_shared PRIVATE
  "-Wl,--whole-archive" whole_archive_static "-Wl,--no-whole-archive")
set_target_properties(whole_archive_shared PROPERTIES VERSION 1.0.0 SOVERSION 1)

# This archive is intentionally not a CMake target. It models a generated or
# third-party native archive that must be carried into the installed asm/ tree.
add_custom_command(
  OUTPUT external_native_member.o
  COMMAND ${CMAKE_C_COMPILER} -fPIC -c "${CMAKE_CURRENT_SOURCE_DIR}/external_native_member.c"
          -o external_native_member.o
  DEPENDS external_native_member.c
)
add_custom_command(
  OUTPUT libexternal_native.a
  COMMAND ${CMAKE_AR} qc libexternal_native.a external_native_member.o
  DEPENDS external_native_member.o
)
add_custom_target(external_native_archive DEPENDS libexternal_native.a)
add_library(external_archive_shared SHARED external_archive_shared.c)
add_dependencies(external_archive_shared external_native_archive)
target_link_libraries(external_archive_shared PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/libexternal_native.a")
set_target_properties(external_archive_shared PROPERTIES VERSION 1.0.0 SOVERSION 1)

add_executable(debug_postfix_app debug_postfix.c)
set_target_properties(debug_postfix_app PROPERTIES DEBUG_POSTFIX _dbg)

add_executable(fallback_app debug_postfix.c)

add_executable(nested_bin_app debug_postfix.c)
add_executable(sbin_app debug_postfix.c)
add_executable(libexec_app debug_postfix.c)

add_custom_command(
  OUTPUT generated_source.c
  COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CMAKE_CURRENT_SOURCE_DIR}/generated_source.in.c" generated_source.c
  DEPENDS generated_source.in.c
)
add_executable(generated_source_app generated_source.c)

# Mirror projects such as curl that ship a pre-generated fallback with the
# same name as a custom-command output compiled from the binary directory.
add_custom_command(
  OUTPUT generated_shadow.c
  COMMAND ${CMAKE_COMMAND} -E copy_if_different "${CMAKE_CURRENT_SOURCE_DIR}/generated_source.in.c" generated_shadow.c
  DEPENDS generated_source.in.c
)
add_executable(generated_shadow_app generated_shadow.c)

add_library(cycle_a STATIC cycle_a.c)
add_library(cycle_b STATIC cycle_b.c)
target_link_libraries(cycle_a PUBLIC cycle_b)
target_link_libraries(cycle_b PUBLIC cycle_a)
add_executable(cycle_app cycle_main.c)
target_link_libraries(cycle_app PRIVATE cycle_a)

install(TARGETS selected_app object_app object_variant_one_app object_variant_two_app linked_app interface_linked_app forward_linked_app same_stem versioned_shared xlinker_versioned_shared static_shared_dependency static_shared_consumer lib_prefixed_output custom_suffix_shared zero_soversion_shared asm_shared asm_object_shared asm_static asm_static_app embedded_static embedded_static_app whole_archive_static whole_archive_shared external_archive_shared fixture_dep forward_dep debug_postfix_app generated_source_app generated_shadow_app cycle_app
  EXPORT LLVMIRFixtureTargets
  FILE_SET fixture_headers DESTINATION include
  RUNTIME DESTINATION bin
  ARCHIVE DESTINATION lib
)
install(TARGETS fallback_app DESTINATION bin)
install(TARGETS nested_bin_app RUNTIME DESTINATION lib/asoftware/bin)
install(TARGETS sbin_app RUNTIME DESTINATION sbin)
install(TARGETS libexec_app RUNTIME DESTINATION libexec)

foreach(_module_layout IN ITEMS share lib lib/plugin lib/plugin/nested)
  string(REPLACE "/" "_" _module_id "${_module_layout}")
  add_library(module_${_module_id} MODULE versioned.c)
  set_target_properties(module_${_module_id} PROPERTIES PREFIX "")
  install(TARGETS module_${_module_id} LIBRARY DESTINATION "${_module_layout}")
endforeach()

add_subdirectory(nested)