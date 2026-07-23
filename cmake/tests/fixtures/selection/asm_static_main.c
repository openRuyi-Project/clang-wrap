int asm_static_library_value(void);

int main(void)
{
  return asm_static_library_value() == 77 ? 0 : 1;
}