#include <stdio.h>
#include <stdint.h>
#include "libm.h"

int main(void) {
    printf("abi=%u name=%s ver=%s\n",
           libm_abi_version(), libm_name(), libm_version());
    double x = 9.0;
    double y = libm_sqrt(x);
    printf("libm_sqrt(%.1f)=%.1f\n", x, y);
    if (y < 2.999 || y > 3.001) {
        fprintf(stderr, "unexpected sqrt\n");
        return 1;
    }
    return 0;
}
