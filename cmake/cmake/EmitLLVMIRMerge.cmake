if(NOT DEFINED LLVM_LINK OR NOT DEFINED OUTPUT)
  message(FATAL_ERROR "LLVM_LINK and OUTPUT are required.")
endif()

string(REPLACE "," ";" _direct_bc_files "${DIRECT_BC_FILES}")
string(REPLACE "," ";" _static_bc_files "${STATIC_BC_FILES}")
string(REPLACE "," ";" _whole_archive_bc_files "${WHOLE_ARCHIVE_BC_FILES}")
if(NOT _direct_bc_files)
  message(FATAL_ERROR "Cannot merge LLVM IR without direct bitcode inputs.")
endif()

get_filename_component(_output_dir "${OUTPUT}" DIRECTORY)
file(MAKE_DIRECTORY "${_output_dir}")
set(_seed "${OUTPUT}.whole-archive-input.bc")
set(_current "${OUTPUT}.static-archive-current.bc")
set(_next "${OUTPUT}.static-archive-next.bc")
file(REMOVE "${_seed}" "${_current}" "${_next}")

# Whole-archive inputs are flattened member bitcode because the native linker
# is explicitly forced to load every member there. Merge those with direct
# inputs first, then let llvm-link apply archive member selection to ordinary
# static-library LLVM archives.
execute_process(
  COMMAND "${LLVM_LINK}" ${_direct_bc_files} ${_whole_archive_bc_files} -o "${_seed}"
  RESULT_VARIABLE _result
  OUTPUT_VARIABLE _output
  ERROR_VARIABLE _error
)
if(NOT _result EQUAL 0)
  message(FATAL_ERROR "LLVM direct bitcode merge failed:\n${_output}\n${_error}")
endif()

# llvm-link scans archives in argument order. A member selected from a later
# archive can introduce a reference satisfied by an earlier archive, so repeat
# the archive scan once per archive to reach the native linker's dependency
# closure while keeping archive member selection semantics.
if(_static_bc_files)
  list(LENGTH _static_bc_files _static_archive_count)
  file(RENAME "${_seed}" "${_current}")
  foreach(_pass RANGE 1 ${_static_archive_count})
    execute_process(
      COMMAND "${LLVM_LINK}" --only-needed "${_current}" ${_static_bc_files} -o "${_next}"
      RESULT_VARIABLE _result
      OUTPUT_VARIABLE _output
      ERROR_VARIABLE _error
    )
    if(NOT _result EQUAL 0)
      message(FATAL_ERROR "LLVM static dependency bitcode merge failed:\n${_output}\n${_error}")
    endif()
    file(RENAME "${_next}" "${_current}")
  endforeach()
  file(COPY_FILE "${_current}" "${OUTPUT}" ONLY_IF_DIFFERENT)
else()
  file(RENAME "${_seed}" "${OUTPUT}")
endif()
file(REMOVE "${_seed}" "${_current}" "${_next}")