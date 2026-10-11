"""Native pthread QoS diagnostic with separate 100 ms CPU-time warmup.

One fresh worker per QoS request, measured sequentially. Uses identical native
PBKDF2 work and records requested QoS only; no CPU/frequency certification.
"""
import ctypes as C
import hashlib
import json
import platform
import resource
import statistics
import subprocess
import threading
import time


def main():
    # This short interpreter probe has bounded native work and allocations.
    resource.setrlimit(resource.RLIMIT_CPU, (5, 5))
    # Darwin rejects lowering RLIMIT_DATA/AS on this host. Monitor native peak
    # RSS during the bounded exercise; do not claim a hard remote memory cap.
    def check_memory():
        if resource.getrusage(resource.RUSAGE_SELF).ru_maxrss > 256 * 1024 ** 2:
            raise RuntimeError('Native probe exceeded its RSS admission limit')
    check_memory()
    lib = C.CDLL('/usr/lib/libSystem.B.dylib')
    lib.pthread_self.restype = C.c_void_p
    lib.pthread_get_qos_class_np.argtypes = [C.c_void_p, C.POINTER(C.c_uint32), C.POINTER(C.c_int)]
    lib.pthread_get_qos_class_np.restype = C.c_int
    lib.pthread_set_qos_class_self_np.argtypes = [C.c_uint32, C.c_int]
    lib.pthread_set_qos_class_self_np.restype = C.c_int

    def get_qos():
        value = C.c_uint32()
        priority = C.c_int()
        rc = lib.pthread_get_qos_class_np(lib.pthread_self(), C.byref(value), C.byref(priority))
        if rc:
            raise RuntimeError(f'pthread QoS query failed: {rc}')
        return {'class': value.value, 'relative_priority': priority.value}

    def work():
        return hashlib.pbkdf2_hmac('sha256', b'zenbench-qos', b'fixed-work', 4000, 32)

    expected = work()
    observations = []
    for name, value in [('user_interactive', 0x21), ('background', 0x09)]:
        def worker():
            out = {'requested_qos': name, 'requested_class': value}
            before = get_qos()
            out['before'] = before
            if before['class'] not in [0x21, 0x19, 0x15, 0x11, 0x09]:
                raise RuntimeError('Cannot restore the original requested QoS')
            try:
                rc = lib.pthread_set_qos_class_self_np(value, 0)
                out['set_return_code'] = rc
                if rc:
                    raise RuntimeError(f'pthread QoS request failed: {rc}')
                out['readback'] = get_qos()
                if out['readback'] != {'class': value, 'relative_priority': 0}:
                    raise RuntimeError('Requested QoS readback differs')
                # Entire warmup is outside metrics, using per-thread CPU time
                # so descheduling does not shorten the exercise itself.
                warmup_wall = time.perf_counter_ns()
                warmup_cpu = time.thread_time_ns()
                while time.thread_time_ns() - warmup_cpu < 100_000_000:
                    if work() != expected:
                        raise RuntimeError('Warmup produced different output')
                    check_memory()
                out['warmup_cpu_ns'] = time.thread_time_ns() - warmup_cpu
                out['warmup_wall_ns'] = time.perf_counter_ns() - warmup_wall
                samples = []
                for _ in range(12):
                    start = time.perf_counter_ns()
                    result = work()
                    samples.append(time.perf_counter_ns() - start)
                    if result != expected:
                        raise RuntimeError('Sample produced different output')
                    check_memory()
                out['elapsed_per_call_ns'] = samples
                out['median_ns'] = statistics.median(samples)
                out['qos_after_samples'] = get_qos()
                if out['qos_after_samples'] != out['readback']:
                    raise RuntimeError('Requested QoS changed during samples')
            except Exception as error:
                out['error'] = str(error)
            finally:
                out['restore_return_code'] = lib.pthread_set_qos_class_self_np(before['class'], before['relative_priority'])
                out['after_restore'] = get_qos()
                observations.append(out)

        thread = threading.Thread(target=worker)
        thread.start()
        thread.join()
    if len(observations) != 2 or any('error' in out for out in observations):
        raise RuntimeError(f'QoS probe failed: {observations}')
    if any(out['restore_return_code'] or out['after_restore'] != out['before'] for out in observations):
        raise RuntimeError(f'QoS restoration failed: {observations}')
    print(json.dumps({
        'cpu': subprocess.check_output(['sysctl', '-n', 'machdep.cpu.brand_string'], text=True).strip(),
        'os': platform.platform(), 'python_version': platform.python_version(),
        'workload': 'PBKDF2-HMAC-SHA256, 4000 iterations, 32-byte output',
        'warmup_min_thread_cpu_ns': 100_000_000, 'samples_per_worker': 12,
        'worker_execution': 'sequential fresh threads, one per QoS class',
        'certifies_cpu_class': False, 'frequency_measured': False,
        'native_max_rss_bytes': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
        'native_hard_memory_cap': False, 'native_rss_admission_limit_bytes': 256 * 1024 ** 2,
        'observations': observations,
    }, indent=2))


if __name__ == '__main__':
    main()
