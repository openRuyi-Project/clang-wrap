if(NOT DEFINED LLVM_LINK OR NOT DEFINED OUTPUT)
  message(FATAL_ERROR "LLVM_LINK and OUTPUT are required.")
endif()

if(NOT DEFINED LLVM_NM OR LLVM_NM STREQUAL "")
  get_filename_component(_llvm_bin_dir "${LLVM_LINK}" DIRECTORY)
  find_program(LLVM_NM NAMES llvm-nm llvm-nm-23 HINTS "${_llvm_bin_dir}" NO_DEFAULT_PATH)
endif()
if(NOT DEFINED LLVM_AR OR LLVM_AR STREQUAL "")
  get_filename_component(_llvm_bin_dir "${LLVM_LINK}" DIRECTORY)
  find_program(LLVM_AR NAMES llvm-ar llvm-ar-23 HINTS "${_llvm_bin_dir}" NO_DEFAULT_PATH)
endif()
if(NOT LLVM_NM OR NOT LLVM_AR)
  message(FATAL_ERROR "Could not locate llvm-nm and llvm-ar next to LLVM_LINK.")
endif()

if(DEFINED INPUTS_FILE)
  include("${INPUTS_FILE}")
else()
  string(REPLACE "," ";" _direct_bc_files "${DIRECT_BC_FILES}")
  string(REPLACE "," ";" _static_bc_files "${STATIC_BC_FILES}")
  string(REPLACE "," ";" _whole_archive_bc_files "${WHOLE_ARCHIVE_BC_FILES}")
endif()
if(NOT _direct_bc_files)
  message(FATAL_ERROR "Cannot merge LLVM IR without direct bitcode inputs.")
endif()

get_filename_component(_output_dir "${OUTPUT}" DIRECTORY)
file(MAKE_DIRECTORY "${_output_dir}")
set(_member_dir "${OUTPUT}.archive-members")
file(REMOVE_RECURSE "${_member_dir}")
file(MAKE_DIRECTORY "${_member_dir}")

function(_read_symbols input defined_var undefined_var)
  execute_process(
    COMMAND "${LLVM_NM}" --defined-only --extern-only --format=posix "${input}"
    RESULT_VARIABLE _nm_result
    OUTPUT_VARIABLE _defined_output
    ERROR_VARIABLE _nm_error
  )
  if(NOT _nm_result EQUAL 0)
    message(FATAL_ERROR "LLVM symbol inspection failed for ${input}: ${_nm_error}")
  endif()
  execute_process(
    COMMAND "${LLVM_NM}" --undefined-only --extern-only --format=posix "${input}"
    RESULT_VARIABLE _nm_result
    OUTPUT_VARIABLE _undefined_output
    ERROR_VARIABLE _nm_error
  )
  if(NOT _nm_result EQUAL 0)
    message(FATAL_ERROR "LLVM undefined-symbol inspection failed for ${input}: ${_nm_error}")
  endif()
  set(_defined "")
  set(_undefined "")
  string(REPLACE "\n" ";" _lines "${_defined_output}")
  foreach(_line IN LISTS _lines)
    if(_line MATCHES "^([^; \t]+)[ \t]+([^; \t]+)")
      list(APPEND _defined "${CMAKE_MATCH_1}")
    endif()
  endforeach()
  string(REPLACE "\n" ";" _lines "${_undefined_output}")
  foreach(_line IN LISTS _lines)
    if(_line MATCHES "^([^; \t]+)[ \t]+([^; \t]+)")
      list(APPEND _undefined "${CMAKE_MATCH_1}")
    endif()
  endforeach()
  list(REMOVE_DUPLICATES _defined)
  list(REMOVE_DUPLICATES _undefined)
  set(${defined_var} "${_defined}" PARENT_SCOPE)
  set(${undefined_var} "${_undefined}" PARENT_SCOPE)
endfunction()

function(_add_symbols input defined_var undefined_var)
  _read_symbols("${input}" _module_defined _module_undefined)
  set(_defined "${${defined_var}}")
  set(_undefined "${${undefined_var}}")
  foreach(_symbol IN LISTS _module_defined)
    list(REMOVE_ITEM _undefined "${_symbol}")
    list(APPEND _defined "${_symbol}")
  endforeach()
  foreach(_symbol IN LISTS _module_undefined)
    if(NOT _symbol IN_LIST _defined)
      list(APPEND _undefined "${_symbol}")
    endif()
  endforeach()
  list(REMOVE_DUPLICATES _defined)
  list(REMOVE_DUPLICATES _undefined)
  set(${defined_var} "${_defined}" PARENT_SCOPE)
  set(${undefined_var} "${_undefined}" PARENT_SCOPE)
endfunction()

function(_extract_archive archive directory members_var)
  file(MAKE_DIRECTORY "${directory}")
  execute_process(
    COMMAND "${LLVM_AR}" x "${archive}"
    WORKING_DIRECTORY "${directory}"
    RESULT_VARIABLE _extract_result
    ERROR_VARIABLE _extract_error
  )
  if(NOT _extract_result EQUAL 0)
    message(FATAL_ERROR "LLVM archive extraction failed for ${archive}: ${_extract_error}")
  endif()
  file(GLOB _members LIST_DIRECTORIES false "${directory}/*")
  set(${members_var} "${_members}" PARENT_SCOPE)
endfunction()

set(_defined_symbols "")
set(_undefined_symbols "")
foreach(_input IN LISTS _direct_bc_files)
  _add_symbols("${_input}" _defined_symbols _undefined_symbols)
endforeach()

set(_member_index 0)
set(_selected_members "")
foreach(_archive IN LISTS _static_bc_files)
  _extract_archive("${_archive}" "${_member_dir}/archive-${_member_index}" _archive_members)
  math(EXPR _member_index "${_member_index} + 1")

  # Static linkers process archives in command-line order. Within one archive,
  # keep extracting members until its unresolved references reach a fixed
  # point, then move on; never revisit an earlier archive implicitly.
  set(_archive_changed TRUE)
  while(_archive_changed)
    set(_archive_changed FALSE)
    foreach(_member IN LISTS _archive_members)
      if(_member IN_LIST _selected_members)
        continue()
      endif()
      _read_symbols("${_member}" _member_defined _member_undefined)
      set(_provides_unresolved FALSE)
      foreach(_symbol IN LISTS _member_defined)
        if(_symbol IN_LIST _undefined_symbols)
          set(_provides_unresolved TRUE)
          break()
        endif()
      endforeach()
      if(_provides_unresolved)
        _add_symbols("${_member}" _defined_symbols _undefined_symbols)
        list(APPEND _selected_members "${_member}")
        set(_archive_changed TRUE)
      endif()
    endforeach()
  endwhile()
endforeach()

set(_whole_members "")
set(_whole_index 0)
foreach(_archive IN LISTS _whole_archive_bc_files)
  if(_archive MATCHES "\\.a$")
    _extract_archive("${_archive}" "${_member_dir}/whole-${_whole_index}" _archive_members)
    list(APPEND _whole_members ${_archive_members})
  else()
    list(APPEND _whole_members "${_archive}")
  endif()
  math(EXPR _whole_index "${_whole_index} + 1")
endforeach()

# LLVM tools accept quoted response-file arguments on every supported host.
set(_response_content "")
foreach(_input IN LISTS _direct_bc_files _selected_members _whole_members)
  string(REPLACE "\\" "\\\\" _quoted_input "${_input}")
  string(REPLACE "\"" "\\\"" _quoted_input "${_quoted_input}")
  string(APPEND _response_content "\"${_quoted_input}\"\n")
endforeach()
set(_response_file "${_member_dir}/inputs.rsp")
file(WRITE "${_response_file}" "${_response_content}")
execute_process(
  COMMAND "${LLVM_LINK}" "@${_response_file}" -o "${OUTPUT}"
  RESULT_VARIABLE _result
  OUTPUT_VARIABLE _output
  ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "LLVM bitcode merge failed:\n${_output}\n${_error}")
endif()
file(REMOVE_RECURSE "${_member_dir}")