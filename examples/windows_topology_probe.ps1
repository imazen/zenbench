# Read-only native Windows topology oracle. No affinity changes or CPU workload.
# https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-getlogicalprocessorinformationex
param([switch]$CompileOnly)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([IntPtr]::Size -ne 8) { throw 'Use 64-bit PowerShell (ARM64 on Windows ARM).' }

if (-not ('ZenbenchWindowsTopologyProbe' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class ZenbenchWindowsTopologyProbe {
    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetLogicalProcessorInformationEx(
        int relationship, IntPtr buffer, ref uint length);

    public static byte[] Read() {
        uint length = 0;
        if (GetLogicalProcessorInformationEx(0xffff, IntPtr.Zero, ref length))
            throw new InvalidOperationException("Unexpected empty topology query success");
        int error = Marshal.GetLastWin32Error();
        if (error != 122) throw new Win32Exception(error);
        for (int attempt = 0; attempt < 3; attempt++) {
            if (length < 8 || length > 16 * 1024 * 1024)
                throw new InvalidOperationException("Invalid native topology buffer size");
            uint capacity = length;
            IntPtr memory = Marshal.AllocHGlobal((int)capacity);
            try {
                if (GetLogicalProcessorInformationEx(0xffff, memory, ref length)) {
                    if (length < 8 || length > capacity)
                        throw new InvalidOperationException("Invalid topology readback length");
                    byte[] result = new byte[length];
                    Marshal.Copy(memory, result, 0, (int)length);
                    return result;
                }
                error = Marshal.GetLastWin32Error();
                if (error != 122) throw new Win32Exception(error);
            } finally { Marshal.FreeHGlobal(memory); }
        }
        throw new InvalidOperationException("Topology changed during every query attempt");
    }
}
'@
}
if ($CompileOnly) { return }
if ($env:OS -ne 'Windows_NT') { throw 'The native topology query requires Windows.' }

function Get-MaskObservation([byte[]]$Data, [int]$Offset, [int]$End) {
    if ($Offset -lt 0 -or $Offset + 16 -gt $End) { throw 'Truncated GROUP_AFFINITY' }
    $cpus = @(for ($bit = 0; $bit -lt 64; $bit++) {
        if (($Data[$Offset + ($bit -shr 3)] -band (1 -shl ($bit % 8))) -ne 0) { $bit }
    })
    [pscustomobject]@{
        group = [BitConverter]::ToUInt16($Data, $Offset + 8)
        mask_hex = '0x{0:X16}' -f [BitConverter]::ToUInt64($Data, $Offset)
        logical_ids_in_group = $cpus
    }
}

$data = [ZenbenchWindowsTopologyProbe]::Read()
$cores = @()
$caches = @()
$groups = @()
$offset = 0
while ($offset -lt $data.Length) {
    if ($data.Length - $offset -lt 8) { throw 'Truncated native record header' }
    $relationship = [BitConverter]::ToUInt32($data, $offset)
    $size = [BitConverter]::ToUInt32($data, $offset + 4)
    if ($size -lt 8 -or $size -gt $data.Length - $offset) { throw 'Invalid native record length' }
    $end = $offset + [int]$size
    if ($relationship -eq 0) {
        if ($size -lt 48) { throw 'Truncated processor-core record' }
        $count = [BitConverter]::ToUInt16($data, $offset + 30)
        if ($count -ne 1) { throw 'A physical core must have one group mask' }
        $cores += [pscustomobject]@{
            core_index = $cores.Count
            efficiency_class = $data[$offset + 9]
            smt = ($data[$offset + 8] -band 1) -ne 0
            affinity = Get-MaskObservation $data ($offset + 32) $end
        }
    } elseif ($relationship -eq 2) {
        if ($size -lt 56) { throw 'Truncated cache record' }
        # Older SDKs reserve this field: zero means the legacy single mask.
        $count = [Math]::Max(1, [BitConverter]::ToUInt16($data, $offset + 38))
        $masks = @(for ($i = 0; $i -lt $count; $i++) {
            Get-MaskObservation $data ($offset + 40 + 16 * $i) $end
        })
        $caches += [pscustomobject]@{
            cache_index = $caches.Count
            level = $data[$offset + 8]
            size_bytes = [BitConverter]::ToUInt32($data, $offset + 12)
            line_bytes = [BitConverter]::ToUInt16($data, $offset + 10)
            type_code = [BitConverter]::ToUInt32($data, $offset + 16)
            sharing_masks = $masks
        }
    } elseif ($relationship -eq 4) {
        if ($size -lt 80) { throw 'Truncated processor-group record' }
        $count = [BitConverter]::ToUInt16($data, $offset + 10)
        if ($count -lt 1) { throw 'No active processor groups' }
        for ($i = 0; $i -lt $count; $i++) {
            $start = $offset + 32 + 48 * $i
            if ($start + 48 -gt $end) { throw 'Truncated PROCESSOR_GROUP_INFO' }
            $groups += [pscustomobject]@{
                group = $i
                maximum_processors = $data[$start]
                active_processors = $data[$start + 1]
                active_mask_hex = '0x{0:X16}' -f [BitConverter]::ToUInt64($data, $start + 40)
            }
        }
    }
    $offset = $end
}
if ($cores.Count -eq 0 -or $groups.Count -eq 0) { throw 'Incomplete native core/group topology' }
$runtimeArchitecture = if ('System.Runtime.InteropServices.RuntimeInformation' -as [type]) {
    [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
} else { $env:PROCESSOR_ARCHITECTURE }
$os = Get-CimInstance Win32_OperatingSystem
[pscustomobject]@{
    source = 'GetLogicalProcessorInformationEx(RelationAll)'
    os = $os.Caption
    os_version = $os.Version
    process_architecture = $runtimeArchitecture
    reported_architecture = $env:PROCESSOR_ARCHITECTURE
    cpu = @((Get-CimInstance Win32_Processor).Name)
    efficiency_classes = @($cores.efficiency_class | Sort-Object -Unique)
    efficiency_class_note = 'Higher values indicate greater intrinsic performance; not measured speed.'
    core_records = $cores
    cache_records = $caches
    processor_groups = $groups
    changes_affinity = $false
    certifies_runtime_placement = $false
} | ConvertTo-Json -Depth 8
