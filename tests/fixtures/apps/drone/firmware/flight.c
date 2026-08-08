#include <stdio.h>
#include "pid.h"

typedef enum { MODE_IDLE, MODE_HOVER, MODE_LAND } flight_mode_t;

static pid_t altitude_pid;
static flight_mode_t mode = MODE_IDLE;

void flight_boot(void) {
    pid_init(&altitude_pid, 0.8f, 0.01f, 0.05f);
    mode = MODE_HOVER;
}

float flight_tick(float target_altitude, float altitude, float dt) {
    if (mode != MODE_HOVER) {
        return 0.0f;
    }
    return pid_step(&altitude_pid, target_altitude, altitude, dt);
}

void flight_land(void) {
    mode = MODE_LAND;
    pid_reset(&altitude_pid);
}

const char *flight_mode_name(flight_mode_t m) {
    switch (m) {
        case MODE_IDLE: return "idle";
        case MODE_HOVER: return "hover";
        case MODE_LAND: return "land";
    }
    return "unknown";
}

int main(int argc, char **argv) {
    flight_boot();
    printf("%s %f\n", flight_mode_name(mode), flight_tick(2.0f, 1.0f, 0.01f));
    return 0;
}
