#include <stdio.h>
#include "sha2.h"

int main(void) {
    printf("c-host-sha2: %s ABI=%u version=%s name=%s\n",
           RIG_PACKAGE_NAME_sha2,
           (unsigned)sha2_abi_version(),
           sha2_version(),
           sha2_name());
    return 0;
}
