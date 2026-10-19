use std::{
    collections::HashMap,
    env,
    io::Write,
    net::Ipv4Addr,
    process::{Command, ExitCode, Stdio},
};
mod guest;
mod hostnet;
mod hostforward;

const HELP: &str = r#"vnetctl — Parallels 跨系统虚拟机网络管理

vnetctl host-forward list
vnetctl host-forward add --listen 127.0.0.1:18787 --target 10.211.55.18:8787 [--vm NAME]
vnetctl host-forward start|stop|delete|check --listen 127.0.0.1:18787
vnetctl host-forward batch --request '[{"listen":"127.0.0.1:18787","target":"10.211.55.18:8787","vm":"Kali"}]'

vnetctl host-network list
vnetctl host-network preview --request JSON
vnetctl host-network apply --request JSON
vnetctl vms
vnetctl --vm NAME addresses
vnetctl --vm NAME status
vnetctl --vm NAME containers list
vnetctl --vm NAME routes list
vnetctl --vm NAME routes preview --request JSON
vnetctl --vm NAME routes apply --request PLAN_JSON
vnetctl --vm NAME routes rollback
vnetctl --vm NAME routes lookup --request '{"ip":"192.168.10.117"}'
vnetctl --vm NAME services list
vnetctl --vm NAME services batch --request JSON_ARRAY
vnetctl --vm NAME forward delete --listen IP:PORT
vnetctl --vm NAME socks delete --listen IP:PORT
vnetctl --vm NAME forward list
vnetctl --vm NAME forward add --listen IP:PORT --target IP:PORT [--allow CIDR[,CIDR...]] [--dry-run]
vnetctl --vm NAME forward remove --listen IP:PORT [--dry-run]
vnetctl --vm NAME socks start --listen IP:PORT [--allow CIDR[,CIDR...]] [--gost PATH] [--dry-run]
vnetctl --vm NAME socks stop --listen IP:PORT [--dry-run]
vnetctl --vm NAME vpn list
vnetctl --vm NAME vpn connect --name NAME [--dry-run]
vnetctl --vm NAME vpn disconnect --name NAME [--dry-run]

--allow 默认仅允许 Mac 的 Parallels 共享接口 10.211.55.2/32。
来源可用逗号分隔；--allow 0.0.0.0/0 允许所有能够连接监听地址的 IPv4 来源。
支持 Windows、Linux、macOS 来宾；Unix 需要 Python 3.9+。不保存密码，不自动启动虚拟机。
VPN connect 仅使用现有配置及已保存凭据。代理无认证，仅限指定私网来源。
--prlctl PATH 可覆盖 prlctl 路径。所有写操作支持 --dry-run。
"#;

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
fn b64(data: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = ((c[0] as u32) << 16)
            | ((c.get(1).copied().unwrap_or(0) as u32) << 8)
            | c.get(2).copied().unwrap_or(0) as u32;
        for i in 0..4 {
            out.push(if i > c.len() {
                '='
            } else {
                alphabet[((n >> (18 - i * 6)) & 63) as usize] as char
            });
        }
    }
    out
}
fn encoded(s: &str) -> String {
    b64(&s
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>())
}
fn endpoint(s: &str) -> Result<(Ipv4Addr, u16), String> {
    let (ip, p) = s.rsplit_once(':').ok_or("地址格式必须为 IPv4:端口")?;
    let ip = ip.parse::<Ipv4Addr>().map_err(|_| "无效 IPv4 地址")?;
    let p = p.parse::<u16>().map_err(|_| "端口必须在 1..65535")?;
    if p == 0 || ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
        return Err("无效地址或端口".into());
    }
    Ok((ip, p))
}
fn cidr(s: &str) -> Result<String, String> {
    if s.trim() == "0.0.0.0/0" { return Ok("0.0.0.0/0".into()); }
    let mut scopes = Vec::new();
    for part in s.split(',') {
        let scope = single_cidr(part.trim())?;
        if !scopes.contains(&scope) { scopes.push(scope); }
        if scopes.len() > 128 { return Err("最多允许 128 个来源地址 / 网段".into()); }
    }
    Ok(scopes.join(","))
}
fn single_cidr(s: &str) -> Result<String, String> {
    let (ip, bits) = s.split_once('/').ok_or("--allow 必须为私网 IPv4/CIDR")?;
    let ip = ip.parse::<Ipv4Addr>().map_err(|_| "无效来源 IP")?;
    let n = bits.parse::<u32>().map_err(|_| "无效 CIDR")?;
    if !(8..=32).contains(&n) {
        return Err("来源网段过大；请使用私网网段或 /32".into());
    }
    let mask = u32::MAX << (32 - n);
    let first = Ipv4Addr::from(u32::from(ip) & mask);
    let last = Ipv4Addr::from((u32::from(ip) & mask) | !mask);
    if !first.is_private() || !last.is_private() {
        return Err("来源范围必须完全位于 RFC1918 私网".into());
    }
    Ok(format!("{first}/{n}"))
}

#[derive(Clone)]
struct Args {
    words: Vec<String>,
    opts: HashMap<String, String>,
    dry: bool,
}
fn parse(raw: Vec<String>) -> Result<Args, String> {
    let mut a = Args {
        words: vec![],
        opts: HashMap::new(),
        dry: false,
    };
    let mut it = raw.into_iter();
    while let Some(s) = it.next() {
        if s == "--dry-run" {
            a.dry = true;
        } else if s.starts_with("--") {
            if ![
                "--request",
                "--vm",
                "--listen",
                "--target",
                "--allow",
                "--gost",
                "--name",
                "--prlctl",
            ]
            .contains(&s.as_str())
            {
                return Err(format!("未知选项 {s}"));
            }
            let v = it.next().ok_or(format!("{s} 缺少值"))?;
            if v.starts_with("--") || a.opts.insert(s.clone(), v).is_some() {
                return Err(format!("{s} 缺少值或重复"));
            }
        } else {
            a.words.push(s);
        }
    }
    Ok(a)
}
fn need<'a>(a: &'a Args, key: &str) -> Result<&'a str, String> {
    a.opts
        .get(key)
        .map(String::as_str)
        .ok_or(format!("缺少 {key}"))
}
fn rule(kind: &str, ip: Ipv4Addr, p: u16) -> String {
    format!("VNetCtl-{kind}-{ip}-{p}")
}
fn local_check(ip: Ipv4Addr) -> String {
    format!("if (-not (Get-NetIPAddress -AddressFamily IPv4 -IPAddress {} -ErrorAction SilentlyContinue)) {{ throw '监听地址不属于此虚拟机' }};",quote(&ip.to_string()))
}
fn ownership(r: &str) -> String {
    format!("if (-not (Get-NetFirewallRule -Name {} -ErrorAction SilentlyContinue)) {{ throw '不是 vnetctl 创建的配置，拒绝删除' }};",quote(r))
}
fn firewall(r: &str, ip: Ipv4Addr, p: u16, allow: &str) -> String {
    format!("New-NetFirewallRule -Name {} -DisplayName {} -Direction Inbound -Action Allow -Protocol TCP -LocalAddress {ip} -LocalPort {p} -RemoteAddress {} -Profile Any | Out-Null;",quote(r),quote(r),if allow=="0.0.0.0/0"{quote("Any")}else{format!("@({})",allow.split(',').map(quote).collect::<Vec<_>>().join(","))})
}
fn free_check(ip: Ipv4Addr, p: u16, r: &str) -> String {
    format!("{} if(Get-NetFirewallRule -Name {} -ErrorAction SilentlyContinue){{throw '配置已存在，请先查看或删除'}}; if(Get-NetTCPConnection -State Listen -LocalPort {p} -ErrorAction SilentlyContinue){{throw '端口被占用'}}; $existing=netsh interface portproxy show v4tov4; if($existing -match {}){{throw '已有转发使用此地址端口'}};",local_check(ip),quote(r),quote(&format!("(?m)^\\s*{}\\s+{}\\s",ip.to_string().replace('.',"\\."),p)))
}

fn raw_script(a: &Args) -> Result<String, String> {
    let words = a.words.iter().map(String::as_str).collect::<Vec<_>>();
    match words.as_slice(){
        ["services","list"]=>Ok(include_str!("../guest/windows_services.ps1").into()),
        ["addresses"]=>Ok("$rows=@(Get-NetIPAddress -AddressFamily IPv4 | ForEach-Object { [pscustomobject]@{ip=$_.IPAddress;interface=$_.InterfaceAlias;prefix=$_.PrefixLength;state=[string]$_.AddressState} }); ConvertTo-Json -InputObject $rows -Compress".into()),
        ["containers","list"]=>Ok(r#"$rows=@(); $errors=@(); foreach($engine in @('docker','podman')) {if(Get-Command $engine -ErrorAction SilentlyContinue) { $raw=& $engine ps -a --format '{{json .}}' 2>&1; if($LASTEXITCODE -eq 0){foreach($line in $raw){$r=$line|ConvertFrom-Json; $rows += [pscustomobject]@{engine=$engine;name=$r.Names;image=$r.Image;state=$r.State;status=$r.Status;ports=$r.Ports}}}else{$errors+=($engine+': '+($raw -join ' '))}}else{$errors+=($engine+': 未安装')}}; [pscustomobject]@{containers=@($rows);errors=@($errors)} | ConvertTo-Json -Depth 5 -Compress"#.into()),
        ["status"]=>Ok("Get-NetAdapter | Select-Object Name,InterfaceDescription,ifIndex,Status | Format-Table; Get-NetIPAddress -AddressFamily IPv4 | Select-Object InterfaceIndex,IPAddress,PrefixLength | Format-Table; Get-NetRoute -AddressFamily IPv4 | Select-Object DestinationPrefix,NextHop,InterfaceIndex,RouteMetric | Format-Table; Get-VpnConnection -ErrorAction SilentlyContinue | Select-Object Name,ServerAddress,TunnelType,ConnectionStatus | Format-Table; Get-VpnConnection -AllUserConnection -ErrorAction SilentlyContinue | Select-Object Name,ServerAddress,TunnelType,ConnectionStatus | Format-Table; Get-NetFirewallRule -Name 'VNetCtl-*' -ErrorAction SilentlyContinue | Select-Object Name,Enabled | Format-Table; netsh interface portproxy show all;".into()),
        ["forward","list"]=>Ok("netsh interface portproxy show all; if($LASTEXITCODE -ne 0){throw 'netsh failed'}".into()),
        ["vpn","list"]=>Ok("Get-VpnConnection -ErrorAction SilentlyContinue | Select-Object Name,ServerAddress,TunnelType,ConnectionStatus | Format-Table; Get-VpnConnection -AllUserConnection -ErrorAction SilentlyContinue | Select-Object Name,ServerAddress,TunnelType,ConnectionStatus | Format-Table;".into()),
        ["vpn",op @ ("connect"|"disconnect")]=>{
            let name=quote(need(a,"--name")?);
            Ok(format!("rasdial {name} {}; if($LASTEXITCODE -ne 0){{throw 'VPN 操作失败；检查配置及保存的凭据'}};",if *op=="disconnect"{"/disconnect"}else{""}))
        }
        [kind @ ("forward"|"socks"),op]=>{
            let (ip,p)=endpoint(need(a,"--listen")?)?;
            if !ip.is_private(){return Err("监听地址必须为虚拟机的私网 IPv4，禁止监听所有接口".into());}
            let r=rule(kind,ip,p); let rq=quote(&r);
            let allow=cidr(a.opts.get("--allow").map(String::as_str).unwrap_or("10.211.55.2/32"))?;
            match (*kind,*op){
                ("forward","add")=>{
                    let (dst,dp)=endpoint(need(a,"--target")?)?;
                    Ok(format!("{} netsh interface portproxy add v4tov4 listenaddress={ip} listenport={p} connectaddress={dst} connectport={dp}; if($LASTEXITCODE -ne 0){{throw '创建转发失败'}}; try {{ {} Start-Service iphlpsvc; }} catch {{ netsh interface portproxy delete v4tov4 listenaddress={ip} listenport={p} | Out-Null; Remove-NetFirewallRule -Name {rq} -ErrorAction SilentlyContinue; throw }}; Write-Output '已创建 TCP 转发 {ip}:{p} -> {dst}:{dp}';",free_check(ip,p,&r),firewall(&r,ip,p,&allow)))
                }
                ("forward","remove")=>Ok(format!("{} netsh interface portproxy delete v4tov4 listenaddress={ip} listenport={p}; if($LASTEXITCODE -ne 0){{throw '删除转发失败'}}; Remove-NetFirewallRule -Name {rq};",ownership(&r))),
                ("socks","start")=>{
                    let path=quote(a.opts.get("--gost").map(String::as_str).unwrap_or("gost.exe"));
                    Ok(format!("{} $exe=(Get-Command {path} -ErrorAction Stop).Source; {} try {{ $started=Start-Process -FilePath $exe -ArgumentList '-L','socks5://{ip}:{p}' -WindowStyle Hidden -PassThru; $listener=$null; for($i=0;$i -lt 20;$i++){{Start-Sleep -Milliseconds 250; $listener=Get-NetTCPConnection -State Listen -LocalAddress {ip} -LocalPort {p} -ErrorAction SilentlyContinue; if($listener){{break}}}}; if(-not $listener){{throw 'GOST 未成功监听'}}; $proc=Get-Process -Id $listener.OwningProcess; if($proc.ProcessName -ne 'gost'){{throw '端口被其他进程占用'}}; Write-Output ('SOCKS5 {ip}:{p}, PID '+$proc.Id); }} catch {{ if($started){{Stop-Process -Id $started.Id -ErrorAction SilentlyContinue}}; Remove-NetFirewallRule -Name {rq} -ErrorAction SilentlyContinue; throw }};",free_check(ip,p,&r),firewall(&r,ip,p,&allow)))
                }
                ("socks","stop")=>Ok(format!("{} $listener=Get-NetTCPConnection -State Listen -LocalAddress {ip} -LocalPort {p} -ErrorAction SilentlyContinue; if($listener){{ $proc=Get-Process -Id $listener.OwningProcess; if($proc.ProcessName -ne 'gost'){{throw '监听进程不是 GOST，拒绝停止'}}; Stop-Process -Id $proc.Id; }}; Remove-NetFirewallRule -Name {rq};",ownership(&r))),
                _=>Err("未知操作，运行 --help 查看用法".into())
            }
        }
        _=>Err("未知命令，运行 --help 查看用法".into())
    }
}
fn script(a: &Args) -> Result<String, String> {
    let kind = a.words.first().map(String::as_str).unwrap_or("");
    let op = a.words.get(1).map(String::as_str).unwrap_or("");
    if !matches!(kind, "forward" | "socks") || op == "list" {
        return raw_script(a);
    }
    let (ip, port) = endpoint(need(a, "--listen")?)?;
    let key = rule(kind, ip, port);
    let pre=format!("$profileDir=Join-Path $env:LOCALAPPDATA 'VNetStudio-Rules'; $profile=Join-Path $profileDir {}; ",quote(&format!("{key}.json")));
    if op == "delete" {
        let mut stop = a.clone();
        stop.words[1] = if kind == "forward" { "remove" } else { "stop" }.into();
        let body = raw_script(&stop)?;
        return Ok(format!("{pre} if(Get-NetFirewallRule -Name {} -ErrorAction SilentlyContinue){{{body}}} elseif(-not(Test-Path -LiteralPath $profile)){{throw '没有本工具管理的规则'}}; if(Test-Path -LiteralPath $profile){{Remove-Item -LiteralPath $profile}}; Write-Output '规则已删除';",quote(&key)));
    }
    let body = raw_script(a)?;
    if matches!(op, "add" | "start") {
        let cfg = unix_config(a)?;
        let mut cfg = cfg;
        cfg["gost"] = serde_json::json!(a
            .opts
            .get("--gost")
            .map(String::as_str)
            .unwrap_or("gost.exe"));
        Ok(format!("{pre} {body} New-Item -ItemType Directory -Path $profileDir -Force | Out-Null; {} | Set-Content -LiteralPath $profile -Encoding UTF8;",quote(&cfg.to_string())))
    } else {
        // Capture legacy native rules before stopping so that they remain restartable.
        let listing = include_str!("../guest/windows_services.ps1");
        Ok(format!("{pre} if(-not(Get-NetFirewallRule -Name {} -ErrorAction SilentlyContinue)){{if(Test-Path -LiteralPath $profile){{Write-Output '服务已停止';return}};throw '没有本工具管理的规则'}}; $snapshot=(& {{ {listing} }}) | ConvertFrom-Json; $saved=@($snapshot.services | Where-Object {{ $_.kind -eq {} -and $_.ip -eq '{}' -and $_.port -eq {} -and $_.managed }}) | Select-Object -First 1; if(-not $saved){{throw '无法保存现有规则，未停止'}}; if(-not(Test-Path -LiteralPath $profile)){{New-Item -ItemType Directory -Path $profileDir -Force | Out-Null;$saved | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $profile -Encoding UTF8}}; {body}",quote(&key),quote(kind),ip,port))
    }
}
fn batch_requests(a: &Args) -> Result<Vec<Vec<String>>, String> {
    let rows: Vec<Vec<String>> =
        serde_json::from_str(need(a, "--request")?).map_err(|e| format!("无效批量规则：{e}"))?;
    if rows.is_empty() || rows.len() > 128 {
        return Err("每批需要 1 至 128 条规则".into());
    }
    let mut seen = std::collections::HashSet::new();
    for (index, row) in rows.iter().enumerate() {
        let item = parse(row.clone())?;
        if item.dry
            || item
                .opts
                .keys()
                .any(|k| !matches!(k.as_str(), "--listen" | "--target" | "--allow" | "--gost"))
            || !(item.words == ["forward", "add"] || item.words == ["socks", "start"])
        {
            return Err(format!("第 {} 行不是新增转发 / 代理命令", index + 1));
        }
        unix_config(&item)?;
        let endpoint = endpoint(need(&item, "--listen")?)?;
        if !seen.insert(endpoint) {
            return Err(format!("第 {} 行的监听入口重复", index + 1));
        }
    }
    Ok(rows)
}
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn unix_config(a: &Args) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let words = a.words.iter().map(String::as_str).collect::<Vec<_>>();
    match words.as_slice() {
        ["services", "list"] => Ok(json!({"kind":"services","action":"list"})),
        ["addresses"] => Ok(json!({"kind":"addresses","action":"list"})),
        ["status"] => Ok(json!({"kind":"status","action":"list"})),
        ["containers", "list"] => Ok(json!({"kind":"containers","action":"list"})),
        ["forward", "list"] => Ok(json!({"kind":"forward","action":"list"})),
        ["vpn", op @ ("list" | "connect" | "disconnect")] => {
            let name = if *op == "list" {
                ""
            } else {
                need(a, "--name")?
            };
            if name.starts_with('-') {
                return Err("无效 VPN 名称".into());
            }
            Ok(json!({"kind":"vpn","action":op,"name":name}))
        }
        [kind @ ("socks" | "forward"), op] => {
            if !matches!(
                (*kind, *op),
                ("socks", "start" | "stop" | "delete") | ("forward", "add" | "remove" | "delete")
            ) {
                return Err("未知操作".into());
            }
            let (ip, port) = endpoint(need(a, "--listen")?)?;
            if !ip.is_private() {
                return Err("监听地址必须为私网 IP".into());
            }
            let allow = cidr(
                a.opts
                    .get("--allow")
                    .map(String::as_str)
                    .unwrap_or("10.211.55.2/32"),
            )?;
            let target = if *kind == "forward" && *op == "add" {
                let t = need(a, "--target")?;
                endpoint(t)?;
                t
            } else {
                ""
            };
            Ok(
                json!({"kind":kind,"action":op,"ip":ip.to_string(),"port":port,"allow":allow,"target":target}),
            )
        }
        _ => Err("未知命令".into()),
    }
}
fn route_config(a: &Args) -> Result<serde_json::Value, String> {
    let op = a.words.get(1).map(String::as_str).unwrap_or("");
    if !["list", "preview", "apply", "rollback", "lookup"].contains(&op) {
        return Err("未知路由操作".into());
    }
    if a.dry {
        return Err("路由操作请使用 routes preview 读取实时状态并生成计划".into());
    }
    let request = if ["list", "rollback"].contains(&op) {
        serde_json::json!({})
    } else {
        serde_json::from_str(need(a, "--request")?).map_err(|e| format!("请求 JSON 无效：{e}"))?
    };
    Ok(serde_json::json!({"action":op,"request":request}))
}
fn run() -> Result<(), String> {
    let raw = env::args().skip(1).collect::<Vec<_>>();
    if raw.is_empty() || raw.iter().any(|s| s == "--help" || s == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    let a = parse(raw)?;
    if a.words.first().map(String::as_str)==Some("host-forward") {
        let op=a.words.get(1).map(String::as_str).ok_or("缺少 Mac 转发操作")?;
        if op=="serve" {return hostforward::serve();}
        if op=="restart-backend" {println!("{}",hostforward::restart_idle()?);return Ok(());}
        if a.dry {return Err("Mac 本地转发不使用 --dry-run；check 可检查已保存规则".into());}
        let request=match op {
            "list"=>serde_json::json!({"op":"list"}),
            "batch"=>serde_json::json!({"op":"batch","rules":serde_json::from_str::<serde_json::Value>(need(&a,"--request")?).map_err(|e|e.to_string())?}),
            "add"=>serde_json::json!({"op":"add","config":{"listen":need(&a,"--listen")?,"target":need(&a,"--target")?,"vm":a.opts.get("--vm").map(String::as_str).unwrap_or("")}}),
            "start"|"stop"|"delete"|"check"=>serde_json::json!({"op":op,"listen":need(&a,"--listen")?}),
            _=>return Err("未知 Mac 转发命令".into()),
        };
        let result=hostforward::request(request)?;
        let failed=result["ok"]==false;
        println!("{result}");
        return if failed {Err("部分 Mac 规则未完成；已成功规则保留".into())}else{Ok(())};
    }
    let bin = a
        .opts
        .get("--prlctl")
        .map(String::as_str)
        .unwrap_or("prlctl");
    if a.words.first().map(String::as_str) == Some("host-network") {
        let purpose = match a.words.get(1).map(String::as_str) {
            Some("list") => "host-read",
            Some("preview") => "host-preview",
            Some("apply") => "host-apply",
            _ => return Err("未知宿主网络命令".into()),
        };
        if a.dry && purpose == "host-apply" {
            return Err("apply 不支持 --dry-run，请使用 preview".into());
        }
        let args = if purpose == "host-read" {
            vec![]
        } else {
            vec![need(&a, "--request")?.to_string()]
        };
        println!("{}", hostnet::handle(bin, purpose, &args)?);
        return Ok(());
    }
    if a.words == ["vms"] {
        let status = Command::new(bin)
            .args(["list", "-a"])
            .status()
            .map_err(|e| format!("无法运行 prlctl: {e}"))?;
        return if status.success() {
            Ok(())
        } else {
            Err("列出虚拟机失败".into())
        };
    }
    let vm = need(&a, "--vm")?;
    if vm.starts_with('-') || vm.contains(['\n', '\r']) {
        return Err("无效虚拟机名称".into());
    }
    let meta = Command::new(bin)
        .args(["list", "-i", "-j", vm])
        .output()
        .map_err(|e| format!("无法读取虚拟机信息：{e}"))?;
    if !meta.status.success() {
        return Err(format!(
            "读取虚拟机信息失败：{}",
            String::from_utf8_lossy(&meta.stderr)
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&meta.stdout)
        .map_err(|e| format!("无法识别 Parallels 元数据：{e}"))?;
    let value = if value.is_array() {
        value.get(0).ok_or("未找到虚拟机")?
    } else {
        &value
    };
    let guest = guest::Guest::parse(value);
    if a.words == ["status"] {
        println!("{}\n", guest.summary());
    }
    guest.check()?;
    if a.words == ["services", "batch"] {
        let rows = batch_requests(&a)?;
        // Validate every script before making the first change.
        if guest.windows() {
            for row in &rows {
                script(&parse(row.clone())?)?;
            }
        }
        if a.dry {
            println!("{}", serde_json::json!({"validated":rows.len()}));
            return Ok(());
        }
        let mut results = vec![];
        let mut failed = false;
        for (index, row) in rows.iter().enumerate() {
            if failed {
                results.push(serde_json::json!({"row":index+1,"state":"未执行"}));
                continue;
            }
            let mut args = vec![
                "--vm".to_string(),
                vm.to_string(),
                "--prlctl".into(),
                bin.to_string(),
            ];
            args.extend(row.clone());
            let result = hostnet::command(
                &env::current_exe()
                    .map_err(|e| e.to_string())?
                    .to_string_lossy(),
                &args,
            );
            failed = result.is_err();
            results.push(serde_json::json!({"row":index+1,"command":row,"state":if failed{"失败"}else{"已开启"},"detail":result.unwrap_or_else(|e|e)}));
        }
        println!("{}", serde_json::json!({"results":results,"ok":!failed}));
        return if failed {
            Err("部分规则未完成；已成功规则保留，请按逐行结果处理".into())
        } else {
            Ok(())
        };
    }

    let mut command = Command::new(bin);
    let mut input: Option<String> = None;
    command.args(["exec", vm]);
    if guest.windows() {
        let body = if a.words.first().map(String::as_str) == Some("routes") {
            let cfg = route_config(&a)?;
            format!(
                "$cfg={} | ConvertFrom-Json;\n{}",
                quote(&cfg.to_string()),
                include_str!("../guest/routes.ps1")
            )
        } else {
            script(&a)?
        };
        if a.dry {
            println!("虚拟机: {vm}\n预览（未执行）:\n{body}");
            return Ok(());
        }
        let wrapped=format!("$ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.Encoding]::UTF8; try {{ {body} }} catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}");
        command.args([
            "powershell.exe",
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded(&wrapped),
        ]);
    } else {
        let routing = a.words.first().map(String::as_str) == Some("routes");
        let cfg = if routing {
            route_config(&a)?
        } else {
            unix_config(&a)?
        };
        if a.dry {
            println!("虚拟机: {vm}\n系统: {}\n预览（未执行）:\n{}\n使用来宾 Python 3.9+；限制客户端来源，不修改系统防火墙。",guest.os,serde_json::to_string_pretty(&cfg).unwrap());
            return Ok(());
        }
        let program = if routing {
            include_str!("../guest/routes.py")
        } else {
            include_str!("../guest/unix_agent.py")
        };
        let source = b64(program.as_bytes());
        input = Some(format!("SOURCE_B64={}\n{}", quote(&source), program));
        command.args([
            "/usr/bin/env",
            "PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
            "python3",
            "-",
            &sh_quote(&cfg.to_string()),
        ]);
    }
    let output = if let Some(input) = input {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut pipe = child.stdin.take().ok_or("无法打开输入管道")?;
        pipe.write_all(input.as_bytes())
            .map_err(|e| e.to_string())?;
        drop(pipe);
        child.wait_with_output().map_err(|e| e.to_string())?
    } else {
        command
            .output()
            .map_err(|e| format!("无法运行 prlctl: {e}"))?
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{stdout}");
    eprint!("{stderr}");
    if output.status.success() {
        Ok(())
    } else {
        if format!("{stdout}{stderr}").contains("Unable to open new session") {
            return Err(format!("Parallels 无法建立来宾执行会话。检测到 Tools 状态为 {}（不等于服务已运行）。请在来宾系统内检查 Parallels Tools 服务或重新安装 Tools，等待系统启动完成，并检查是否启用了与 Mac 隔离。此错误发生在执行网络操作之前。",guest.tools));
        }
        Err(format!(
            "虚拟机命令失败（退出码 {}）；未验证成功",
            output
                .status
                .code()
                .map(|x| x.to_string())
                .unwrap_or_else(|| "被信号终止".into())
        ))
    }
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_vectors() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(encoded("A"), "QQA=");
    }
    #[test]
    fn endpoint_validation() {
        assert!(endpoint("10.0.0.1:0").is_err());
        assert!(endpoint("0.0.0.0:80").is_err());
        assert!(endpoint("10.0.0.1:65536").is_err());
        assert!(endpoint("10.0.0.1:1080").is_ok());
    }
    #[test]
    fn private_range_boundary() {
        assert!(cidr("10.0.0.1/7").is_err());
        assert!(cidr("172.16.0.0/8").is_err());
        assert_eq!(cidr("10.211.55.17/24").unwrap(), "10.211.55.0/24");
        assert!(cidr("192.168.0.0/16").is_ok());
    }
    #[test]
    fn source_union_validation_and_windows_array() {
        let allow=cidr("10.211.55.2/32, 10.211.55.18/32,10.211.55.2/32").unwrap();
        assert_eq!(allow,"10.211.55.2/32,10.211.55.18/32");
        for bad in ["",",","10.211.55.2/32,", "10.211.55.2/32,invalid", "0.0.0.0/0,10.0.0.1/32"] { assert!(cidr(bad).is_err()); }
        assert_eq!(cidr("0.0.0.0/0").unwrap(),"0.0.0.0/0");
        assert!(firewall("test","10.0.0.1".parse().unwrap(),8080,&allow).contains("-RemoteAddress @('10.211.55.2/32','10.211.55.18/32')"));
        assert!(firewall("test","10.0.0.1".parse().unwrap(),8080,"0.0.0.0/0").contains("-RemoteAddress 'Any'"));
    }
    #[test]
    fn literal_quote() {
        assert_eq!(quote("a'; Stop-Process x; '"), "'a''; Stop-Process x; '''");
    }
    #[test]
    fn removal_requires_ownership() {
        let a = parse(
            ["forward", "remove", "--listen", "10.211.55.17:18117"]
                .map(str::to_string)
                .to_vec(),
        )
        .unwrap();
        let s = script(&a).unwrap();
        assert!(s.find("Get-NetFirewallRule").unwrap() < s.find("portproxy delete").unwrap());
    }
    #[test]
    fn add_rolls_back() {
        let a = parse(
            [
                "forward",
                "add",
                "--listen",
                "10.211.55.17:18117",
                "--target",
                "192.168.10.117:80",
            ]
            .map(str::to_string)
            .to_vec(),
        )
        .unwrap();
        let s = script(&a).unwrap();
        assert!(s.contains("catch { netsh interface portproxy delete"));
        assert!(s.contains("10.211.55.2/32"));
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    fn batch(rows: serde_json::Value) -> Args {
        parse(vec![
            "services".into(),
            "batch".into(),
            "--request".into(),
            rows.to_string(),
        ])
        .unwrap()
    }
    #[test]
    fn all_rows_validated_and_duplicate_listeners_rejected() {
        let valid = serde_json::json!([
            "forward",
            "add",
            "--listen",
            "10.0.0.1:8080",
            "--target",
            "127.0.0.1:80"
        ]);
        assert!(batch_requests(&batch(serde_json::json!([
            valid.clone(),
            ["socks", "start", "--listen", "10.0.0.1:1080"]
        ])))
        .is_ok());
        assert!(batch_requests(&batch(serde_json::json!([
            valid.clone(),
            ["socks", "start", "--listen", "10.0.0.1:8080"]
        ])))
        .unwrap_err()
        .contains("重复"));
        assert!(batch_requests(&batch(serde_json::json!([
            valid,
            [
                "forward",
                "add",
                "--listen",
                "10.0.0.1:8081",
                "--target",
                "broken"
            ]
        ])))
        .is_err());
    }
    #[test]
    fn batch_cannot_change_guest_or_run_other_commands() {
        for row in [
            serde_json::json!([
                "forward",
                "add",
                "--listen",
                "10.0.0.1:8080",
                "--target",
                "127.0.0.1:80",
                "--vm",
                "other"
            ]),
            serde_json::json!(["vpn", "connect", "--name", "x"]),
        ] {
            assert!(batch_requests(&batch(serde_json::json!([row]))).is_err());
        }
        assert!(batch_requests(&batch(serde_json::json!([]))).is_err());
    }
    #[test]
    fn delete_supported_without_target_and_still_requires_private_listener() {
        let a = parse(vec![
            "forward".into(),
            "delete".into(),
            "--listen".into(),
            "10.0.0.1:8080".into(),
        ])
        .unwrap();
        assert_eq!(unix_config(&a).unwrap()["action"], "delete");
        assert!(script(&a).unwrap().contains("没有本工具管理的规则"));
        let mut bad = a;
        bad.opts.insert("--listen".into(), "0.0.0.0:8080".into());
        assert!(unix_config(&bad).is_err());
        assert!(script(&bad).is_err());
    }
}
