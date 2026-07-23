int fixture_exported_symbol(void);

int static_shared_dependency_value(void) {
  return fixture_exported_symbol();
}