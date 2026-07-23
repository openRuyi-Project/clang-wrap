if(NOT DEFINED LLVM_AR OR NOT DEFINED ARCHIVE_OUTPUT OR NOT DEFINED MISSING_FILE OR NOT DEFINED NATIVE_LINK_FILE OR
   NOT DEFINED TARGET_BINARY_DIR OR NOT DEFINED TARGET_NAME OR NOT DEFINED NINJA_BUILD_DIR OR
   NOT DEFINED CMAKE_MAKE_PROGRAM)
  message(FATAL_ERROR "LLVM_AR, ARCHIVE_OUTPUT, MISSING_FILE, NATIVE_LINK_FILE, TARGET_BINARY_DIR, TARGET_NAME, NINJA_BUILD_DIR, and CMAKE_MAKE_PROGRAM are required.")
endif()

if(EXISTS "${NATIVE_LINK_FILE}")
  file(READ "${NATIVE_LINK_FILE}" _native_command)
elseif(CMAKE_MAKE_PROGRAM MATCHES "ninja$")
  execute_process(
    COMMAND "${CMAKE_MAKE_PROGRAM}" -C "${NINJA_BUILD_DIR}" -t commands "${TARGET_NAME}"
    OUTPUT_VARIABLE _commands
    RESULT_VARIABLE _commands_result
    OUTPUT_STRIP_TRAILING_WHITESPACE
  )
  if(NOT _commands_result EQUAL 0)
    message(FATAL_ERROR "Cannot read Ninja archive command for ${TARGET_NAME}.")
  endif()
  string(REPLACE "\n" ";" _command_lines "${_commands}")
  foreach(_line IN LISTS _command_lines)
    if(_line MATCHES "(^|/)(llvm-)?ar( |$)")
      set(_native_command "${_line}")
      break()
    endif()
  endforeach()
  if(NOT _native_command)
    message(FATAL_ERROR "Cannot find Ninja archive command for ${TARGET_NAME}.")
  endif()
else()
  message(FATAL_ERROR "Static-library link command does not exist: ${NATIVE_LINK_FILE}")
endif()

separate_arguments(_arguments UNIX_COMMAND "${_native_command}")
include("${CMAKE_CURRENT_LIST_DIR}/ExpandResponseFile.cmake")
if(EXISTS "${NATIVE_LINK_FILE}")
  get_filename_component(_response_base_dir "${NATIVE_LINK_FILE}" DIRECTORY)
else()
  set(_response_base_dir "${TARGET_BINARY_DIR}")
endif()
cmake_emit_llvmir_expand_response_arguments(_arguments "${_response_base_dir}" ${_arguments})
string(REPLACE "," ";" _bitcode_members "${BC_FILES}")
set(_missing_members "")
foreach(_argument IN LISTS _arguments)
  if(_argument MATCHES "(^|.*/)CMakeFiles/[^/]+\\.dir/.*\\.(o|O)$" AND
     NOT _argument MATCHES "\\.(c|C|cc|cpp|cxx|c\\+\\+|CC|CPP|CXX)\\.(o|O)$")
    if(IS_ABSOLUTE "${_argument}")
      set(_assembly_object "${_argument}")
    else()
      set(_assembly_object "${TARGET_BINARY_DIR}/${_argument}")
    endif()
    if(NOT EXISTS "${_assembly_object}")
      message(FATAL_ERROR "Native object does not exist: ${_assembly_object}")
    endif()
    list(APPEND _missing_members "${_assembly_object}")
  endif()
endforeach()

list(REMOVE_DUPLICATES _bitcode_members)
if(NOT _bitcode_members)
  message(FATAL_ERROR "Cannot create ${ARCHIVE_OUTPUT}: no LLVM bitcode members found.")
endif()

list(REMOVE_DUPLICATES _missing_members)
if(_missing_members)
  string(JOIN "\n" _missing_members_content ${_missing_members})
  file(WRITE "${MISSING_FILE}" "${_missing_members_content}\n")
else()
  file(REMOVE "${MISSING_FILE}")
endif()
get_filename_component(_archive_dir "${ARCHIVE_OUTPUT}" DIRECTORY)
file(MAKE_DIRECTORY "${_archive_dir}")
execute_process(
  COMMAND "${LLVM_AR}" rcsD "${ARCHIVE_OUTPUT}" ${_bitcode_members}
  RESULT_VARIABLE _archive_result
  OUTPUT_VARIABLE _archive_output
  ERROR_VARIABLE _archive_error
)
if(NOT _archive_result EQUAL 0)
  message(FATAL_ERROR "LLVM archive creation failed:\n${_archive_output}\n${_archive_error}")
endif()