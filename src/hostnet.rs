//! Host-side Parallels networking. Commands are argv arrays, never shell strings.
use serde_json::{json, Value};
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn command(bin: &str, args: &[String]) -> Result<String, String> {
    command_with_timeout(bin,args,Duration::from_secs(30))
}
pub fn command_with_timeout(bin:&str,args:&[String],timeout:Duration)->Result<String,String>{
    let mut child = Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("命令超时；操作结果未确认，请刷新状态后再操作".into());
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    if status.success() {
        Ok(out)
    } else {
        Err(format!("{bin} {}\n{out}{err}", args.join(" ")))
    }
}
fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}
fn read_json(bin: &str, args: &[&str]) -> Result<Value, String> {
    serde_json::from_str(&command(bin, &strings(args))?).map_err(|e| e.to_string())
}
fn srv(prl: &str) -> String {
    std::path::Path::new(prl)
        .with_file_name("prlsrvctl")
        .to_string_lossy()
        .into_owned()
}
pub fn field<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
pub fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub fn net_id(s: &str) -> bool {
    s.strip_prefix("net")
        .map(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or(false)
}
pub fn adapters(vm: &Value) -> Vec<(String, Value)> {
    vm.get("Hardware")
        .and_then(Value::as_object)
        .map(|h| {
            h.iter()
                .filter(|(k, _)| net_id(k))
                .map(|(k, v)| {
                    let mut v = v.clone();
                    if field(&v, "type") == "host" {
                        v["type"] = json!("host-only");
                    }
                    (k.clone(), v)
                })
                .collect()
        })
        .unwrap_or_default()
}
pub fn bridge_interface(s: &Value, a: &Value) -> String {
    let raw = field(a, "iface");
    array(&s["interfaces"])
        .iter()
        .find(|i| field(i, "device") == raw || field(i, "name") == raw)
        .map(|i| field(i, "device").to_string())
        .unwrap_or_else(|| raw.to_string())
}
pub fn parse_interfaces(hardware: &str, ifconfig: &str) -> Value {
    let mut names = std::collections::BTreeMap::new();
    let mut name = "";
    for line in hardware.lines() {
        if let Some(s) = line.strip_prefix("Hardware Port: ") {
            name = s;
        }
        if let Some(s) = line.strip_prefix("Device: ") {
            names.insert(s.to_string(), name.to_string());
        }
    }
    let mut rows = vec![];
    for block in ifconfig
        .split('\n')
        .fold(Vec::<String>::new(), |mut blocks, line| {
            if !line.starts_with(char::is_whitespace) && line.contains(": flags=") {
                blocks.push(line.into());
            } else if let Some(last) = blocks.last_mut() {
                last.push('\n');
                last.push_str(line);
            }
            blocks
        })
    {
        let dev = block.split(':').next().unwrap_or("");
        let ips = block
            .lines()
            .filter_map(|l| {
                l.trim()
                    .strip_prefix("inet ")
                    .and_then(|s| s.split_whitespace().next())
            })
            .collect::<Vec<_>>();
        let state = block
            .lines()
            .find_map(|l| l.trim().strip_prefix("status: "))
            .unwrap_or("unknown");
        if names.contains_key(dev) || !ips.is_empty() {
            rows.push(json!({"device":dev,"name":names.get(dev).map(String::as_str).unwrap_or(dev),"ips":ips,"state":state}));
        }
    }
    Value::Array(rows)
}
pub fn snapshot(prl: &str) -> Result<Value, String> {
    let vms = read_json(prl, &["list", "-a", "-i", "-j"])?;
    let mut errors = vec![];
    let mut optional = |bin: &str, args: &[&str]| match read_json(bin, args) {
        Ok(v) => v,
        Err(e) => {
            errors.push(e);
            json!([])
        }
    };
    let usb = optional(&srv(prl), &["usb", "list", "-a", "-j"]);
    let networks = optional(&srv(prl), &["net", "list", "-j"]);
    let hardware = command(
        "/usr/sbin/networksetup",
        &strings(&["-listallhardwareports"]),
    )
    .unwrap_or_else(|e| {
        errors.push(e);
        String::new()
    });
    let interfaces = command("/sbin/ifconfig", &strings(&["-a"]))
        .map(|s| parse_interfaces(&hardware, &s))
        .unwrap_or_else(|e| {
            errors.push(e);
            json!([])
        });
    // Only retain network metadata, not unrelated VM configuration.
    let vms:Vec<Value>=array(&vms).iter().map(|v|json!({"ID":v["ID"],"Name":v["Name"],"OS":v["OS"],"State":v["State"],"GuestTools":v["GuestTools"],"Network":v["Network"],"Hardware":adapters(v).into_iter().collect::<serde_json::Map<_,_>>()})).collect();
    Ok(json!({"vms":vms,"usb":usb,"networks":networks,"interfaces":interfaces,"errors":errors}))
}
fn vm<'a>(snapshot: &'a Value, id: &str) -> Result<&'a Value, String> {
    array(&snapshot["vms"])
        .iter()
        .find(|v| field(v, "ID") == id)
        .ok_or("虚拟机已不存在，请刷新".into())
}
pub fn plan(s: &Value, r: &Value) -> Result<Value, String> {
    let v = vm(s, field(r, "vm"))?;
    let id = field(v, "ID");
    let state = field(v, "State");
    let op = field(r, "op");
    let dev = field(r, "device");
    let mut args = strings(&["set", id]);
    let mut bin = "prlctl";
    let before;
    let description;
    match op {
        "mode" | "add" | "connect" | "disconnect" | "enable" | "disable" => {
            if !["running", "stopped"].contains(&state) {
                return Err("请先在 PD 中恢复或关闭挂起/暂停的虚拟机，然后修改网卡".into());
            }
            if op != "add" && (!net_id(dev) || v["Hardware"].get(dev).is_none()) {
                return Err("请选择当前虚拟机的有效网卡".into());
            }
            before = v["Hardware"].clone();
            if ["connect", "disconnect", "enable", "disable"].contains(&op) {
                args.extend(strings(&["--device-set", dev, &format!("--{op}")]));
            } else {
                let mode = field(r, "mode");
                if !["shared", "bridged", "host-only"].contains(&mode) {
                    return Err("未知网络模式".into());
                }
                if field(v, "OS").starts_with("mac") && mode == "host-only" {
                    return Err("此界面不向 macOS 来宾配置未经确认支持的仅主机模式".into());
                }
                args.extend(strings(&[
                    if op == "add" {
                        "--device-add"
                    } else {
                        "--device-set"
                    },
                    if op == "add" { "net" } else { dev },
                    "--type",
                    mode,
                ]));
                if mode == "bridged" {
                    let iface = field(r, "iface");
                    if !array(&s["networks"]).iter().any(|n| {
                        field(n, "Type") == "bridged"
                            && field(n, "Bound To") == iface
                            && iface != "FF:FF:FF:FF:FF:FF"
                    }) {
                        return Err(
                            "请选择 PD 当前可桥接的物理接口；USB 直通中的网卡需先归还 Mac".into(),
                        );
                    }
                    args.extend(strings(&["--iface", iface]));
                }
            }
            description = format!(
                "{}：{} {} → {}。更改现有网卡可能中断来宾连接；新增网卡可保留现有连接。",
                field(v, "Name"),
                op,
                dev,
                field(r, "mode")
            );
        }
        "usb-connect" | "usb-disconnect" | "usb-assign" | "usb-host" | "usb-ask" => {
            let usb = array(&s["usb"])
                .iter()
                .find(|u| field(u, "System name") == dev)
                .ok_or("USB 设备已不存在，请刷新")?;
            before = usb.clone();
            if op == "usb-connect" || op == "usb-disconnect" {
                if state != "running" {
                    return Err("即时 USB 连接/释放要求虚拟机运行中".into());
                }
                if field(usb, "Present-On-Host") != "YES" && field(usb, "Connected-To-Vm") == "NO" {
                    return Err("USB 设备当前未插入".into());
                }
                if op == "usb-connect" && field(usb, "Connected-To-Vm") != "NO" {
                    return Err("设备已连接虚拟机；请先在原虚拟机中释放，再连接到目标虚拟机".into());
                }
                args.extend(strings(&[
                    if op == "usb-connect" {
                        "--device-connect"
                    } else {
                        "--device-disconnect"
                    },
                    dev,
                ]));
            } else {
                if field(v, "OS").starts_with("mac") && op == "usb-assign" {
                    return Err(
                        "macOS 来宾的 USB 永久分配受 PD 限制，请使用即时连接或 PD 设备菜单".into(),
                    );
                }
                bin = "prlsrvctl";
                args = strings(&["usb", "set", dev]);
                if op == "usb-assign" {
                    args.push(id.into());
                } else {
                    args.extend(strings(&[
                        "--autoconnect",
                        if op == "usb-host" { "host" } else { "ask" },
                    ]));
                }
            }
            description=format!("USB {}：{op}；目标虚拟机 {}。直通会让 Mac 失去对此设备的使用权。永久归属在下次插入设备或启动虚拟机时生效；不会自动从其他虚拟机抢占设备。",field(usb,"Name"),field(v,"Name"));
        }
        _ => return Err("未知网络管理操作".into()),
    }
    Ok(json!({"request":r,"bin":bin,"args":args,"before":before,"description":description}))
}
pub fn verify(plan: &Value, after: &Value) -> Result<(), String> {
    let r = &plan["request"];
    let v = vm(after, field(r, "vm"))?;
    let op = field(r, "op");
    if op.starts_with("usb-") {
        return Ok(());
    }
    let a = &v["Hardware"][field(r, "device")];
    let correct = match op {
        "mode" => {
            field(a, "type") == field(r, "mode")
                && (field(r, "mode") != "bridged"
                    || bridge_interface(after, a) == field(r, "iface"))
        }
        "connect" => field(a, "state") != "disconnected",
        "disconnect" => field(a, "state") == "disconnected",
        "enable" => a["enabled"] == true,
        "disable" => a["enabled"] == false,
        "add" => adapters(v).iter().any(|(name, a)| {
            plan["before"].get(name).is_none()
                && field(a, "type") == field(r, "mode")
                && (field(r, "mode") != "bridged"
                    || bridge_interface(after, a) == field(r, "iface"))
        }),
        _ => false,
    };
    if correct {
        Ok(())
    } else {
        Err("命令已返回，但回读的网卡状态与请求不一致；请刷新确认，勿重复新增网卡".into())
    }
}
pub fn handle(prl: &str, purpose: &str, args: &[String]) -> Result<String, String> {
    let s = snapshot(prl)?;
    if purpose == "host-read" {
        return Ok(s.to_string());
    }
    let data: Value =
        serde_json::from_str(args.first().ok_or("缺少请求")?).map_err(|e| e.to_string())?;
    if purpose == "host-preview" {
        return Ok(plan(&s, &data)?.to_string());
    }
    if purpose != "host-apply" {
        return Err("未知请求".into());
    }
    let fresh = plan(&s, &data["request"])?;
    if fresh != data {
        return Err("网卡或 USB 状态已变化，请重新预览后应用".into());
    }
    let bin = if field(&fresh, "bin") == "prlsrvctl" {
        srv(prl)
    } else {
        prl.into()
    };
    let argv: Vec<String> = array(&fresh["args"])
        .iter()
        .map(|v| v.as_str().unwrap_or("").into())
        .collect();
    let output = command(&bin, &argv)?;
    let after =
        snapshot(prl).map_err(|e| format!("命令已执行，但状态回读失败：{e}。请刷新确认。"))?;
    verify(&fresh, &after)?;
    Ok(
        json!({"snapshot":after,"result":format!("{}\n{output}",if field(&fresh["request"],"op").starts_with("usb-"){"USB 命令已执行；请查看回读的连接状态和插入时归属。"}else{"网卡变更已执行，回读验证通过。"})})
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"vms":[{"ID":"id","Name":"vm","OS":"ubuntu","State":"running","Hardware":{"net0":{"type":"shared"}}}],"networks":[{"Type":"bridged","Bound To":"en7"}],"usb":[{"Name":"USB LAN","System name":"usb|1","Present-On-Host":"NO","Connected-To-Vm":"NO"}]})
    }
    #[test]
    fn pd_host_alias_and_bridge_display_name() {
        let v = json!({"Hardware":{"net0":{"type":"host"}}});
        assert_eq!(adapters(&v)[0].1["type"], "host-only");
        let s = json!({"interfaces":[{"name":"Wi-Fi","device":"en0"}]});
        assert_eq!(bridge_interface(&s, &json!({"iface":"Wi-Fi"})), "en0");
    }
    #[test]
    fn mode_requires_real_interface() {
        let s = fixture();
        let mut r = json!({"vm":"id","op":"mode","device":"net0","mode":"bridged","iface":"en7"});
        assert!(plan(&s, &r).is_ok());
        r["iface"] = json!("en99");
        assert!(plan(&s, &r).is_err());
    }
    #[test]
    fn unplugged_usb_cannot_connect_but_can_assign() {
        let s = fixture();
        let mut r = json!({"vm":"id","op":"usb-connect","device":"usb|1"});
        assert!(plan(&s, &r).is_err());
        r["op"] = json!("usb-assign");
        assert_eq!(
            plan(&s, &r).unwrap()["args"],
            json!(["usb", "set", "usb|1", "id"])
        );
    }
    #[test]
    fn state_and_hardware_changes_invalidate_plan() {
        let mut s = fixture();
        let r = json!({"vm":"id","op":"mode","device":"net0","mode":"shared"});
        let a = plan(&s, &r).unwrap();
        s["vms"][0]["Hardware"]["net0"]["type"] = json!("bridged");
        assert_ne!(a, plan(&s, &r).unwrap());
        s["vms"][0]["State"] = json!("suspended");
        assert!(plan(&s, &r).is_err());
    }
    #[test]
    fn parses_host_interface_without_ipv6_confusion() {
        let v = parse_interfaces(
            "Hardware Port: USB LAN\nDevice: en7\n",
            "en7: flags=1\n\tinet 10.0.0.1 netmask 0xffffff00\n\tinet6 fe80::1\n\tstatus: active\n",
        );
        assert_eq!(v[0]["name"], "USB LAN");
        assert_eq!(v[0]["ips"], json!(["10.0.0.1"]));
    }
}
