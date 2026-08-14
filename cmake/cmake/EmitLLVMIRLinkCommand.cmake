if(NOT DEFINED TARGET_NAME OR NOT DEFINED TARGET_TYPE OR NOT DEFINED BC_FILE OR
   NOT DEFINED LINK_SCRIPT OR NOT DEFINED OUTPUT_NAME OR NOT DEFINED NATIVE_LINK_FILE OR
   NOT DEFINED NATIVE_OUTPUT_NAME OR NOT DEFINED NINJA_BUILD_DIR OR NOT DEFINED TARGET_BINARY_DIR OR NOT DEFINED CMAKE_MAKE_PROGRAM)
  message(FATAL_ERROR "TARGET_NAME, TARGET_TYPE, BC_FILE, LINK_SCRIPT, OUTPUT_NAME, NATIVE_LINK_FILE, NATIVE_OUTPUT_NAME, NINJA_BUILD_DIR, TARGET_BINARY_DIR, and CMAKE_MAKE_PROGRAM are required.")
endif()
if(NOT CLANG_VERSION_LINE)
  message(FATAL_ERROR "CLANG_VERSION_LINE is required so generated _cmd files record the Clang version.")
endif()

string(REPLACE "," ";" LINK_SEARCH_DIRS "${LINK_SEARCH_DIRS}")

# Return the ELF SONAME of a shared object.  The installed runtime package
# provides this name, whereas the concrete file may include a package update
# version that is not guaranteed to be present when the relink script runs.
function(_cmake_emit_llvmir_get_shared_library_soname library output_variable)
  if(NOT EXISTS "${library}")
    set(${output_variable} "" PARENT_SCOPE)
    return()
  endif()

  file(REAL_PATH "${library}" _resolved_library)
  execute_process(
    COMMAND readelf -d "${_resolved_library}"
    RESULT_VARIABLE _readelf_result
    OUTPUT_VARIABLE _dynamic_section
    ERROR_QUIET
  )
  if(_readelf_result EQUAL 0 AND
     _dynamic_section MATCHES "Library soname: \\[([^]]+)\\]")
    set(${output_variable} "${CMAKE_MATCH_1}" PARENT_SCOPE)
  else()
    set(${output_variable} "" PARENT_SCOPE)
  endif()
endfunction()

function(_cmake_emit_llvmir_append_shared_library_soname library original_argument output_variable)
  _cmake_emit_llvmir_get_shared_library_soname("${library}" _soname)
  if(_soname)
    set(${output_variable} "-l:${_soname}" PARENT_SCOPE)
  else()
    set(${output_variable} "${original_argument}" PARENT_SCOPE)
  endif()
endfunction()

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
    message(FATAL_ERROR "Cannot read Ninja link command for ${TARGET_NAME}.")
  endif()
  string(REPLACE "\n" ";" _command_lines "${_commands}")
  set(_native_command_candidates "")
  foreach(_line IN LISTS _command_lines)
    if(_line MATCHES "(^| )-o[ ]+([^ ]*/)?${NATIVE_OUTPUT_NAME}([ ]|$)")
      list(APPEND _native_command_candidates "${_line}")
    endif()
  endforeach()
  list(LENGTH _native_command_candidates _native_command_count)
  if(NOT _native_command_count EQUAL 1)
    message(FATAL_ERROR "Expected exactly one Ninja link command for ${TARGET_NAME} producing '${NATIVE_OUTPUT_NAME}', found ${_native_command_count}.")
  endif()
  list(GET _native_command_candidates 0 _native_command)
else()
  message(FATAL_ERROR "No supported native link command source for ${TARGET_NAME}.")
endif()

if(NOT _native_command)
  message(FATAL_ERROR "No link command found for ${TARGET_NAME}.")
endif()

# Ninja wraps real commands in ': && ... && :'.  Retain only the invocation.
string(REGEX REPLACE "^: && " "" _native_command "${_native_command}")
string(REGEX REPLACE "[ ]+&& :$" "" _native_command "${_native_command}")
separate_arguments(_arguments UNIX_COMMAND "${_native_command}")
include("${CMAKE_CURRENT_LIST_DIR}/ExpandResponseFile.cmake")
if(EXISTS "${NATIVE_LINK_FILE}")
  get_filename_component(_response_base_dir "${NATIVE_LINK_FILE}" DIRECTORY)
else()
  set(_response_base_dir "${TARGET_BINARY_DIR}")
endif()
cmake_emit_llvmir_expand_response_arguments(_arguments "${_response_base_dir}" ${_arguments})
string(REPLACE "," ";" PROJECT_STATIC_LIBRARIES "${PROJECT_STATIC_LIBRARIES}")
string(REPLACE "|" ";" PROJECT_SHARED_LIBRARIES "${PROJECT_SHARED_LIBRARIES}")
string(REPLACE "," ";" STATIC_MISSING_FILES "${STATIC_MISSING_FILES}")

# Include explicit -L paths in addition to compiler implicit paths when
# resolving linker library operands below.
set(_library_search_dirs ${LINK_SEARCH_DIRS})
set(_expect_library_search_dir FALSE)
foreach(_argument IN LISTS _arguments)
  if(_expect_library_search_dir)
    list(APPEND _library_search_dirs "${_argument}")
    set(_expect_library_search_dir FALSE)
  elseif(_argument STREQUAL "-L")
    set(_expect_library_search_dir TRUE)
  elseif(_argument MATCHES "^-L(.+)$")
    list(APPEND _library_search_dirs "${CMAKE_MATCH_1}")
  endif()
endforeach()
list(REMOVE_DUPLICATES _library_search_dirs)

# When the configuration-time handler identified a version script name but
# could not copy the source (it is generated at build time), copy it now.
if(VERSION_SCRIPT_NAME AND VERSION_SCRIPT_SOURCE)
  get_filename_component(_verscript_dir "${LINK_SCRIPT}" DIRECTORY)
  set(_verscript_destination "${_verscript_dir}/${VERSION_SCRIPT_NAME}")
  if(NOT EXISTS "${_verscript_destination}")
    if(NOT EXISTS "${VERSION_SCRIPT_SOURCE}")
      message(FATAL_ERROR "Version script for ${TARGET_NAME} was requested but does not exist: ${VERSION_SCRIPT_SOURCE}")
    endif()
    file(COPY_FILE "${VERSION_SCRIPT_SOURCE}" "${_verscript_destination}" ONLY_IF_DIFFERENT)
  endif()
endif()

set(_link_arguments "")
set(_static_link_arguments "")
set(_native_fallback_objects "")
set(_skip_next FALSE)
set(_pending_xlinker FALSE)
set(_pending_xlinker_version_script_path FALSE)
set(_pending_xlinker_dependency_file_path FALSE)
set(_pending_xlinker_rpath_link_path FALSE)
set(_skip_compiler_only_value FALSE)
set(_version_script_added FALSE)
set(_saw_output FALSE)
set(_soname_output_name "")
set(_has_optimization_level FALSE)
set(_reject_nonportable_linker_path FALSE)
foreach(_argument IN LISTS _arguments)
  if(_argument MATCHES "^-O($|[0-9]+$|fast$|g$|s$|z$)")
    set(_has_optimization_level TRUE)
  endif()
  if(_pending_xlinker_dependency_file_path)
    if(_argument STREQUAL "-Xlinker")
      # The dependency-file value can itself be forwarded with -Xlinker.
    else()
      set(_pending_xlinker_dependency_file_path FALSE)
    endif()
  elseif(_pending_xlinker_rpath_link_path)
    if(_argument STREQUAL "-Xlinker")
      # Consume the separator in '-Xlinker -rpath-link -Xlinker <path>'.
    else()
      # rpath-link is build-time linker search metadata.  It must not become
      # part of an installed, relocatable _cmd script.
      set(_pending_xlinker_rpath_link_path FALSE)
    endif()
  elseif(_pending_xlinker_version_script_path)
    if(_argument STREQUAL "-Xlinker")
      # Consume the separator in '-Xlinker <option> -Xlinker <value>'.
    else()
      if(NOT _version_script_added)
        list(APPEND _link_arguments "-Wl,--version-script=./${VERSION_SCRIPT_NAME}")
        set(_version_script_added TRUE)
      endif()
      set(_pending_xlinker_version_script_path FALSE)
    endif()
  elseif(_pending_xlinker)
    if(_argument MATCHES "^--dependency-file=")
      # CMake's link dependency tracking is meaningful only in its build tree.
    elseif(_argument STREQUAL "--dependency-file")
      set(_pending_xlinker_dependency_file_path TRUE)
    elseif(_argument STREQUAL "-rpath-link" OR _argument STREQUAL "--rpath-link")
      set(_pending_xlinker_rpath_link_path TRUE)
    elseif(_argument MATCHES "^(-rpath-link|--rpath-link)=")
      # The value is embedded in this argument.
    elseif(_argument MATCHES "^(--?version-script)=(.+)$" AND VERSION_SCRIPT_NAME)
      if(NOT _version_script_added)
        list(APPEND _link_arguments "-Wl,--version-script=./${VERSION_SCRIPT_NAME}")
        set(_version_script_added TRUE)
      endif()
    elseif((_argument STREQUAL "--version-script" OR _argument STREQUAL "-version-script") AND VERSION_SCRIPT_NAME)
      set(_pending_xlinker_version_script_path TRUE)
    elseif(_argument MATCHES "^/" OR
           _argument STREQUAL "--sysroot" OR _argument MATCHES "^--sysroot=" OR
           _argument STREQUAL "-T" OR _argument MATCHES "^-T.+" OR
           _argument STREQUAL "--script" OR _argument MATCHES "^--script=")
      # A path passed through -Xlinker cannot be relocated reliably.  Do not
      # copy it into an installable _cmd script.
      set(_reject_nonportable_linker_path TRUE)
    else()
      list(APPEND _link_arguments "-Xlinker" "${_argument}")
    endif()
    set(_pending_xlinker FALSE)
  elseif(_skip_next)
    set(_skip_next FALSE)
  elseif(_skip_compiler_only_value)
    set(_skip_compiler_only_value FALSE)
  elseif(_argument STREQUAL "-D" OR _argument STREQUAL "-U" OR
         _argument STREQUAL "-I" OR _argument STREQUAL "-F" OR
         _argument STREQUAL "-include" OR _argument STREQUAL "-imacros" OR
         _argument STREQUAL "-include-pch" OR _argument STREQUAL "-idirafter" OR
         _argument STREQUAL "-iprefix" OR _argument STREQUAL "-iwithprefix" OR
         _argument STREQUAL "-iwithprefixbefore" OR _argument STREQUAL "-isystem" OR
         _argument STREQUAL "-iquote" OR _argument STREQUAL "-isysroot" OR
         _argument STREQUAL "-resource-dir" OR _argument STREQUAL "-Xclang" OR
         _argument STREQUAL "-Xpreprocessor")
    set(_skip_compiler_only_value TRUE)
  elseif(_argument STREQUAL "-nostdinc" OR _argument STREQUAL "-nostdinc++" OR
         _argument STREQUAL "-fsyntax-only" OR _argument STREQUAL "-ffat-lto-objects" OR
         _argument MATCHES "^-D.+" OR _argument MATCHES "^-U.+" OR
         _argument MATCHES "^-I.+" OR _argument MATCHES "^-F.+" OR
         _argument MATCHES "^-(include|imacros|include-pch|idirafter|iprefix|iwithprefix|iwithprefixbefore|isystem|iquote|isysroot|resource-dir)=" OR
         _argument MATCHES "^-isysroot.+" OR
         _argument MATCHES "^-f(debug|file|macro)-prefix-map=")
    # _cmd links already-generated bitcode.  Front-end options are irrelevant
    # and frequently disclose build-tree include/sysroot paths.
  elseif(_argument STREQUAL "-rpath-link" OR _argument STREQUAL "--rpath-link")
    # Drop the following build-tree search path.
    set(_skip_next TRUE)
  elseif(_argument MATCHES "^(-rpath-link|--rpath-link)=")
    # See the split form above.
  elseif(_argument STREQUAL "--sysroot" OR _argument MATCHES "^--sysroot=" OR
         _argument STREQUAL "-T" OR _argument MATCHES "^-T.+" OR
         _argument STREQUAL "--script" OR _argument MATCHES "^--script=")
    set(_reject_nonportable_linker_path TRUE)
  elseif(_argument STREQUAL "-o")
    set(_skip_next TRUE)
    set(_saw_output TRUE)
  elseif(_argument MATCHES "^-o.+")
    set(_saw_output TRUE)
  elseif(_argument MATCHES "(^|.*/)CMakeFiles/[^/]+\\.dir/.*\\.o$")
    # Replace C/C++ objects with merged bitcode. Preserve other native inputs.
    if(NOT _argument MATCHES "\\.(c|C|cc|cpp|cxx|c\\+\\+|CC|CPP|CXX)\\.(o|O)$")
      if(IS_ABSOLUTE "${_argument}")
        set(_native_object "${_argument}")
      else()
        set(_native_object "${TARGET_BINARY_DIR}/${_argument}")
      endif()
      if(NOT EXISTS "${_native_object}")
        message(FATAL_ERROR "Native object for ${TARGET_NAME} does not exist: ${_native_object}")
      endif()
      get_filename_component(_native_object_abs "${_native_object}" ABSOLUTE)
      string(SHA256 _native_hash "${_native_object_abs}")
      string(SUBSTRING "${_native_hash}" 0 12 _native_hash)
      get_filename_component(_native_name "${_argument}" NAME)
      get_filename_component(_script_dir "${LINK_SCRIPT}" DIRECTORY)
      set(_native_relative_path "asm/${_native_hash}-${_native_name}")
      set(_native_destination "${_script_dir}/${_native_relative_path}")
      get_filename_component(_native_destination_dir "${_native_destination}" DIRECTORY)
      file(MAKE_DIRECTORY "${_native_destination_dir}")
      file(COPY_FILE "${_native_object}" "${_native_destination}" ONLY_IF_DIFFERENT)
      list(APPEND _link_arguments "./${_native_relative_path}")
    endif()
  elseif(_argument STREQUAL "-L")
    # It is used above to resolve -l inputs, but retaining the build-tree
    # search directory would make the installed script non-portable.
    set(_skip_next TRUE)
  elseif(_argument MATCHES "^-L.+$")
    # See the split -L form above.
  elseif(_argument MATCHES "^-l(([^:][^/]*)|:(.+))$")
    # A bare '-lfoo' depends on a development-package 'libfoo.so' symlink
    # being present at relink time. If the search paths instead expose only
    # a versioned file such as 'libfoo.so.1.0.1', rewrite the input so the
    # linker matches that exact filename without requiring the symlink.
    set(_lib_name "${CMAKE_MATCH_2}")
    set(_lib_filename "${CMAKE_MATCH_3}")
    set(_resolved_lib_argument "")
    if(_library_search_dirs)
      foreach(_search_dir IN LISTS _library_search_dirs)
        if(NOT EXISTS "${_search_dir}")
          continue()
        endif()
        if(_lib_filename)
          if(EXISTS "${_search_dir}/${_lib_filename}")
            set(_candidate_shared_libraries "${_search_dir}/${_lib_filename}")
          else()
            set(_candidate_shared_libraries "")
          endif()
        else()
          file(GLOB _candidate_shared_libraries
            "${_search_dir}/lib${_lib_name}.so.*"
          )
        endif()
        if(NOT _candidate_shared_libraries)
          continue()
        endif()
        list(SORT _candidate_shared_libraries COMPARE NATURAL)
        list(REVERSE _candidate_shared_libraries)
        foreach(_candidate IN LISTS _candidate_shared_libraries)
          _cmake_emit_llvmir_append_shared_library_soname(
            "${_candidate}" "${_argument}" _normalized_lib_argument)
          if(NOT _normalized_lib_argument STREQUAL "${_argument}")
            list(APPEND _link_arguments "${_normalized_lib_argument}")
            set(_resolved_lib_argument TRUE)
            break()
          endif()
        endforeach()
        if(_resolved_lib_argument)
          break()
        endif()
      endforeach()
    endif()
    if(NOT _resolved_lib_argument)
      list(APPEND _link_arguments "${_argument}")
    endif()
  elseif(_argument STREQUAL "-Xlinker")
    # Keep linker-specific options. They can carry required flags such as a
    # version script and are already represented as separate native arguments.
    set(_pending_xlinker TRUE)
  elseif(_argument MATCHES "^-Wl,(--?version-script)=" AND VERSION_SCRIPT_NAME)
    if(NOT _version_script_added)
      list(APPEND _link_arguments "-Wl,--version-script=./${VERSION_SCRIPT_NAME}")
      set(_version_script_added TRUE)
    endif()
  elseif(_argument MATCHES "^-Wl,(--?version-script)," AND VERSION_SCRIPT_NAME)
    if(NOT _version_script_added)
      list(APPEND _link_arguments "-Wl,--version-script=./${VERSION_SCRIPT_NAME}")
      set(_version_script_added TRUE)
    endif()
  elseif((_argument STREQUAL "-Wl,-version-script" OR
          _argument STREQUAL "-Wl,--version-script") AND VERSION_SCRIPT_NAME)
    # Libtool passes the generated script as a separate `-Wl,<path>` argument.
    # Replace both arguments with the installed _verscript sidecar.
    if(NOT _version_script_added)
      list(APPEND _link_arguments "-Wl,--version-script=./${VERSION_SCRIPT_NAME}")
      set(_version_script_added TRUE)
    endif()
    set(_skip_next TRUE)
  elseif(_argument MATCHES "^-Wl,-soname,([^,]+)$")
    set(_soname_output_name "${CMAKE_MATCH_1}")
    list(APPEND _link_arguments "${_argument}")
  elseif(_argument MATCHES "^--dependency-file=")
    # Build-system dependency tracking is meaningful only in its build tree.
  elseif(_argument STREQUAL "--dependency-file")
    # See the -Xlinker form above; drop the following dependency-file path.
    set(_skip_next TRUE)
  elseif(_argument MATCHES "^-Wl,--dependency-file(,|=)")
    # CMake/Ninja linker dependency tracking is not part of the relink ABI.
  elseif(_argument STREQUAL "-Wl,--dependency-file")
    set(_skip_next TRUE)
  elseif(_argument MATCHES "^-Wl,(-rpath|--rpath)[,=]")
    # Preserve the portable runtime-relative form, but not build-tree rpaths.
    string(FIND "${_argument}" "\$ORIGIN" _origin_position)
    if(NOT _origin_position EQUAL -1)
      list(APPEND _link_arguments "${_argument}")
    endif()
  elseif(_argument MATCHES "^-Wl,(-rpath-link|--rpath-link)(,|=|$)")
    # Unlike rpath, rpath-link is not encoded into the output. It only helps
    # the native build resolve indirect shared-library dependencies.
  elseif(_argument MATCHES "^-Wl,")
    string(REPLACE "," ";" _wl_values "${_argument}")
    set(_wl_has_absolute_path FALSE)
    foreach(_wl_value IN LISTS _wl_values)
      if(_wl_value MATCHES "^/")
        set(_wl_has_absolute_path TRUE)
      endif()
    endforeach()
    if(_wl_has_absolute_path OR
       _argument MATCHES "^-Wl,(--sysroot|-T|--script)(,|=|$)")
      set(_reject_nonportable_linker_path TRUE)
    else()
      list(APPEND _link_arguments "${_argument}")
    endif()
  else()
    get_filename_component(_argument_name "${_argument}" NAME)
    if(_argument IN_LIST PROJECT_STATIC_LIBRARIES OR _argument_name IN_LIST PROJECT_STATIC_LIBRARIES)
      # Its C/C++ bitcode was merged during the build, while any native
      # members were copied to asm/ above. Do not retain an archive dependency.
    elseif(_argument IN_LIST PROJECT_SHARED_LIBRARIES OR _argument_name IN_LIST PROJECT_SHARED_LIBRARIES)
      # Relink scripts are independently executable.  Refer to the installed
      # native library directly instead of relying on another _cmd script to
      # have populated an output directory first. The native command can use
      # paths relative to its target build directory (for example ../lib/),
      # which do not apply to the installed llvmir directory.
      list(APPEND _link_arguments "../${_argument_name}")
    elseif(IS_ABSOLUTE "${_argument}" AND EXISTS "${_argument}" AND
           _argument_name MATCHES "^.+\.so(\.[^/]+)*$")
      # Development packages commonly expose libfoo.so as a symlink while
      # the runtime package only ships libfoo.so.<SOVERSION>. Resolve the
      # external library now so installed relink scripts do not require the
      # development symlink.
      _cmake_emit_llvmir_append_shared_library_soname(
        "${_argument}" "${_argument}" _normalized_lib_argument)
      list(APPEND _link_arguments "${_normalized_lib_argument}")
    elseif(_argument_name MATCHES "^(.+\.so)(\.[^/]+)+$")
      set(_shared_library_base "${CMAKE_MATCH_1}")
      if(_shared_library_base IN_LIST PROJECT_SHARED_LIBRARIES)
        # CMake target metadata can expose an unversioned OUTPUT_NAME while
        # the native command links the ELF SONAME (for example libfoo.so.1).
        # Preserve the native versioned basename but relocate it beside the
        # installed llvmir directory.
        list(APPEND _link_arguments "../${_argument_name}")
      else()
        list(APPEND _link_arguments "${_argument}")
      endif()
    elseif(TARGET_TYPE STREQUAL "EXECUTABLE" AND
           _argument_name MATCHES "^.+\\.so(\\.[^/]+)*$" AND
           EXISTS "${TARGET_BINARY_DIR}/${_argument_name}")
      # Some projects link tools with a bare versioned shared-library filename
      # instead of a CMake target. The installed script runs from llvmir-bin,
      # while the matching library lives in the sibling llvmir directory.
      list(APPEND _link_arguments "../${_argument_name}")
    elseif(_argument_name MATCHES "^.+\\.(o|a)$")
      # An explicit object or an archive that is not a project CMake target
      # has no emitted-IR/sidecar contract. Preserve it as a native fallback
      # beside the installed script rather than retaining its build-tree path.
      if(IS_ABSOLUTE "${_argument}")
        set(_native_input "${_argument}")
      else()
        set(_native_input "${TARGET_BINARY_DIR}/${_argument}")
      endif()
      if(NOT EXISTS "${_native_input}")
        message(FATAL_ERROR "Native link input for ${TARGET_NAME} does not exist: ${_native_input}")
      endif()
      get_filename_component(_native_input_abs "${_native_input}" ABSOLUTE)
      string(SHA256 _native_hash "${_native_input_abs}")
      string(SUBSTRING "${_native_hash}" 0 12 _native_hash)
      get_filename_component(_script_dir "${LINK_SCRIPT}" DIRECTORY)
      set(_native_relative_path "asm/${_native_hash}-${_argument_name}")
      set(_native_destination "${_script_dir}/${_native_relative_path}")
      get_filename_component(_native_destination_dir "${_native_destination}" DIRECTORY)
      file(MAKE_DIRECTORY "${_native_destination_dir}")
      file(COPY_FILE "${_native_input}" "${_native_destination}" ONLY_IF_DIFFERENT)
      list(APPEND _link_arguments "./${_native_relative_path}")
    else()
      list(APPEND _link_arguments "${_argument}")
    endif()
  endif()
endforeach()

# Static dependency bitcode is merged into this target. Its native archive
# members cannot enter LLVM IR, so make them local asm/ inputs of this script.
foreach(_missing_file IN LISTS STATIC_MISSING_FILES)
  if(NOT _missing_file OR NOT EXISTS "${_missing_file}")
    continue()
  endif()
  file(STRINGS "${_missing_file}" _static_missing_members)
  list(APPEND _native_fallback_objects ${_static_missing_members})
endforeach()
list(REMOVE_DUPLICATES _native_fallback_objects)
foreach(_native_object IN LISTS _native_fallback_objects)
  if(NOT IS_ABSOLUTE "${_native_object}")
    message(FATAL_ERROR "Static native fallback for ${TARGET_NAME} is not absolute: ${_native_object}")
  endif()
  if(NOT EXISTS "${_native_object}")
    message(FATAL_ERROR "Static native fallback for ${TARGET_NAME} does not exist: ${_native_object}")
  endif()
  string(SHA256 _native_hash "${_native_object}")
  string(SUBSTRING "${_native_hash}" 0 12 _native_hash)
  get_filename_component(_native_name "${_native_object}" NAME)
  get_filename_component(_script_dir "${LINK_SCRIPT}" DIRECTORY)
  set(_native_relative_path "asm/${_native_hash}-${_native_name}")
  set(_native_destination "${_script_dir}/${_native_relative_path}")
  get_filename_component(_native_destination_dir "${_native_destination}" DIRECTORY)
  file(MAKE_DIRECTORY "${_native_destination_dir}")
  file(COPY_FILE "${_native_object}" "${_native_destination}" ONLY_IF_DIFFERENT)
  list(APPEND _link_arguments "./${_native_relative_path}")
endforeach()

if(_pending_xlinker_version_script_path)
  message(FATAL_ERROR "Native link command for ${TARGET_NAME} has a version-script option without a path.")
endif()

if(NOT _saw_output)
  message(FATAL_ERROR "Native link command for ${TARGET_NAME} has no output option.")
endif()

if(_reject_nonportable_linker_path)
  # Do not generate an intentionally broken _cmd.  The target's build must
  # fail while the original link context is still available.
  file(REMOVE "${LINK_SCRIPT}")
  message(FATAL_ERROR "Cannot generate _cmd for ${TARGET_NAME}: unsupported non-relocatable linker path option (sysroot, linker script, or absolute linker input).")
endif()

# Native final-link commands commonly omit -O*, but linking LLVM IR requires
# an optimization policy. Match clang-wrap: honor an explicit level and use
# -O3 when the native command does not provide one.
if(NOT _has_optimization_level)
  list(APPEND _link_arguments "-O3")
endif()

set(_output_name "${OUTPUT_NAME}")
if(TARGET_TYPE MATCHES "^(SHARED_LIBRARY|MODULE_LIBRARY)$" AND _soname_output_name)
  # A shared object's relinked filename must match its ELF SONAME. This is
  # significant for libraries whose version is expressed only through
  # SOVERSION, such as libcpuinfo.so.0.
  set(_output_name "${_soname_output_name}")
endif()
set(_output_argument "output/${_output_name}")

list(APPEND _link_arguments "-x" "ir" "./${BC_FILE}")
if(_static_link_arguments)
  # The merged module is LLVM IR, but following LLVM archives must be parsed
  # according to their archive format rather than forced through -x ir.
  list(APPEND _link_arguments "-x" "none")
endif()
list(APPEND _link_arguments ${_static_link_arguments})
list(APPEND _link_arguments "--output=${_output_argument}")
list(INSERT _link_arguments 1 "-fuse-ld=lld")
# Executable commands are installed in lib/llvmir-bin, and libraries from the
# same package are installed in the parent lib directory. This preserves local
# resolution after library inputs have been normalized to -l:SONAME.
list(INSERT _link_arguments 2 "-L..")

set(_script "#!/bin/bash\n")
string(APPEND _script "# Clang version: ${CLANG_VERSION_LINE}\n")
string(APPEND _script "set -e\nmkdir -p output\ncmd=(\n")
foreach(_argument IN LISTS _link_arguments)
  string(REPLACE "'" "'\"'\"'" _escaped "${_argument}")
  string(APPEND _script "  '${_escaped}'\n")
endforeach()
string(APPEND _script ")\n\"\${cmd[@]}\"\n")
file(WRITE "${LINK_SCRIPT}" "${_script}")
file(CHMOD "${LINK_SCRIPT}" PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE GROUP_READ GROUP_EXECUTE WORLD_READ WORLD_EXECUTE)