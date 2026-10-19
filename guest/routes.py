"""IPv4 guest routing: structured plans, conflict checks, reversible runtime changes."""
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import platform
import re
import socket
import subprocess
import sys


def run(args):
    p = subprocess.run(args, capture_output=True, text=True, timeout=15)
    if p.returncode: raise RuntimeError((p.stderr or p.stdout).strip()[:2000])
    return p.stdout.strip()


def ipv4(value):
    ip = ipaddress.IPv4Address(value)
    if ip.is_multicast or ip.is_unspecified or ip.is_loopback or int(ip) >> 24 == 0 or int(ip) >> 24 >= 240 or str(ip) == '255.255.255.255':
        raise ValueError('请填写有效的非回环单播 IPv4 地址')
    return str(ip)


def prefix(value):
    net = ipaddress.IPv4Network(value if '/' in value else value + '/32', strict=False)
    if net.prefixlen == 0: return '0.0.0.0/0'
    if net.is_multicast or net.is_loopback or net.network_address.is_unspecified:
        raise ValueError('不支持该目标网段')
    return str(net)


def state_path():
    root = Path.home() / '.vnetstudio'
    root.mkdir(mode=0o700, exist_ok=True)
    if root.is_symlink() or root.stat().st_uid != os.getuid(): raise RuntimeError('状态目录所有者不匹配')
    os.chmod(root, 0o700)
    return root / 'routes-last.record'


def mac_routes(raw):
    rows = []
    for line in raw.splitlines():
        words = line.split()
        if len(words) < 4 or words[0] in ('Routing', 'Internet:', 'Destination'): continue
        dst, gw, flags, iface = words[:4]
        # Interface-scoped and cloned/cache routes are visible but never rewritten.
        try:
            if dst == 'default': dst = '0.0.0.0/0'
            else:
                ip, sep, mask = dst.partition('/')
                parts=ip.split('.')
                dst = prefix('.'.join(parts + ['0']*(4-len(parts))) + '/' + (mask if sep else str(len(parts)*8)))
        except ValueError: continue
        rows.append(dict(destination=dst, gateway=gw if 'G' in flags else '', interface=iface,
                         metric=0, protocol='static' if 'S' in flags else 'system', flags=flags,
                         protected=any(c in flags for c in 'IWYLb') or ('G' not in flags and 'S' not in flags)))
    return rows


def snapshot():
    mac = platform.system() == 'Darwin'
    interfaces=[]; warnings=[]
    if mac:
        current=None
        for line in run(['/sbin/ifconfig']).splitlines():
            if line and not line[0].isspace():
                current=dict(id=line.split(':')[0],name=line.split(':')[0],ips=[],state='UP' if '<UP,' in line else 'DOWN');interfaces.append(current)
            m=re.search(r'inet (\d+\.\d+\.\d+\.\d+) netmask (0x[0-9a-f]+)',line)
            if m and current is not None:
                current['ips'].append(m[1]+'/'+str(bin(int(m[2],16)).count('1')))
        rows=mac_routes(run(['/usr/sbin/netstat','-rn','-f','inet']))
        try:dns=run(['/usr/sbin/scutil','--dns'])
        except Exception as e:dns=str(e);warnings.append('DNS 信息未能读取；不影响 IPv4 路由表显示。')
        identity=socket.gethostname()+' '+run(['/usr/sbin/sysctl','-n','kern.boottime'])
        warnings.append('macOS 的接口作用域、动态缓存和系统直连路由只读；网络服务重连可能恢复默认路由。')
        policy=[]
    else:
        for i in json.loads(run(['ip','-j','-4','addr','show'])):
            interfaces.append(dict(id=i['ifname'],name=i['ifname'],ips=[a['local']+'/'+str(a['prefixlen']) for a in i.get('addr_info',[])],state=i.get('operstate','UNKNOWN')))
        raw=json.loads(run(['ip','-j','-4','route','show','table','main']))
        rows=[]
        for r in raw:
            simple=set(r) <= {'dst','gateway','dev','protocol','scope','prefsrc','metric','flags','type'} and not r.get('flags') and r.get('type','unicast')=='unicast'
            rows.append(dict(destination='0.0.0.0/0' if r.get('dst')=='default' else str(ipaddress.IPv4Network(r.get('dst','0.0.0.0/0'),strict=False)),gateway=r.get('gateway',''),interface=r.get('dev',''),metric=r.get('metric',0),protocol=r.get('protocol',''),source=r.get('prefsrc',''),scope=r.get('scope','global'),protected=not simple or r.get('protocol')=='kernel',raw=r))
        policy=json.loads(run(['ip','-j','-4','rule','show']))
        if any(r.get('table') not in ('local','main','default',253,254,255) or any(k in r for k in ('fwmark','iif','oif','uidrange')) for r in policy):
            warnings.append('存在策略路由/VPN 规则；修改 main 表不保证改变所有应用出口，请使用实际出口查询核对。')
        try:dns=Path('/etc/resolv.conf').read_text()
        except OSError as e:dns=str(e);warnings.append('DNS 信息未能读取；不影响 IPv4 路由表显示。')
        identity=Path('/proc/sys/kernel/random/boot_id').read_text().strip()
    rows.sort(key=lambda r:json.dumps(r,sort_keys=True))
    # Ignore macOS dynamic ARP entries for optimistic concurrency checks.
    stable=[r for r in rows if not (mac and any(c in r.get('flags','') for c in 'WYL'))]
    stableifs=[dict(id=i['id'],ips=i['ips'],state=i['state']) for i in interfaces]
    digest=hashlib.sha256(json.dumps([identity,stableifs,stable,policy],sort_keys=True).encode()).hexdigest()
    return dict(epoch=identity,platform='macOS' if mac else 'Linux',interfaces=interfaces,routes=rows,dns=dns,warnings=warnings,fingerprint=digest,can_rollback=state_path().exists())


def affected(s, dst):
    return [r for r in s['routes'] if r['destination']==dst and not ('I' in r.get('flags',''))]


def plan(request, s):
    op=request.get('op')
    if op not in ('default','add','delete'): raise ValueError('未知路由操作')
    dst='0.0.0.0/0' if op=='default' else prefix(request.get('destination',''))
    if dst=='0.0.0.0/0' and op!='default': raise ValueError('默认路由请使用默认出口功能')
    old=affected(s,dst)
    if any(r.get('protected') for r in old): raise ValueError('此目标包含系统直连或复杂路由，不支持自动修改')
    if op=='add' and old: raise ValueError('此网段已有路由；请先查看并删除精确匹配的旧路由')
    if op=='delete' and not old: raise ValueError('没有精确匹配的可删除路由')
    new=[]
    if op!='delete':
        iface=request.get('interface','')
        i=next((i for i in s['interfaces'] if i['id']==iface),None)
        if i is None or not i['ips']: raise ValueError('所选网卡不存在或没有 IPv4 地址')
        if i['state']=='DOWN' or iface=='lo' or iface=='lo0': raise ValueError('不能使用已关闭或回环网卡')
        gw=request.get('gateway','').strip()
        if gw:
            gw=ipv4(gw)
            if not any(ipaddress.IPv4Address(gw) in ipaddress.IPv4Interface(a).network and str(ipaddress.IPv4Interface(a).ip)!=gw for a in i['ips']):
                raise ValueError('网关必须是所选网卡直连网段中的另一台主机')
        elif op=='default': raise ValueError('默认出口必须填写可达网关')
        elif not any(ipaddress.IPv4Network(dst).subnet_of(ipaddress.IPv4Interface(a).network) for a in i['ips']):
            raise ValueError('目标不是直连网段，请填写该网卡能到达的下一跳网关')
        new=[dict(destination=dst,gateway=gw,interface=iface,metric=0,protocol='static',scope='global' if gw else 'link',source='',protected=False)]
    return dict(epoch=s.get('epoch',''),request=request,fingerprint=s['fingerprint'],destination=dst,before=old,after=new,
                description=('更换默认 IPv4 出口' if op=='default' else '新增指定路由' if op=='add' else '删除精确匹配路由')+'：'+dst,
                note='仅当前运行期间生效；不修改 DNS、IPv6、DHCP 配置及 VPN 策略。重连或重启可能覆盖。')


def route_args(op,r,mac=False):
    dst=r['destination']
    if mac:
        args=['/sbin/route','-n',op,'-net',dst]
        if r['gateway']: args += [r['gateway'],'-ifp',r['interface']]
        else: args += ['-interface',r['interface']]
        return args
    args=['ip','-4','route',op,dst]
    if r['gateway']: args += ['via',r['gateway']]
    args += ['dev',r['interface'],'metric',str(r['metric'])]
    if op=='add':
        if r.get('protocol'):args += ['proto',str(r['protocol'])]
        if r.get('source'):args += ['src',r['source']]
        if r.get('scope'):args += ['scope',r['scope']]
    return args


def signature(rows):
    return sorted((r['destination'],r['gateway'],r['interface'],int(r['metric'])) for r in rows)


def replace_rows(current, desired, mac):
    removed=[]; added=[]
    try:
        for r in current:run(route_args('delete' if mac else 'del',r,mac));removed.append(r)
        for r in desired:run(route_args('add',r,mac));added.append(r)
    except Exception as error:
        failures=[]
        for r in reversed(added):
            try:run(route_args('delete' if mac else 'del',r,mac))
            except Exception as e:failures.append(str(e))
        for r in removed:
            try:run(route_args('add',r,mac))
            except Exception as e:failures.append(str(e))
        raise RuntimeError(str(error)+'；回滚：'+('; '.join(failures) if failures else '成功'))


def main(cfg):
    action=cfg['action']
    s=snapshot()
    if action=='list':return s
    if action=='lookup':
        ip=ipv4(cfg['request'].get('ip',''))
        raw=run(['/sbin/route','-n','get',ip] if s['platform']=='macOS' else ['ip','-j','-4','route','get',ip])
        return dict(ip=ip,result=raw,note='查询来宾内核路由选择；不会向目标发包，也不保证目标服务可达。')
    if action=='preview':return plan(cfg['request'],s)
    if action=='apply':
        supplied=cfg['request']
        if supplied.get('fingerprint')!=s['fingerprint']:raise RuntimeError('网卡或路由已变化，请重新预览')
        p=plan(supplied['request'],s)
    elif action=='rollback':
        path=state_path()
        if not path.exists():raise RuntimeError('没有可撤销的变更')
        last=json.loads(path.read_text())
        if last.get('epoch','')!=s.get('epoch',''):raise RuntimeError('系统已经重启，拒绝恢复上次启动期间的路由')
        if signature(affected(s,last['destination']))!=signature(last['after']):raise RuntimeError('相关路由已被其他程序修改，拒绝自动撤销')
        p=dict(before=affected(s,last['destination']),after=last['before'],destination=last['destination'])
    else:raise ValueError('未知操作')
    if os.geteuid()!=0:raise RuntimeError('需要来宾 root 权限；未修改任何路由')
    # Persist recovery information before the first write.
    journal=state_path().with_suffix('.pending')
    journal.write_text(json.dumps(p));os.chmod(journal,0o600)
    mac=s['platform']=='macOS'
    replace_rows(p['before'],p['after'],mac)
    post=snapshot()
    if signature(affected(post,p['destination']))!=signature(p['after']):
        # Keep the journal: an external DHCP/VPN writer may be racing us.
        raise RuntimeError('执行后路由与计划不一致；请刷新核对。恢复信息保存在 '+str(journal))
    if action=='apply':
        state_path().write_text(json.dumps(p));os.chmod(state_path(),0o600)
    else:state_path().unlink(missing_ok=True)
    journal.unlink(missing_ok=True)
    post['can_rollback']=state_path().exists()
    return dict(result='已应用并核对路由' if action=='apply' else '已撤销并核对路由',snapshot=post)

if __name__=='__main__':
    try:
        import fcntl
        with state_path().with_suffix('.lock').open('a') as lock:
            os.chmod(lock.name,0o600)
            fcntl.flock(lock,fcntl.LOCK_EX | fcntl.LOCK_NB)
            print(json.dumps(main(json.loads(sys.argv[1])),ensure_ascii=False))
    except Exception as e:print('错误：'+str(e),file=sys.stderr);sys.exit(1)
