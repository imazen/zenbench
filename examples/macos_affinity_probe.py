"""Native Mach API oracle matching the scoped Rust backend's policy calls.

Run on macOS with python3 -B. No remote files or compilation are needed when
piped to an SSH interpreter. Failure to save a policy prevents setting it.
This probe does not certify CPU IDs, P/E placement, or scheduling reliability.
"""

import ctypes as C
import json
import platform
import subprocess


def main():
    lib = C.CDLL('/usr/lib/libSystem.B.dylib')
    lib.pthread_self.argtypes = []
    lib.pthread_self.restype = C.c_void_p
    lib.pthread_mach_thread_np.argtypes = [C.c_void_p]
    lib.pthread_mach_thread_np.restype = C.c_uint32
    lib.thread_policy_get.argtypes = [
        C.c_uint32, C.c_int, C.POINTER(C.c_int32),
        C.POINTER(C.c_uint32), C.POINTER(C.c_int),
    ]
    lib.thread_policy_get.restype = C.c_int
    lib.thread_policy_set.argtypes = [
        C.c_uint32, C.c_int, C.POINTER(C.c_int32), C.c_uint32,
    ]
    lib.thread_policy_set.restype = C.c_int
    lib.mach_error_string.argtypes = [C.c_int]
    lib.mach_error_string.restype = C.c_char_p
    thread = lib.pthread_mach_thread_np(lib.pthread_self())

    def get():
        tag = C.c_int32(0)
        count = C.c_uint32(1)
        default = C.c_int(0)
        rc = lib.thread_policy_get(
            thread, 4, C.byref(tag), C.byref(count), C.byref(default)
        )
        return {
            'return_code': rc,
            'runtime_error_name': lib.mach_error_string(rc).decode(),
            'tag': tag.value,
            'count': count.value,
            'get_default': default.value,
        }

    def set_tag(tag):
        value = C.c_int32(tag)
        return lib.thread_policy_set(thread, 4, C.byref(value), 1)

    def sysctl(key):
        result = subprocess.run(['sysctl', '-n', key], text=True, capture_output=True)
        if result.returncode:
            return {'return_code': result.returncode, 'error': result.stderr.strip()}
        return result.stdout.strip()

    out = {
        'os': platform.platform(),
        'cpu': sysctl('machdep.cpu.brand_string'),
        'python_version': platform.python_version(),
        'probe': 'native Mach THREAD_AFFINITY_POLICY save/set/read/restore',
        'certifies_cpu_class': False,
        'topology': {
            key: sysctl(key) for key in [
                'hw.ncpu', 'hw.nperflevels',
                'hw.perflevel0.logicalcpu', 'hw.perflevel1.logicalcpu',
            ]
        },
    }
    before = get()
    out['before'] = before
    if before['return_code'] == 0 and before['count'] == 1:
        try:
            out['set_requested_tag'] = 12345
            out['set_return_code'] = set_tag(12345)
            out['after_set'] = get()
        finally:
            out['restore_return_code'] = set_tag(before['tag'])
            out['after_restore'] = get()
    else:
        out['set_not_attempted'] = (
            'Cannot save the original policy; matches zenbench guard behavior.'
        )
    print(json.dumps(out, indent=2))


if __name__ == '__main__':
    main()
