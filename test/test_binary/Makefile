# Makefile for testing multiple compilation methods

# Use our clang-wrap instead of system clang
CLANG = ../../target/release/clang

# Enable LLVM IR generation (set to 1 to enable)
EMIT_LLVMIR ?= 1

# Enable debug mode (set to 1 to enable)
CLANG_WRAP_DEBUG ?= 1

# Export environment variables for clang-wrap
export EMIT_LLVMIR
export CLANG_WRAP_DEBUG

# Compiler flags
CFLAGS = -O2

SOURCES = test_multi_a.c test_multi_b.c
OBJECTS = test_multi_a.o test_multi_b.o

# Object list file for @file syntax test
OBJS_FILE = test_multi.objs

# Method 1: Single clang command (compile + link in one step)
TARGET_ONE = test_multi_one

# Method 2: Separate compile and link steps
TARGET_TWO = test_multi_two

# Method 3: Link with @file syntax (object files listed in a file)
TARGET_THREE = test_multi_three

.PHONY: all clean method1 method2 method3

all: method1 method2 method3

# Method 1: Direct compilation with one clang command
method1: $(TARGET_ONE)

$(TARGET_ONE): $(SOURCES)
	$(CLANG) $(CFLAGS) -o $@ $^

# Method 2: Two compile steps + one link step
method2: $(TARGET_TWO)

$(TARGET_TWO): $(OBJECTS)
	$(CLANG) $(CFLAGS) -o $@ $^

# Method 3: Use @file syntax for object files
method3: $(TARGET_THREE)

# Generate the object list file
$(OBJS_FILE): $(OBJECTS)
	@echo "Generating object list file: $@"
	@rm -f $@
	@for obj in $(OBJECTS); do echo "$$obj" >> $@; done

# Link using @file syntax
$(TARGET_THREE): $(OBJECTS) $(OBJS_FILE)
	$(CLANG) $(CFLAGS) -o $@ @$(OBJS_FILE)

test_multi_a.o: test_multi_a.c
	$(CLANG) $(CFLAGS) -c -o $@ $<

test_multi_b.o: test_multi_b.c
	$(CLANG) $(CFLAGS) -c -o $@ $<

clean:
	rm -f $(TARGET_ONE) $(TARGET_TWO) $(TARGET_THREE) $(OBJECTS) $(OBJS_FILE)
