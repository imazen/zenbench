# Native primitive oracle: restrict this diagnostic process and existing workers.
# No benchmark workload or system policy changes; prior process mask is restored.
param([int]$CpuId = -1, [switch]$CompileOnly)
$ErrorActionPreference = 'Stop'
if ([IntPtr]::Size -ne 8) { throw 'Use 64-bit PowerShell.' }
if (-not ('ZenbenchWindowsProcessMaskProbe' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Threading;
using System.Threading.Tasks;
using System.Runtime.InteropServices;
public static class ZenbenchWindowsProcessMaskProbe {
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
    [DllImport("kernel32.dll")] static extern ushort GetActiveProcessorGroupCount();
    [DllImport("kernel32.dll")] static extern uint GetCurrentProcessorNumber();
    [DllImport("kernel32.dll", SetLastError=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool GetProcessAffinityMask(IntPtr process, out UIntPtr mask, out UIntPtr system);
    [DllImport("kernel32.dll", SetLastError=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool SetProcessAffinityMask(IntPtr process, UIntPtr mask);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern UIntPtr SetThreadAffinityMask(IntPtr thread, UIntPtr mask);
    public class Worker {
        public string Kind;
        public string Before;
        public string After;
        public string Restored;
        public uint ObservedCpu;
        public bool WidenRejected;
        public int WidenError;
    }
    public class Result {
        public int CpuId;
        public string Before;
        public string Restricted;
        public string Restored;
        public Worker[] Workers;
    }
    static string Hex(ulong mask) { return "0x" + mask.ToString("X16"); }
    static ulong ProcessMask() {
        UIntPtr mask, system;
        if (!GetProcessAffinityMask(GetCurrentProcess(), out mask, out system))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        return mask.ToUInt64();
    }
    static ulong ThreadMask(ulong permitted) {
        UIntPtr prior = SetThreadAffinityMask(GetCurrentThread(), new UIntPtr(permitted));
        if (prior == UIntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
        if (SetThreadAffinityMask(GetCurrentThread(), prior) == UIntPtr.Zero)
            throw new Win32Exception(Marshal.GetLastWin32Error());
        return prior.ToUInt64();
    }
    public static Result Run(int cpu) {
        if (GetActiveProcessorGroupCount() != 1)
            throw new InvalidOperationException("Oracle requires one processor group");
        ulong original = ProcessMask();
        if (cpu == -1) {
            for (cpu = 0; cpu < 64; cpu++) if ((original & (1UL << cpu)) != 0) break;
        }
        if (cpu < 0 || cpu >= 64 || (original & (1UL << cpu)) == 0)
            throw new InvalidOperationException("CPU is outside the original process mask");
        ulong selected = 1UL << cpu;
        if ((original & ~selected) == 0)
            throw new InvalidOperationException("Oracle needs an outside CPU for its negative control");
        Worker[] workers = { new Worker { Kind="existing_managed_thread" },
                             new Worker { Kind="existing_managed_pool_worker" } };
        Exception[] errors = new Exception[2];
        ulong[] priorMasks = new ulong[2];
        using (CountdownEvent ready = new CountdownEvent(2))
        using (CountdownEvent observed = new CountdownEvent(2))
        using (ManualResetEventSlim proceed = new ManualResetEventSlim(false))
        using (ManualResetEventSlim restored = new ManualResetEventSlim(false)) {
            Action<int> work = index => {
                try {
                    priorMasks[index] = ThreadMask(original);
                    workers[index].Before = Hex(priorMasks[index]);
                }
                catch (Exception e) { errors[index] = e; }
                finally { ready.Signal(); }
                if (!proceed.Wait(TimeSpan.FromSeconds(10))) {
                    errors[index] = new TimeoutException("Process restriction signal missing");
                }
                try {
                    if (errors[index] != null) return;
                    UIntPtr widened = SetThreadAffinityMask(GetCurrentThread(), new UIntPtr(original));
                    workers[index].WidenError = Marshal.GetLastWin32Error();
                    workers[index].WidenRejected = widened == UIntPtr.Zero;
                    workers[index].After = Hex(ThreadMask(selected));
                    workers[index].ObservedCpu = GetCurrentProcessorNumber();
                    if (!workers[index].WidenRejected || workers[index].WidenError != 87)
                        throw new InvalidOperationException("Worker widening was not rejected as invalid");
                    if (workers[index].After != Hex(selected) || workers[index].ObservedCpu != cpu)
                        throw new InvalidOperationException("Worker escaped the selected process CPU");
                } catch (Exception e) { errors[index] = e; }
                finally {
                    observed.Signal();
                    try {
                        if (!restored.Wait(TimeSpan.FromSeconds(10)))
                            throw new TimeoutException("Process restoration signal missing");
                        if (priorMasks[index] != 0) {
                            if (SetThreadAffinityMask(GetCurrentThread(), new UIntPtr(priorMasks[index])) == UIntPtr.Zero)
                                throw new Win32Exception(Marshal.GetLastWin32Error());
                            workers[index].Restored = Hex(ThreadMask(original));
                            if (workers[index].Restored != workers[index].Before)
                                throw new InvalidOperationException("Worker restoration differs");
                        }
                    } catch (Exception e) { errors[index] = e; }
                }
            };
            Thread ordinary = new Thread(() => work(0));
            ordinary.IsBackground = true;
            ordinary.Start();
            Task pool = Task.Run(() => work(1));
            Result result = new Result { CpuId=cpu, Before=Hex(original), Workers=workers };
            try {
                if (!ready.Wait(TimeSpan.FromSeconds(10))) throw new TimeoutException("Workers did not start");
                if (errors[0] != null || errors[1] != null) throw new AggregateException(Array.FindAll(errors, e => e != null));
                if (!SetProcessAffinityMask(GetCurrentProcess(), new UIntPtr(selected)))
                    throw new Win32Exception(Marshal.GetLastWin32Error());
                result.Restricted = Hex(ProcessMask());
                if (result.Restricted != Hex(selected)) throw new InvalidOperationException("Process readback differs");
                proceed.Set();
                if (!observed.Wait(TimeSpan.FromSeconds(10))) throw new TimeoutException("Workers did not observe restriction");
                if (errors[0] != null || errors[1] != null) throw new AggregateException(Array.FindAll(errors, e => e != null));
            } finally {
                try {
                    if (!SetProcessAffinityMask(GetCurrentProcess(), new UIntPtr(original)))
                        throw new Win32Exception(Marshal.GetLastWin32Error());
                } finally {
                    proceed.Set();
                    restored.Set();
                    if (!ordinary.Join(10000) || !pool.Wait(10000)) throw new TimeoutException("Worker cleanup did not finish");
                }
                if (errors[0] != null || errors[1] != null) throw new AggregateException(Array.FindAll(errors, e => e != null));
                result.Restored = Hex(ProcessMask());
                if (result.Restored != result.Before) throw new InvalidOperationException("Process restoration differs");
            }
            return result;
        }
    }
}
'@
}
if ($CompileOnly) { return }
if ($env:OS -ne 'Windows_NT') { throw 'The native process-mask query requires Windows.' }
[pscustomobject]@{
    source = 'GetProcessAffinityMask/SetProcessAffinityMask/SetThreadAffinityMask'
    scope = 'Native primitive oracle; existing managed thread and managed pool worker, not Rayon or compiled zenbench'
    result = [ZenbenchWindowsProcessMaskProbe]::Run($CpuId)
    contains_benchmark_workload = $false
} | ConvertTo-Json -Depth 8
