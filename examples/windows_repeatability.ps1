# Checks an SFTP-staged executable's hash and runs
# fresh benchmark processes under an inherited native Job Object memory cap.
param([string]$Sha256, [string]$CpuSets = '0;2', [int]$Repeats = 2, [switch]$CompileOnly)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class ZenbenchBenchJob {
 [StructLayout(LayoutKind.Sequential)] public struct MemoryCounters {
  public uint Size, Faults;
  public UIntPtr PeakWorkingSet, WorkingSet, PagedPeak, Paged,
   NonPagedPeak, NonPaged, Pagefile, PeakPagefile;
 }
 [DllImport("psapi.dll", SetLastError=true)] static extern bool GetProcessMemoryInfo(IntPtr p, ref MemoryCounters m, uint n);
 public static ulong PeakRss(IntPtr p) {
  MemoryCounters m = new MemoryCounters(); m.Size=(uint)Marshal.SizeOf(m);
  if(!GetProcessMemoryInfo(p,ref m,m.Size)) throw new Win32Exception();
  return m.PeakWorkingSet.ToUInt64();
 }
 [StructLayout(LayoutKind.Sequential)] public struct Basic {
  public long ProcessTime, JobTime; public uint Flags;
  public UIntPtr MinWorking, MaxWorking; public uint ActiveProcesses;
  public UIntPtr Affinity; public uint Priority, Scheduling;
 }
 [StructLayout(LayoutKind.Sequential)] public struct Io {
  public ulong ReadOps, WriteOps, OtherOps, ReadBytes, WriteBytes, OtherBytes;
 }
 [StructLayout(LayoutKind.Sequential)] public struct Extended {
  public Basic Basic; public Io Io;
  public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
 }
 [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr a, string name);
 [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(IntPtr j, int c, ref Extended e, uint n);
 [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr j, int c, out Extended e, uint n, IntPtr returned);
 [DllImport("kernel32.dll", SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr j, IntPtr p);
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 public static IntPtr Start() {
  IntPtr j = CreateJobObject(IntPtr.Zero, null);
  if(j == IntPtr.Zero) throw new Win32Exception();
  Extended e = new Extended();
  // SDK flags: per-process memory, total job memory, unhandled-exception
  // exit, fixed priority, and termination when the final job handle closes.
  e.Basic.Flags = 0x100 | 0x200 | 0x400 | 0x20 | 0x2000;
  e.Basic.Priority = 0x40; // IDLE_PRIORITY_CLASS, inherited by every child
  e.ProcessMemory = (UIntPtr)2147483648UL;
  e.JobMemory = (UIntPtr)2147483648UL;
  if(!SetInformationJobObject(j, 9, ref e, (uint)Marshal.SizeOf(e))) throw new Win32Exception();
  if(!AssignProcessToJobObject(j, GetCurrentProcess())) throw new Win32Exception();
  return j; // Kept alive until parent exits; all children run synchronously.
 }
 public static ulong Peak(IntPtr j) {
  Extended e;
  if(!QueryInformationJobObject(j,9,out e,(uint)Marshal.SizeOf(typeof(Extended)),IntPtr.Zero)) throw new Win32Exception();
  if(e.ProcessMemory.ToUInt64()!=2147483648UL || e.JobMemory.ToUInt64()!=2147483648UL ||
     (e.Basic.Flags & 0x2720)!=0x2720 || e.Basic.Priority!=0x40) throw new Exception("Native job limits changed");
  return e.PeakJobMemory.ToUInt64();
 }
}
'@
if ($CompileOnly) { 'Native benchmark Job Object declarations compiled'; return }
if ($Sha256 -notmatch '^[0-9a-f]{64}$') { throw 'Invalid executable hash' }
if ($Repeats -lt 1 -or $Repeats -gt 32) { throw 'Repeats must be 1..32' }
$sets = $CpuSets.Split(';')
foreach ($set in $sets) { if ($set -notin @('default','performance','efficiency') -and $set -notmatch '^\d+(,\d+)*$') { throw 'Invalid CPU set' } }
$job = [ZenbenchBenchJob]::Start()
[ZenbenchBenchJob]::Peak($job) | Out-Null # verify every limit before launching work
$root = Join-Path $env:USERPROFILE ('tmp\zenbench-native-' + $Sha256.Substring(0,12))
[IO.Directory]::CreateDirectory($root) | Out-Null
$exe = Join-Path $env:USERPROFILE ('tmp\zenbench-native-' + $Sha256 + '.exe')
if ((Get-Item $exe).Length -gt 64MB) { throw 'Executable exceeds transfer bound' }
if ((Get-FileHash -Algorithm SHA256 $exe).Hash.ToLowerInvariant() -ne $Sha256) { throw 'Executable checksum mismatch' }
$env:ZENBENCH_NO_SAVE='1'; $env:ZENBENCH_NO_CALIBRATE='1'
Remove-Item Env:ZENBENCH_CPU_POLICY,Env:ZENBENCH_CPU_SET,Env:ZENBENCH_PIN -ErrorAction SilentlyContinue
$runs = @()
for ($repeat=0; $repeat -lt $Repeats; $repeat++) {
 # Alternate class order across repetitions rather than warming one class first.
 $order = if ($repeat % 2 -eq 0) { $sets } else { @($sets[($sets.Count-1)..0]) }
 foreach ($set in $order) {
  $psi = New-Object Diagnostics.ProcessStartInfo
  $psi.FileName=$exe; $psi.Arguments=$set; $psi.WorkingDirectory=$root
  $psi.UseShellExecute=$false; $psi.RedirectStandardOutput=$true; $psi.RedirectStandardError=$true
  $p = New-Object Diagnostics.Process; $p.StartInfo=$psi
  if (!$p.Start()) { throw 'Child did not start' }
  $outTask=$p.StandardOutput.ReadToEndAsync(); $errTask=$p.StandardError.ReadToEndAsync()
  if (!$p.WaitForExit(90000)) { $p.Kill(); $p.WaitForExit(); throw 'Native benchmark timeout' }
  $stdout=$outTask.GetAwaiter().GetResult(); $stderr=$errTask.GetAwaiter().GetResult()
  $peakRss=[ZenbenchBenchJob]::PeakRss($p.Handle)
  $run = [ordered]@{cpu_set=$set; repeat=$repeat; exit_code=$p.ExitCode; peak_rss_bytes=$peakRss; stderr=$stderr}
  if ($p.ExitCode -eq 0) { $run.evidence=($stdout | ConvertFrom-Json) } else { $run.stdout=$stdout }
  $runs += $run
  [Console]::Error.WriteLine('completed set={0} repeat={1} exit={2} peak-rss={3}', $set,$repeat,$p.ExitCode,$peakRss)
  $p.Dispose()
 }
}
[ordered]@{binary_sha256=$Sha256; memory_limit_bytes=2147483648; priority='idle'; peak_job_commit_bytes=[ZenbenchBenchJob]::Peak($job); runs=$runs} | ConvertTo-Json -Depth 40 -Compress
