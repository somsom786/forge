#!/usr/bin/env python3
"""Measure the Windows test process while running the existing memory FAQ evaluation.

Build the Rust test executable first. The reported peak includes the test harness, fixture setup,
both retrieval modes and SQLite; it excludes child processes and is not a per-query allocation.
"""
import argparse
import ctypes
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


class MemoryCounters(ctypes.Structure):
    _fields_ = [('cb', ctypes.c_ulong), ('PageFaultCount', ctypes.c_ulong),
                ('PeakWorkingSetSize', ctypes.c_size_t), ('WorkingSetSize', ctypes.c_size_t),
                ('QuotaPeakPagedPoolUsage', ctypes.c_size_t), ('QuotaPagedPoolUsage', ctypes.c_size_t),
                ('QuotaPeakNonPagedPoolUsage', ctypes.c_size_t), ('QuotaNonPagedPoolUsage', ctypes.c_size_t),
                ('PagefileUsage', ctypes.c_size_t), ('PeakPagefileUsage', ctypes.c_size_t)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--test-exe', required=True, type=Path, help='Rust desktop test executable')
    parser.add_argument('--output', required=True, type=Path, help='evaluation JSON output path')
    parser.add_argument('faq', nargs='+', type=Path, help='FAQ Markdown files (paths may not contain commas)')
    args = parser.parse_args()
    if os.name != 'nt':
        parser.error('this working-set measurement uses the Windows process API')
    if any(',' in str(path) for path in args.faq):
        parser.error('the evaluation uses commas to separate FAQ paths')
    env = os.environ.copy()
    env['FORGE_MEMORY_EVAL'] = ','.join(str(path.resolve()) for path in args.faq)
    env['FORGE_MEMORY_EVAL_OUT'] = str(args.output.resolve())
    env['RUST_TEST_THREADS'] = '1'
    with tempfile.TemporaryFile() as log:
        child = subprocess.Popen([str(args.test_exe.resolve()), 'knowledge::memory_eval::memory_eval_faq_retrieval', '--exact', '--nocapture'], env=env,
                                 stdout=log, stderr=subprocess.STDOUT,
                                 creationflags=subprocess.CREATE_NO_WINDOW | subprocess.BELOW_NORMAL_PRIORITY_CLASS)
        peak = 0
        while child.poll() is None:
            counters = MemoryCounters()
            counters.cb = ctypes.sizeof(counters)
            if not ctypes.windll.psapi.GetProcessMemoryInfo(ctypes.c_void_p(int(child._handle)), ctypes.byref(counters), counters.cb):
                print('Could not read process memory; the reported peak may be incomplete.', file=sys.stderr)
            else:
                peak = max(peak, counters.PeakWorkingSetSize)
            time.sleep(0.02)
        log.seek(0)
        output = log.read().decode('utf-8', errors='replace')
    print(output)
    print(f'Test exit: {child.returncode}; sampled peak working set: {peak} bytes')
    if child.returncode != 0 or 'test result: ok. 1 passed;' not in output or peak == 0:
        print('The requested evaluation did not run successfully or no memory measurement was obtained.', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
