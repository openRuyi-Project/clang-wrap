int fixture_dependency_value(void);

int main(void) { return fixture_dependency_value() == 23 ? 0 : 1; }