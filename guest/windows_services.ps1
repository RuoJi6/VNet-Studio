function Read-VNetAllow($name) {
    $rule = @(Get-NetFirewallRule -Name $name -ErrorAction SilentlyContinue | Where-Object { $_.Enabled -eq 'True' -and $_.Action -eq 'Allow' })
    if ($rule.Count -ne 1) { return '' }
    $address = @($rule[0] | Get-NetFirewallAddressFilter -ErrorAction SilentlyContinue | Select-Object -ExpandProperty RemoteAddress)
    $scopes = @()
    foreach($entry in $address) {
        $a = [string]$entry
        if($a -eq 'Any') { return '0.0.0.0/0' }
        if($a -match '^\d+\.\d+\.\d+\.\d+$') { $a += '/32' }
        $scopes += $a
    }
    return ($scopes -join ',')
}
$rows = @()
$errors = @()
$connections = @(Get-NetTCPConnection -ErrorAction Stop)
$rules = @(Get-NetFirewallRule -Name 'VNetCtl-*' -ErrorAction SilentlyContinue)
$raw = @(netsh interface portproxy show v4tov4)
if ($LASTEXITCODE -ne 0) { throw '读取端口转发失败' }
foreach ($line in $raw) {
    if ($line -match '^\s*(\d+\.\d+\.\d+\.\d+)\s+(\d+)\s+(\d+\.\d+\.\d+\.\d+)\s+(\d+)\s*$') {
        $ip = $Matches[1]; $port = [int]$Matches[2]; $target = $Matches[3]+':'+$Matches[4]
        $name = 'VNetCtl-forward-'+$ip+'-'+$port
        $managed = @($rules | Where-Object Name -eq $name).Count -gt 0
        $local = @($connections | Where-Object { $_.LocalPort -eq $port -and ($_.LocalAddress -eq $ip -or $_.LocalAddress -eq '0.0.0.0') })
        $peers = @($local | Where-Object State -ne Listen | ForEach-Object { [pscustomobject]@{time='当前快照';event=[string]$_.State;peer=($_.RemoteAddress+':'+$_.RemotePort);target=$target} })
        $rows += [pscustomobject]@{kind='forward';ip=$ip;port=$port;target=$target;allow=(Read-VNetAllow $name);managed=$managed;running=(@($local | Where-Object State -eq Listen).Count -gt 0);telemetry='snapshot';events=$peers;stats=@{active=$peers.Count}}
    }
}
foreach ($r in $rules) {
    if ($r.Name -match '^VNetCtl-socks-(\d+\.\d+\.\d+\.\d+)-(\d+)$') {
        $ip=$Matches[1].Replace('_','.');$port=[int]$Matches[2]
        $local=@($connections | Where-Object { $_.LocalPort -eq $port -and $_.LocalAddress -eq $ip })
        $peers=@($local | Where-Object State -ne Listen | ForEach-Object {[pscustomobject]@{time='当前快照';event=[string]$_.State;peer=($_.RemoteAddress+':'+$_.RemotePort);target='SOCKS5'}})
        $rows += [pscustomobject]@{kind='socks';ip=$ip;port=$port;target='SOCKS5';allow=(Read-VNetAllow $r.Name);managed=$true;running=(@($local | Where-Object State -eq Listen).Count -gt 0);telemetry='snapshot';events=$peers;stats=@{active=$peers.Count}}
    }
}
# Retain stopped owned rules; never infer that a foreign listener is ours.
$profileDir=Join-Path $env:LOCALAPPDATA 'VNetStudio-Rules'
if(Test-Path -LiteralPath $profileDir){
    foreach($file in @(Get-ChildItem -LiteralPath $profileDir -Filter 'VNetCtl-*.json')){
        try {
            $saved=Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
            $existing=@($rows | Where-Object {$_.kind -eq $saved.kind -and $_.ip -eq $saved.ip -and $_.port -eq $saved.port})
            if($existing.Count -gt 0){
                if($saved.gost){$existing[0] | Add-Member -NotePropertyName gost -NotePropertyValue $saved.gost -Force}
                continue
            }
            $rows += [pscustomobject]@{kind=$saved.kind;ip=$saved.ip;port=$saved.port;target=$saved.target;allow=$saved.allow;gost=$saved.gost;managed=$true;running=$false;status='stopped';telemetry='snapshot';events=@();stats=@{active=0}}
        } catch {$errors += ('读取保存规则失败：'+$file.Name)}
    }
}
[pscustomobject]@{services=@($rows);note='Windows 原生 portproxy / GOST：显示配置、监听和当前连接快照；无历史通信日志与字节统计。';errors=@($errors)} | ConvertTo-Json -Depth 8 -Compress
