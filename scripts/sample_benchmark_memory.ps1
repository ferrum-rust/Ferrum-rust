param(
    [Parameter(Mandatory=$true)][string]$ProcessName,
    [Parameter(Mandatory=$true)][string]$Output,
    [ValidateRange(1,60)][int]$IntervalSeconds = 20,
    [ValidateRange(30,900)][int]$TimeoutSeconds = 240,
    [switch]$SystemMemory
)
$ErrorActionPreference = 'Stop'
$outputPath = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Path (Split-Path -Parent $outputPath) -Force | Out-Null
# Optional interop compiles, loads DLLs and JITs before process waiting. The
# caller must wait for SYSTEM_MEMORY_SAMPLER_READY before launching the game.
if ($SystemMemory) {
    if (-not ('FerrumBenchmark.NativeMemoryV1' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace FerrumBenchmark {
    public static class NativeMemoryV1 {
        // Exact windows-sys 0.61.2 MEMORYSTATUSEX: 2 DWORD + 7 DWORDLONG.
        [StructLayout(LayoutKind.Sequential)]
        private struct MemoryStatusEx {
            public UInt32 dwLength, dwMemoryLoad;
            public UInt64 ullTotalPhys, ullAvailPhys, ullTotalPageFile, ullAvailPageFile;
            public UInt64 ullTotalVirtual, ullAvailVirtual, ullAvailExtendedVirtual;
        }
        // Exact PROCESS_MEMORY_COUNTERS: 2 DWORD + 8 SIZE_T. UIntPtr preserves
        // native pointer width on both 32-bit and 64-bit sampler hosts.
        [StructLayout(LayoutKind.Sequential)]
        private struct ProcessMemoryCounters {
            public UInt32 cb, PageFaultCount;
            public UIntPtr PeakWorkingSetSize, WorkingSetSize;
            public UIntPtr QuotaPeakPagedPoolUsage, QuotaPagedPoolUsage;
            public UIntPtr QuotaPeakNonPagedPoolUsage, QuotaNonPagedPoolUsage;
            public UIntPtr PagefileUsage, PeakPagefileUsage;
        }
        [DllImport("kernel32.dll", ExactSpelling=true, SetLastError=true, CallingConvention=CallingConvention.Winapi)]
        private static extern Int32 GlobalMemoryStatusEx(ref MemoryStatusEx value);
        [DllImport("psapi.dll", ExactSpelling=true, SetLastError=true, CallingConvention=CallingConvention.Winapi)]
        private static extern Int32 GetProcessMemoryInfo(IntPtr process, ref ProcessMemoryCounters value, UInt32 size);
        public sealed class Sample {
            public UInt64 TotalPhysicalBytes, AvailablePhysicalBytes;
            public UInt64 TotalPageFileBytes, AvailablePageFileBytes;
            public UInt32 MemoryLoadPercent, ProcessPageFaultsTotal;
        }
        private static readonly UInt32 StatusSize=(UInt32)Marshal.SizeOf(typeof(MemoryStatusEx));
        private static readonly UInt32 ProcessSize=(UInt32)Marshal.SizeOf(typeof(ProcessMemoryCounters));
        public static void ValidateLayout() {
            if (StatusSize!=64 || ProcessSize!=(IntPtr.Size==8?72:40)
                || Marshal.OffsetOf(typeof(MemoryStatusEx),"ullTotalPhys").ToInt32()!=8
                || Marshal.OffsetOf(typeof(ProcessMemoryCounters),"PageFaultCount").ToInt32()!=4
                || Marshal.OffsetOf(typeof(ProcessMemoryCounters),"WorkingSetSize").ToInt32()!=8+IntPtr.Size
                || Marshal.OffsetOf(typeof(ProcessMemoryCounters),"PagefileUsage").ToInt32()!=8+6*IntPtr.Size) {
                throw new InvalidOperationException("Native memory-counter ABI layout mismatch.");
            }
        }
        public static Sample Read(IntPtr process) {
            MemoryStatusEx system=new MemoryStatusEx();system.dwLength=StatusSize;
            if (GlobalMemoryStatusEx(ref system)==0) {
                throw new Win32Exception(Marshal.GetLastWin32Error(),"GlobalMemoryStatusEx failed.");
            }
            ProcessMemoryCounters counters=new ProcessMemoryCounters();counters.cb=ProcessSize;
            if (GetProcessMemoryInfo(process,ref counters,ProcessSize)==0) {
                throw new Win32Exception(Marshal.GetLastWin32Error(),"GetProcessMemoryInfo failed.");
            }
            // The process handle is borrowed from System.Diagnostics.Process;
            // this helper neither opens nor closes it and never changes memory.
            return new Sample {TotalPhysicalBytes=system.ullTotalPhys,AvailablePhysicalBytes=system.ullAvailPhys,
                TotalPageFileBytes=system.ullTotalPageFile,AvailablePageFileBytes=system.ullAvailPageFile,
                MemoryLoadPercent=system.dwMemoryLoad,ProcessPageFaultsTotal=counters.PageFaultCount};
        }
    }
}
'@
    }
    [FerrumBenchmark.NativeMemoryV1]::ValidateLayout()
    $warmupProcess = [Diagnostics.Process]::GetCurrentProcess()
    try { [FerrumBenchmark.NativeMemoryV1]::Read($warmupProcess.Handle) | Out-Null }
    finally { $warmupProcess.Dispose() }
    Write-Output "SYSTEM_MEMORY_SAMPLER_READY pid=$PID utc=$([DateTime]::UtcNow.ToString('o'))"
}
$timer = [Diagnostics.Stopwatch]::StartNew()
$observedId = $null
$samples = 0
while ($timer.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
    $matches = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
    if ($null -eq $observedId) {
        if ($matches.Count -gt 1) { throw 'Multiple matching benchmark processes.' }
        if ($matches.Count -eq 0) { Start-Sleep -Seconds 1; continue }
        $observedId = $matches[0].Id
    }
    $process = $matches | Where-Object Id -eq $observedId | Select-Object -First 1
    if ($null -eq $process) { break }
    try {
        $rowData = [ordered]@{
            elapsed_seconds = ([DateTime]::Now - $process.StartTime).TotalSeconds
            process_id = $process.Id
            working_set_mib = $process.WorkingSet64 / 1MB
            private_mib = $process.PrivateMemorySize64 / 1MB
            peak_working_set_mib = $process.PeakWorkingSet64 / 1MB
            cpu_seconds = $process.CPU
        }
        if ($SystemMemory) {
            $native = [FerrumBenchmark.NativeMemoryV1]::Read($process.Handle)
            $rowData.system_total_physical_mib = $native.TotalPhysicalBytes / 1MB
            $rowData.system_available_physical_mib = $native.AvailablePhysicalBytes / 1MB
            $rowData.system_memory_load_percent = $native.MemoryLoadPercent
            $rowData.global_memory_status_total_page_file_mib = $native.TotalPageFileBytes / 1MB
            $rowData.global_memory_status_available_page_file_mib = $native.AvailablePageFileBytes / 1MB
            $rowData.process_page_faults_total_u32 = $native.ProcessPageFaultsTotal
            $rowData.page_fault_scope = 'Cumulative Win32 DWORD, wraps at 2^32; includes SOFT and HARD faults. Not hard-fault count or disk I/O.'
            $rowData.page_file_scope = 'GlobalMemoryStatusEx ullTotalPageFile/ullAvailPageFile queried by sampler: commit-related limits/availability, not on-disk pagefile usage; process limits may apply.'
        }
        $row = $rowData | ConvertTo-Json -Compress
        if ($samples -eq 0) { $row | Set-Content -LiteralPath $outputPath }
        else { $row | Add-Content -LiteralPath $outputPath }
        $samples++
    } catch {
        if ($process.HasExited) { break }
        throw
    }
    Start-Sleep -Seconds $IntervalSeconds
}
if ($samples -eq 0) { throw 'No benchmark process observed.' }
Write-Output "Recorded $samples memory samples for process $observedId."
