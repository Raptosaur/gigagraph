#ifndef DRONE_PID_H
#define DRONE_PID_H

typedef struct {
    float kp;
    float ki;
    float kd;
    float integral;
    float last_error;
} pid_t;

void pid_init(pid_t *pid, float kp, float ki, float kd);
float pid_step(pid_t *pid, float setpoint, float measured, float dt);
void pid_reset(pid_t *pid);

static inline float pid_clamp(float value, float lo, float hi) {
    if (value < lo) return lo;
    if (value > hi) return hi;
    return value;
}

#endif
