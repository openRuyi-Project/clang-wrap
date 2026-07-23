int embedded_static_dependency(void);

int embedded_value(void) {
  return 41;
}

int main(void) {
  return embedded_value() + embedded_static_dependency() == 42 ? 0 : 1;
}