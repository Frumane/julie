# Julie ve Floorp'un o anki bellek ve işlemci kullanımını ölçer.
# Kullanım (proje klasöründe):
#   powershell -ExecutionPolicy Bypass -File tools\olc.ps1
# Hangi tarayıcı açıksa onu ölçer. 10 saniye sürer.
# Süreç süreç döküm için sonuna -Detay ekle:
#   powershell -ExecutionPolicy Bypass -File tools\olc.ps1 -Detay

param([switch]$Detay)

function Get-BrowserIds($browser) {
  $all = Get-CimInstance Win32_Process
  if ($browser -eq 'Julie') {
    $procs = $all | Where-Object { $_.Name -eq 'julie.exe' -or ($_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -like '*\Julie\*') }
  } else {
    $procs = $all | Where-Object { $_.Name -eq 'floorp.exe' }
  }
  @($procs | ForEach-Object { $_.ProcessId })
}

$seconds = 10
$found = @{}
foreach ($b in 'Julie', 'Floorp') {
  $ids = Get-BrowserIds $b
  if ($ids.Count -gt 0) {
    $found[$b] = @{ Ids = $ids; Cpu = (Get-Process -Id $ids -ErrorAction SilentlyContinue | Measure-Object CPU -Sum).Sum }
  }
}
if ($found.Count -eq 0) {
  Write-Host "Ne Julie ne Floorp açık."
  exit
}
Write-Host "Ölçülüyor, $seconds saniye bekle..."
Start-Sleep $seconds
$perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process
foreach ($b in $found.Keys) {
  $ids = $found[$b].Ids
  $ram = ($perf | Where-Object { $ids -contains $_.IDProcess } | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB
  $cpu2 = (Get-Process -Id $ids -ErrorAction SilentlyContinue | Measure-Object CPU -Sum).Sum
  $cpu = ($cpu2 - $found[$b].Cpu) / $seconds / [Environment]::ProcessorCount * 100
  Write-Host ("{0}: bellek {1:N0} MB | işlemci %{2:N1} | {3} süreç" -f $b, $ram, $cpu, $ids.Count)
  if ($Detay) {
    $cmd = @{}
    Get-CimInstance Win32_Process | Where-Object { $ids -contains $_.ProcessId } | ForEach-Object { $cmd[[int]$_.ProcessId] = "$($_.Name) $($_.CommandLine)" }
    $rows = $perf | Where-Object { $ids -contains $_.IDProcess } | ForEach-Object {
      $line = $cmd[[int]$_.IDProcess]
      $kind = if ($line -match '^julie\.exe') { 'Julie arayüzü + reklam engelleyici' }
        elseif ($line -match '^floorp\.exe' -and $line -notmatch '-contentproc') { 'ana süreç' }
        elseif ($line -match '--utility-sub-type=([\w\.]+)') { 'yardımcı: ' + $Matches[1] }
        elseif ($line -match '--type=([\w-]+)') { $Matches[1] }
        elseif ($line -match '-contentproc.*\s(tab|gpu|socket|rdd|utility|forkserver|web|webIsolated=\S+|privilegedmozilla)\b') { $Matches[1] }
        else { 'tarayıcı çekirdeği' }
      [pscustomobject]@{ MB = [math]::Round($_.WorkingSetPrivate / 1MB); Tur = $kind }
    }
    $rows | Sort-Object MB -Descending | Format-Table -AutoSize | Out-String | Write-Host
  }
}
