"""Internal isolated-worker entry point: ``python -m servo_fetch._worker``."""

from servo_fetch._native import run_worker_stdio

if __name__ == "__main__":
    run_worker_stdio()
