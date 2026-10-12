# Run from an extracted, revision-identified zenbench source tree.
# All descendants inherit the idle-priority, 3 GiB Job Object cap.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
if ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory -lt 4GB / 1KB) {
    throw 'Native controls require at least 4 GiB available before starting'
}
# This build envelope is separate from the 2 GiB repeatability fixture.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class ZenbenchNativeBuildJob {
 [StructLayout(LayoutKind.Sequential)] struct Basic {
  public long ProcessTime, JobTime; public uint Flags;
  public UIntPtr MinWorking, MaxWorking; public uint ActiveProcesses;
  public UIntPtr Affinity; public uint Priority, Scheduling;
 }
 [StructLayout(LayoutKind.Sequential)] struct Io {
  public ulong ReadOps, WriteOps, OtherOps, ReadBytes, WriteBytes, OtherBytes;
 }
 [StructLayout(LayoutKind.Sequential)] struct Extended {
  public Basic Basic; public Io Io;
  public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
 }
 [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr a, string name);
 [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(IntPtr j, int c, ref Extended e, uint n);
 [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr j, int c, out Extended e, uint n, IntPtr returned);
 [DllImport("kernel32.dll", SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr j, IntPtr p);
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 public static IntPtr Start() {
  IntPtr j=CreateJobObject(IntPtr.Zero,null);
  if(j==IntPtr.Zero) throw new Win32Exception();
  Extended e=new Extended(); e.Basic.Flags=0x2720; e.Basic.Priority=0x40;
  e.ProcessMemory=(UIntPtr)3221225472UL; e.JobMemory=(UIntPtr)3221225472UL;
  if(!SetInformationJobObject(j,9,ref e,(uint)Marshal.SizeOf(e))) throw new Win32Exception();
  if(!AssignProcessToJobObject(j,GetCurrentProcess())) throw new Win32Exception();
  return j;
 }
 public static ulong Peak(IntPtr j) {
  Extended e;
  if(!QueryInformationJobObject(j,9,out e,(uint)Marshal.SizeOf(typeof(Extended)),IntPtr.Zero)) throw new Win32Exception();
  if(e.ProcessMemory.ToUInt64()!=3221225472UL || e.JobMemory.ToUInt64()!=3221225472UL ||
     (e.Basic.Flags & 0x2720)!=0x2720 || e.Basic.Priority!=0x40) throw new Exception("Native build job limits changed");
  return e.PeakJobMemory.ToUInt64();
 }
}
'@
$job = [ZenbenchNativeBuildJob]::Start()
[ZenbenchNativeBuildJob]::Peak($job) | Out-Null
$env:CARGO_BUILD_JOBS = '1'
# Keep generated Windows API debug information out of the bounded control build.
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_INCREMENTAL = '0'
$env:RAYON_NUM_THREADS = '2'
$env:OMP_NUM_THREADS = '2'
$env:ZENBENCH_NO_SAVE = '1'
$env:ZENBENCH_CPU_POLICY = 'unrestricted' # Functional fixtures; defaults checked separately.
$env:TMP = Join-Path $root 'scratch'
$env:TEMP = $env:TMP
[IO.Directory]::CreateDirectory($env:TMP) | Out-Null
Start-Transcript -Path (Join-Path $root ('native-check-' + [Guid]::NewGuid().ToString('N') + '.log')) | Out-Null
try {
    rustc -vV
    if ($LASTEXITCODE -ne 0) { throw 'Rust compiler probe failed' }
    & "$PSScriptRoot/windows_topology_probe.ps1"
    & "$PSScriptRoot/windows_process_mask_probe.ps1"
    cargo run --locked --features heap-profiling --example heap_phases -- --exercise-oom
    if ($LASTEXITCODE -ne 0) { throw 'Heap controls failed' }
    cargo test --locked --features heap-profiling --lib heap::
    if ($LASTEXITCODE -ne 0) { throw 'Heap unit controls failed' }
    cargo test --locked --test windows_job_boundary --test fleet_cli
    if ($LASTEXITCODE -ne 0) { throw 'Job boundary/fleet controls failed' }
    Remove-Item Env:ZENBENCH_CPU_POLICY
    cargo run --locked --bin zenbench -- capabilities
    if ($LASTEXITCODE -ne 0) { throw 'Capability probe failed' }
    'Native heap and fleet controls passed'
} finally {
    [Console]::Error.WriteLine('native-control: idle priority; jobs=1; memory-cap-bytes=3221225472; peak-job-commit-bytes={0}', [ZenbenchNativeBuildJob]::Peak($job))
    Stop-Transcript | Out-Null
}
