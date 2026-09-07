#include <stdio.h>
#include <string.h>
#include "sha2.h"

int main(void) {
    printf("c-host-sha2: %s ABI=%u version=%s name=%s\n",
           RIG_PACKAGE_NAME_sha2,
           (unsigned)sha2_abi_version(),
           sha2_version(),
           sha2_name());

    const char *msg = "hi";
    unsigned char out[32];
    int rc = sha2_hash_256((const uint8_t *)msg, strlen(msg), out);
    printf("c-host-sha2: hash_256(rc=%d) ", rc);
    for (int i = 0; i < 32; i++) printf("%02x", out[i]);
    printf("\n");
    return rc == 0 ? 0 : 1;
}
