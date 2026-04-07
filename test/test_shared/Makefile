# Makefile for testing shared library compilation

# Use our clang-wrap instead of system clang
CLANG = ../../target/release/clang

# Use our install-wrap instead of system install
INSTALL = ../../target/release/install
LN = ../../target/release/ln

# Enable LLVM IR generation (set to 1 to enable)
EMIT_LLVMIR ?= 1

# Enable debug mode (set to 1 to enable)
CLANG_WRAP_DEBUG ?= 1

# Export environment variables for clang-wrap
export EMIT_LLVMIR
export CLANG_WRAP_DEBUG

# Compiler flags
CFLAGS = -O2 -fPIC

# Source files
LIB_SRC = libadd.c libaddf.c
MAIN_SRC = main.c

# Output files
LIB_OBJ = libadd.o libaddf.o
LIB_SO = libadd.so
MAIN_OBJ = main.o

# Version info
VERSION = 1.0.0
SOVERSION = 1

# Version script (original filename)
VERSION_SCRIPT = libadd.version

# Target
TARGET = test_shared

# Installation prefix
PREFIX ?= /usr/local
LIBDIR = $(PREFIX)/lib
BINDIR = $(PREFIX)/bin
INCLUDEDIR = $(PREFIX)/include

.PHONY: all clean install uninstall

all: $(TARGET)

libadd.o: libadd.c
	$(CLANG) $(CFLAGS) -c -o $@ $<

libaddf.o: libaddf.c
	$(CLANG) $(CFLAGS) -c -o $@ $<

$(LIB_SO): $(LIB_OBJ)
	$(CLANG) -shared -Wl,--version-script=$(VERSION_SCRIPT) \
		-Wl,-soname,libadd.so.$(SOVERSION) \
		-o libadd.so.$(VERSION) $(LIB_OBJ)
	$(LN) -sf libadd.so.$(VERSION) libadd.so.$(SOVERSION)
	$(LN) -sf libadd.so.$(SOVERSION) $@

$(MAIN_OBJ): $(MAIN_SRC)
	$(CLANG) $(CFLAGS) -c -o $@ $<

$(TARGET): $(MAIN_OBJ) $(LIB_SO)
	$(CLANG) -o $@ $(MAIN_OBJ) -L. -ladd -Wl,-rpath,.

clean:
	rm -f $(TARGET) $(LIB_OBJ) libadd.so libadd.so.$(SOVERSION) libadd.so.$(VERSION) $(MAIN_OBJ)

# Install shared library, executable
install: all
	# Create directories
	$(INSTALL) -d $(DESTDIR)$(LIBDIR)
	$(INSTALL) -d $(DESTDIR)$(BINDIR)
	$(INSTALL) -d $(DESTDIR)$(INCLUDEDIR)
	
	$(INSTALL) -m 755 libadd.so.$(VERSION) $(DESTDIR)$(LIBDIR)
	$(LN) -sf libadd.so.$(VERSION) $(DESTDIR)$(LIBDIR)/libadd.so.$(SOVERSION)
	$(LN) -sf libadd.so.$(SOVERSION) $(DESTDIR)$(LIBDIR)/libadd.so
	
	# Install header
	$(INSTALL) -m 644 libadd.h $(DESTDIR)$(INCLUDEDIR)
	
	$(INSTALL) -m 755 $(TARGET) $(DESTDIR)$(BINDIR)
	
# Uninstall all installed files
uninstall:
	rm -f $(DESTDIR)$(LIBDIR)/libadd.so.$(VERSION)
	rm -f $(DESTDIR)$(LIBDIR)/libadd.so.$(SOVERSION)
	rm -f $(DESTDIR)$(LIBDIR)/libadd.so
	rm -f $(DESTDIR)$(INCLUDEDIR)/libadd.h
	rm -f $(DESTDIR)$(BINDIR)/$(TARGET)
