param(
    [string]$Seed = '8924706420471469774',
    [ValidateRange(2,128)][int]$Radius = 16,
    [double]$Seconds = 3,
    [ValidateRange(0.1,1000)][double]$SlowFrameMs = 8,
    [Parameter(Mandatory=$true)][string]$Output,
    [string]$Executable = 'target\release\ferrum189.exe',
    [switch]$DistantReady,
    [switch]$CaptureTransparency,
    [ValidateSet('original','owner-batched','scoped-one','scoped-large','scoped-wide','scoped-two')][string]$TransparencySort,
    [ValidateSet('queue','staged')][string]$TransparencyUpload,
    [ValidateSet('direct','mdi')][string]$TransparencyDraw,
    [ValidateSet('baseline','visual','all')][string]$WorkerPriority,
    [switch]$CaptureNearOpaque,
    [switch]$CaptureUploads,
    [switch]$CaptureArenaUploads,
    [switch]$GpuTimestamps,
    [switch]$GpuStages,
    [switch]$SurfaceWaits,
    [switch]$PreAcquireVisibility,
    [switch]$PackedFrustum,
    [switch]$CullCoveredLod,
    [switch]$NearOpaqueRuns,
    [ValidateSet('original','dedicated','direct','mdi')][string]$NearArena,
    [double]$ReturnAfter = 0
)
$ErrorActionPreference = 'Stop'
if ($CullCoveredLod -and ($PackedFrustum -or $PreAcquireVisibility)) { throw 'Covered LOD, packed frustum and pre-acquire visibility are separate trials.' }
if ($GpuStages -and -not $GpuTimestamps) { throw 'GpuStages requires GpuTimestamps.' }
if ($PackedFrustum -and $PreAcquireVisibility) { throw 'PackedFrustum and PreAcquireVisibility are separate, mutually exclusive trials.' }
if ([double]::IsNaN($SlowFrameMs) -or [double]::IsInfinity($SlowFrameMs) -or $SlowFrameMs -lt 0.1 -or $SlowFrameMs -gt 1000) {
    throw 'SlowFrameMs must be finite and in 0.1..=1000 milliseconds.'
}
$slowFrameDiagnostic = $PSBoundParameters.ContainsKey('SlowFrameMs') -and $SlowFrameMs -ne 8
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    $binary = (Resolve-Path -LiteralPath $Executable).Path
    $outputPath = if ([IO.Path]::IsPathRooted($Output)) { [IO.Path]::GetFullPath($Output) }
        else { [IO.Path]::GetFullPath((Join-Path $taskRoot $Output)) }
    New-Item -ItemType Directory -Path $outputPath -Force | Out-Null
    $beforeHash = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
    $arguments = @('--benchmark', '--benchmark-streaming', '--creative', '--seed', $Seed,
        '--render-distance', "$Radius", '--seconds', $Seconds.ToString([Globalization.CultureInfo]::InvariantCulture),
        '--benchmark-output-dir', $outputPath)
    if ($PSBoundParameters.ContainsKey('SlowFrameMs')) {
        $arguments += @('--benchmark-slow-frame-ms', $SlowFrameMs.ToString([Globalization.CultureInfo]::InvariantCulture))
    }
    if ($DistantReady) { $arguments += '--benchmark-distant-ready' }
    if ($CaptureTransparency) { $arguments += '--benchmark-capture-transparency' }
    if ($PSBoundParameters.ContainsKey('TransparencySort')) {
        $arguments += @('--benchmark-transparency-sort', $TransparencySort)
    }
    if ($PSBoundParameters.ContainsKey('TransparencyUpload')) {
        $arguments += @('--benchmark-transparency-upload', $TransparencyUpload)
    }
    if ($PSBoundParameters.ContainsKey('TransparencyDraw')) {
        $arguments += @('--benchmark-transparency-draw', $TransparencyDraw)
    }
    if ($PSBoundParameters.ContainsKey('WorkerPriority')) {
        $arguments += @('--benchmark-worker-priority', $WorkerPriority)
    }
    if ($CaptureNearOpaque) { $arguments += '--benchmark-capture-near-opaque' }
    if ($CaptureUploads) { $arguments += '--benchmark-capture-uploads' }
    if ($CaptureArenaUploads) { $arguments += '--benchmark-capture-arena-uploads' }
    if ($GpuTimestamps) { $arguments += '--benchmark-gpu-timestamps' }
    if ($GpuStages) { $arguments += '--benchmark-gpu-stages' }
    if ($SurfaceWaits) { $arguments += '--benchmark-surface-waits' }
    if ($PreAcquireVisibility) { $arguments += '--benchmark-pre-acquire-visibility' }
    if ($PackedFrustum) { $arguments += '--benchmark-packed-frustum' }
    if ($CullCoveredLod) { $arguments += '--benchmark-cull-covered-lod' }
    if ($NearOpaqueRuns) { $arguments += '--benchmark-near-opaque-runs' }
    if ($PSBoundParameters.ContainsKey('NearArena')) {
        $arguments += @('--benchmark-near-arena', $NearArena)
    }
    if ($PSBoundParameters.ContainsKey('ReturnAfter')) {
        $arguments += @('--benchmark-streaming-return-after',
            $ReturnAfter.ToString([Globalization.CultureInfo]::InvariantCulture))
    }
    $optionsPath = Join-Path $taskRoot 'options.json'
    $optionsBefore = if (Test-Path -LiteralPath $optionsPath) { (Get-FileHash -LiteralPath $optionsPath -Algorithm SHA256).Hash } else { $null }
    $startedUtc = [DateTime]::UtcNow.ToString('o')
    $timer = [Diagnostics.Stopwatch]::StartNew()
    & $binary @arguments *> (Join-Path $outputPath 'console.log')
    $runExitCode = $LASTEXITCODE
    $timer.Stop()
    $finishedUtc = [DateTime]::UtcNow.ToString('o')
    $optionsAfter = if (Test-Path -LiteralPath $optionsPath) { (Get-FileHash -LiteralPath $optionsPath -Algorithm SHA256).Hash } else { $null }
    if (Test-Path -LiteralPath 'reports\world.json') {
        Copy-Item -LiteralPath 'reports\world.json' -Destination (Join-Path $outputPath 'world.json')
    }
    $entryRunMetadata = [ordered]@{release_sha256=$beforeHash; seed=$Seed; render_distance=$Radius;
        slow_frame_threshold_ms=$SlowFrameMs; slow_frame_threshold_override=$PSBoundParameters.ContainsKey('SlowFrameMs');
        started_utc=$startedUtc; finished_utc=$finishedUtc;
        options_sha256_before=$optionsBefore; options_sha256_after=$optionsAfter;
        options_file_unchanged=($optionsBefore -eq $optionsAfter);
        distant_ready_gate=[bool]$DistantReady; diagnostic_not_baseline=([bool]$CaptureTransparency -or [bool]$CaptureNearOpaque -or [bool]$CaptureUploads -or [bool]$CaptureArenaUploads -or [bool]$GpuTimestamps -or [bool]$SurfaceWaits);
        gpu_timestamps=[bool]$GpuTimestamps;
        gpu_stages=[bool]$GpuStages;
        surface_waits=[bool]$SurfaceWaits;
        pre_acquire_visibility=[bool]$PreAcquireVisibility;
        pre_acquire_visibility_explicit_trial=[bool]$PreAcquireVisibility;
        packed_frustum=[bool]$PackedFrustum;
        packed_frustum_explicit_trial=[bool]$PackedFrustum;
        cull_covered_lod=[bool]$CullCoveredLod;
        cull_covered_lod_explicit_trial=[bool]$CullCoveredLod;
        transparency_upload_override=$TransparencyUpload;
        transparency_upload_explicit_trial=$PSBoundParameters.ContainsKey('TransparencyUpload');
        transparency_draw_override=$TransparencyDraw;
        transparency_draw_explicit_trial=$PSBoundParameters.ContainsKey('TransparencyDraw');
        transparency_sort_override=$TransparencySort;
        worker_priority_override=$WorkerPriority; capture_near_opaque=[bool]$CaptureNearOpaque; capture_uploads=[bool]$CaptureUploads; capture_arena_uploads=[bool]$CaptureArenaUploads;
        near_opaque_runs=[bool]$NearOpaqueRuns;
        near_arena_override=$NearArena;
        near_arena_explicit_trial=($PSBoundParameters.ContainsKey('NearArena') -and $NearArena -ne 'original');
        near_arena_cache_note='Absent NearArena uses guarded automatic distance128/capability admission; Original keeps legacy cached rendering. Explicit dedicated/direct/mdi controls and active automatic arena bypass near caches; consult near_arena.policy for actual selection/latch/fallback.';
        whole_process_seconds=$timer.Elapsed.TotalSeconds;
        exit_code=$runExitCode; arguments=$arguments;
        note='Entry is measured separately from process startup, flight warmup, and far-view completion.'
    }
    if ($slowFrameDiagnostic) { $entryRunMetadata.diagnostic_not_baseline = $true }
    $entryRunMetadata | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $outputPath 'run.json')
    if ($runExitCode -ne 0) { throw "World-entry benchmark failed with exit code $runExitCode. See $outputPath\console.log" }
    $entryPath = Join-Path $outputPath 'world-entry.json'
    if (Test-Path -LiteralPath $entryPath) {
        $entry = Get-Content -LiteralPath $entryPath -Raw | ConvertFrom-Json
        $entry | Select-Object seed,render_distance,request_to_first_play_ms,loaded_chunks,initial_view_pending
    }
} finally {
    Pop-Location
}
