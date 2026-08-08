#include "pid.h"

void pid_init(pid_t *pid, float kp, float ki, float kd) {
    pid->kp = kp;
    pid->ki = ki;
    pid->kd = kd;
    pid_reset(pid);
}

void pid_reset(pid_t *pid) {
    pid->integral = 0.0f;
    pid->last_error = 0.0f;
}

float pid_step(pid_t *pid, float setpoint, float measured, float dt) {
    float error = setpoint - measured;
    pid->integral += error * dt;
    float derivative = (error - pid->last_error) / dt;
    pid->last_error = error;
    return pid_clamp(pid->kp * error + pid->ki * pid->integral + pid->kd * derivative, -1.0f, 1.0f);
}
