#ifndef OBJECT_VARIANT
#error "OBJECT_VARIANT must be supplied by the owning object library"
#endif

int object_variant_value(void) {
  return OBJECT_VARIANT;
}