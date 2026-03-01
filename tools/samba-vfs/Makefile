# ClaudeFS Samba VFS Module Makefile
#
# Requirements:
#   - Samba development headers: apt install samba-dev (Ubuntu) / dnf install samba-devel (RHEL)
#   - ClaudeFS RPC library: built from claudefs-transport crate
#
# Usage:
#   make
#   sudo make install

SAMBA_CFLAGS  := $(shell pkg-config --cflags samba-util samba-hostconfig 2>/dev/null || \
                  echo "-I/usr/include/samba-4.0")
SAMBA_LDFLAGS := $(shell pkg-config --libs samba-util samba-hostconfig 2>/dev/null || \
                  echo "-lsamba-util")

# ClaudeFS RPC library (built from Rust crate)
CFS_LIB_DIR   ?= ../../target/release
CFS_LDFLAGS   := -L$(CFS_LIB_DIR) -lcfsrpc

# Compiler flags
CC      := gcc
CFLAGS  := -O2 -Wall -Wextra -fPIC -shared -DHAVE_SAMBA_HEADERS \
           $(SAMBA_CFLAGS)
LDFLAGS := $(SAMBA_LDFLAGS) $(CFS_LDFLAGS) -Wl,-rpath,$(CFS_LIB_DIR)

# Output
TARGET  := cfs_vfs.so
SRCS    := cfs_vfs.c

# Samba VFS module installation path
SAMBA_VFS_DIR := $(shell pkg-config --variable=modulesdir samba-modules 2>/dev/null || \
                  echo "/usr/lib/x86_64-linux-gnu/samba/vfs")

.PHONY: all install clean check-deps

all: check-deps $(TARGET)

$(TARGET): $(SRCS) cfsrpc.h
	$(CC) $(CFLAGS) -o $@ $(SRCS) $(LDFLAGS)

install: $(TARGET)
	install -m 755 -D $(TARGET) $(SAMBA_VFS_DIR)/$(TARGET)
	@echo "Installed $(TARGET) to $(SAMBA_VFS_DIR)/"
	@echo "Add 'vfs objects = cfs_vfs' to smb.conf share sections"

clean:
	rm -f $(TARGET)

check-deps:
	@pkg-config --exists samba-util samba-hostconfig 2>/dev/null || \
	  (echo "WARNING: samba-dev not found. Install with: apt install samba-dev" && true)
	@test -f $(CFS_LIB_DIR)/libcfsrpc.so 2>/dev/null || \
	  (echo "WARNING: libcfsrpc.so not found. Build with: cargo build --release -p claudefs-transport" && true)

# Show effective smb.conf snippet for testing
show-config:
	@echo ""
	@echo "Add to /etc/samba/smb.conf:"
	@echo ""
	@echo "  [data]"
	@echo "    path = /tmp/cfs-export"
	@echo "    vfs objects = cfs_vfs"
	@echo "    cfs:server = localhost:9400"
	@echo "    cfs:export = /data"
	@echo "    cfs:timeout_ms = 5000"
	@echo "    cfs:mtls = yes"
	@echo "    read only = no"
	@echo "    guest ok = no"
	@echo ""
