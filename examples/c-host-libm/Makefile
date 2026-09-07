CC ?= cc
CFLAGS ?= -Wall -Wextra
LIBM_DIR := target/rig/libm

.PHONY: all run clean
all: main

main: src/main.c
	$(CC) $(CFLAGS) -o $@ src/main.c -I src/rig_bindings -I $(LIBM_DIR) \
		-L $(LIBM_DIR) -llibm_ffi -Wl,-rpath,$(LIBM_DIR)

run: main
	./main

clean:
	rm -f main
