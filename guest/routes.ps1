# $cfg is supplied as JSON by the Rust controller. Never evaluate request text as code.
$root = Join-Path $env:LOCALAPPDATA 'VNetStudio'
$record = Join-Path $root 'routes-last.json'
function Get-Snapshot {
    $ifs=@(Get-NetIPInterface -AddressFamily IPv4 | ForEach-Object {
        $idx=$_.InterfaceIndex
        $ips=@(Get-NetIPAddress -AddressFamily IPv4 -InterfaceIndex $idx -ErrorAction SilentlyContinue | ForEach-Object { $_.IPAddress+'/'+$_.PrefixLength })
        [pscustomobject]@{id=[string]$idx;name=$_.InterfaceAlias;ips=$ips;state=[string]$_.ConnectionState;metric=$_.InterfaceMetric}
    } | Sort-Object id)
    $routes=@(Get-NetRoute -AddressFamily IPv4 -PolicyStore ActiveStore | ForEach-Object {
        [pscustomobject]@{destination=$_.DestinationPrefix;gateway=$(if($_.NextHop -eq '0.0.0.0'){''}else{$_.NextHop});interface=[string]$_.InterfaceIndex;metric=$_.RouteMetric;protocol=[string]$_.Protocol;protected=([string]$_.Protocol -eq 'Local')}
    } | Sort-Object destination,interface,gateway,metric)
    $identity=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToString('o')
    $bytes=[Text.Encoding]::UTF8.GetBytes((@($identity,$ifs,$routes)|ConvertTo-Json -Depth 10 -Compress))
    $hash=([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($bytes))).Replace('-','')
    $dns=(Get-DnsClientServerAddress -AddressFamily IPv4 | Format-Table InterfaceAlias,ServerAddresses | Out-String)
    return [pscustomobject]@{epoch=$identity;platform='Windows';interfaces=$ifs;routes=$routes;dns=$dns;warnings=@('路由优先级同时受接口 metric、VPN 及策略影响；请使用实际出口查询核对。');fingerprint=$hash;can_rollback=(Test-Path $record)}
}
function IP-Number([string]$value) {
    $ip=[Net.IPAddress]::Parse($value)
    if($ip.AddressFamily -ne [Net.Sockets.AddressFamily]::InterNetwork -or $value -notmatch '^\d+\.\d+\.\d+\.\d+$'){throw '仅支持点分 IPv4 地址'}
    $b=$ip.GetAddressBytes()
    return [uint64]$b[0]*16777216+[uint64]$b[1]*65536+[uint64]$b[2]*256+[uint64]$b[3]
}
function Parse-Prefix([string]$value) {
    $parts=$value.Split('/');if($parts.Count -gt 2){throw '无效网段'}
    $bits=32;if($parts.Count -eq 2){$bits=[int]$parts[1]};if($bits -lt 0 -or $bits -gt 32){throw '前缀须为 0..32'}
    $ip=IP-Number $parts[0];$size=[math]::Pow(2,32-$bits);$first=[uint64]([math]::Floor($ip/$size)*$size)
    $text=('{0}.{1}.{2}.{3}/{4}' -f (($first -shr 24)-band 255),(($first -shr 16)-band 255),(($first -shr 8)-band 255),($first -band 255),$bits)
    return [pscustomobject]@{text=$text;first=$first;last=($first+$size-1);bits=$bits}
}
function Exact-Rows($s,$dst){return @($s.routes | Where-Object destination -eq $dst)}
function New-Plan($req,$s) {
    if($req.op -notin @('default','add','delete')){throw '未知路由操作'}
    $dst=if($req.op -eq 'default'){'0.0.0.0/0'}else{(Parse-Prefix $req.destination).text}
    $net=Parse-Prefix $dst
    if($dst -eq '0.0.0.0/0' -and $req.op -ne 'default'){throw '请使用默认出口功能'}
    if($net.bits -gt 0 -and ($net.first -eq 0 -or ($net.first -shr 24) -eq 127 -or ($net.first -shr 24) -ge 224)){throw '不支持该目标网段'}
    $old=@(Exact-Rows $s $dst)
    if(@($old | Where-Object protected).Count){throw '不能修改系统直连路由'}
    if($req.op -eq 'add' -and $old.Count){throw '该网段已有路由；请先查看并删除精确匹配的路由'}
    if($req.op -eq 'delete' -and -not $old.Count){throw '没有精确匹配的路由'}
    $new=@()
    if($req.op -ne 'delete'){
        $i=$s.interfaces|Where-Object id -eq $req.interface|Select-Object -First 1
        if(-not $i -or -not $i.ips.Count -or $i.state -ne 'Connected' -or $i.name -like '*Loopback*'){throw '所选网卡未连接或没有 IPv4 地址'}
        $gw=([string]$req.gateway).Trim();$valid=$false
        if($gw){
            $num=IP-Number $gw
            if($num -eq 0 -or ($num -shr 24) -eq 127 -or ($num -shr 24) -ge 224){throw '网关无效'}
            foreach($addr in $i.ips){$n=Parse-Prefix $addr;if($num -ge $n.first -and $num -le $n.last -and $num -ne (IP-Number ($addr.Split('/')[0]))){$valid=$true}}
        } else {
            if($req.op -eq 'default'){throw '默认出口需要网关'}
            foreach($addr in $i.ips){$n=Parse-Prefix $addr;if($net.first -ge $n.first -and $net.last -le $n.last){$valid=$true}}
        }
        if(-not $valid){throw '网关必须位于所选网卡直连网段；非直连目标必须填写下一跳'}
        $new=@([pscustomobject]@{destination=$dst;gateway=$gw;interface=$i.id;metric=0;protocol='NetMgmt';protected=$false})
    }
    return [pscustomobject]@{epoch=$s.epoch;request=$req;fingerprint=$s.fingerprint;destination=$dst;before=$old;after=$new;description=('路由操作 '+$req.op+'：'+$dst);note='仅 ActiveStore 当前会话生效；不修改 DNS、IPv6、DHCP 或持久化配置，重连可能恢复。'}
}
function Route-Signature($rows){return (@($rows | ForEach-Object { '{0}|{1}|{2}|{3}' -f $_.destination,$_.gateway,$_.interface,$_.metric } | Sort-Object)|ConvertTo-Json -Compress)}
function Change-Route($op,$r){
    $gw=if($r.gateway){$r.gateway}else{'0.0.0.0'}
    if($op -eq 'add'){
        New-NetRoute -AddressFamily IPv4 -DestinationPrefix $r.destination -InterfaceIndex ([int]$r.interface) -NextHop $gw -RouteMetric ([uint16]$r.metric) -Protocol $r.protocol -PolicyStore ActiveStore -ErrorAction Stop | Out-Null
    }else{
        Get-NetRoute -AddressFamily IPv4 -DestinationPrefix $r.destination -InterfaceIndex ([int]$r.interface) -NextHop $gw -PolicyStore ActiveStore -ErrorAction Stop | Where-Object RouteMetric -eq $r.metric | Remove-NetRoute -Confirm:$false -ErrorAction Stop
    }
}
$s=Get-Snapshot
switch($cfg.action){
    'list' { $s|ConvertTo-Json -Depth 12 -Compress;return }
    'lookup' { $ip=[string]$cfg.request.ip;$null=IP-Number $ip;[pscustomobject]@{ip=$ip;result=(Find-NetRoute -RemoteIPAddress $ip|Format-List|Out-String);note='查询内核 IPv4 出口，不向目标发包。'}|ConvertTo-Json -Compress;return }
    'preview' { New-Plan $cfg.request $s|ConvertTo-Json -Depth 12 -Compress;return }
    'apply' { if($cfg.request.fingerprint -ne $s.fingerprint){throw '网卡或路由变化，请重新预览'};$p=New-Plan $cfg.request.request $s }
    'rollback' {
        if(-not(Test-Path $record)){throw '没有可撤销变更'}
        $last=Get-Content -Raw $record|ConvertFrom-Json
        if($last.epoch -ne $s.epoch){throw '系统已重启，拒绝恢复上次启动的路由'}
        if((Route-Signature (Exact-Rows $s $last.destination)) -ne (Route-Signature $last.after)){throw '相关路由已变化，拒绝自动撤销'}
        $p=[pscustomobject]@{destination=$last.destination;before=@(Exact-Rows $s $last.destination);after=@($last.before)}
    }
    default {throw '未知路由操作'}
}
if(-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw '需要来宾管理员权限'}
New-Item -ItemType Directory -Force $root|Out-Null
$p|ConvertTo-Json -Depth 12|Set-Content -Encoding UTF8 ($record+'.pending')
$removed=@();$added=@()
try {
    foreach($r in $p.before){Change-Route 'delete' $r;$removed+=,$r}
    foreach($r in $p.after){Change-Route 'add' $r;$added+=,$r}
}catch{
    $original=$_.Exception.Message;$rollbackErrors=@()
    foreach($r in $added){try{Change-Route 'delete' $r}catch{$rollbackErrors+=$_.Exception.Message}}
    foreach($r in $removed){try{Change-Route 'add' $r}catch{$rollbackErrors+=$_.Exception.Message}}
    throw ($original+'；回滚：'+$(if($rollbackErrors.Count){$rollbackErrors -join '; '}else{'成功'}))
}
$post=Get-Snapshot
if((Route-Signature (Exact-Rows $post $p.destination)) -ne (Route-Signature $p.after)){throw ('应用后验证不一致，请刷新；恢复记录：'+$record+'.pending')}
if($cfg.action -eq 'apply'){$p|ConvertTo-Json -Depth 12|Set-Content -Encoding UTF8 $record}else{Remove-Item $record}
Remove-Item ($record+'.pending')
$post.can_rollback=Test-Path $record
[pscustomobject]@{result='已完成并核对路由';snapshot=$post}|ConvertTo-Json -Depth 12 -Compress
