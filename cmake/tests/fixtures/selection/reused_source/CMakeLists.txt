get_filename_component(_instance_name "${CMAKE_CURRENT_BINARY_DIR}" NAME)
add_library(${_instance_name}_lib STATIC reused.c)