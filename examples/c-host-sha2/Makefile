CC ?= cc
CFLAGS ?= -Wall -Wextra
SHA2_DIR := target/rig/sha2

.PHONY: all run clean
all: main

main: src/main.c
	$(CC) $(CFLAGS) -o $@ src/main.c -I src/rig_bindings -I $(SHA2_DIR) \
		-L $(SHA2_DIR) -lsha2_ffi -Wl,-rpath,$(SHA2_DIR)

run: main
	./main

clean:
	rm -f main
