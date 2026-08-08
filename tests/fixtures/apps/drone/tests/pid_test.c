#include <assert.h>
#include "../firmware/pid.h"

static pid_t make_pid(void) {
    pid_t pid;
    pid_init(&pid, 1.0f, 0.0f, 0.0f);
    return pid;
}

void test_pid_clamps_high(void) {
    pid_t pid = make_pid();
    assert(pid_step(&pid, 100.0f, 0.0f, 0.1f) == 1.0f);
}

void test_pid_reset_clears_integral(void) {
    pid_t pid = make_pid();
    pid_step(&pid, 1.0f, 0.0f, 0.1f);
    pid_reset(&pid);
    assert(pid.integral == 0.0f);
}

int main(void) {
    test_pid_clamps_high();
    test_pid_reset_clears_integral();
    return 0;
}
