"""Small, dependency-free guest helper. Invoked by the Rust controller through Parallels Tools.
No installation, credentials or root escalation. Only its own local control sockets are managed.
"""
import base64
from collections import deque
from datetime import datetime, timezone
import ipaddress
import json
import os
from pathlib import Path
import platform
import secrets
import selectors
import shutil
import socket
import struct
import subprocess
import sys
import threading
import time


def run(argv):
    try:
        p = subprocess.run(argv, capture_output=True, text=True, timeout=20)
        if p.returncode:
            raise RuntimeError((p.stderr or p.stdout).strip()[:1500])
        return p.stdout.strip()
    except FileNotFoundError:
        raise RuntimeError('未安装命令：' + argv[0])


def containers():
    result = {'containers': [], 'errors': []}
    for engine in ('docker', 'podman'):
        if not shutil.which(engine):
            result['errors'].append(engine + '：未安装')
            continue
        try:
            for line in run([engine, 'ps', '-a', '--format', '{{json .}}']).splitlines():
                row = json.loads(line)
                names = row.get('Names', '')
                if isinstance(names, list): names = ', '.join(names)
                result['containers'].append(dict(engine=engine, name=names, image=row.get('Image', ''),
                    state=row.get('State', ''), status=row.get('Status', ''), ports=row.get('Ports', '')))
        except Exception as e:
            result['errors'].append(engine + '：' + str(e))
    return result


def rootdir():
    root = Path.home() / '.vnetstudio'
    root.mkdir(mode=0o700, exist_ok=True)
    if root.is_symlink() or root.stat().st_uid != os.getuid():
        raise RuntimeError('状态目录所有者不匹配')
    os.chmod(root, 0o700)
    return root


def paths(cfg):
    key = cfg['kind'] + '-' + cfg['ip'].replace('.', '_') + '-' + str(cfg['port'])
    root = rootdir()
    return root / (key + '.json'), root / (key + '.sock')


def control(meta, action):
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(2)
        s.connect(meta['control'])
        s.sendall((json.dumps({'token': meta['token'], 'action': action}) + '\n').encode())
        data = b''
        while True:
            chunk = s.recv(65536)
            if not chunk: break
            data += chunk
            if len(data) > 2_000_000: raise RuntimeError('控制响应过大')
        return json.loads(data)


class Traffic:
    """Bounded connection metadata only: never record payloads or credentials."""
    def __init__(self, cfg):
        self.lock = threading.RLock()
        self.events = deque(maxlen=200)
        self.stats = dict(active=0, accepted=0, rejected=0, failed=0, tx=0, rx=0)
        self.log = paths(cfg)[0].with_suffix('.events')
        self.error = ''
        self.targets = {}

    def add(self, key, amount=1):
        with self.lock: self.stats[key] += amount

    def event(self, event, **fields):
        with self.lock:
            row = dict(time=datetime.now(timezone.utc).isoformat(timespec='seconds'), event=event, **fields)
            self.events.append(row)
            try:
                if self.log.exists() and self.log.stat().st_size >= 1_048_576:
                    self.log.replace(self.log.with_suffix('.events.1'))
                with self.log.open('a') as f:
                    os.chmod(self.log, 0o600)
                    f.write(json.dumps(row, ensure_ascii=False) + '\n')
            except OSError as e: self.error = str(e)

    def target_change(self, target, delta):
        with self.lock:
            count = self.targets.get(target, 0) + delta
            if count > 0: self.targets[target] = count
            else: self.targets.pop(target, None)

    def snapshot(self):
        with self.lock:
            return dict(stats=dict(self.stats), events=list(self.events), log_error=self.error,
                        active_targets=[dict(target=k, connections=v) for k,v in sorted(self.targets.items())])


def list_services():
    rows = []
    root = Path.home() / '.vnetstudio'
    if not root.exists(): return rows
    # Records retain the most recent stopped session for troubleshooting.
    keys = {f.stem for f in root.glob('*.json')} | {f.stem for f in root.glob('*.record')}
    for key in sorted(k for k in keys if k.startswith(('forward-', 'socks-'))):
        try:
            state = root / (key + '.json')
            record = root / (key + '.record')
            meta = json.loads((state if state.exists() else record).read_text())
            live = False
            details = {}
            try:
                details = control(meta, 'status')
                live = details.get('ok', False)
            except Exception: pass
            if not live:
                events = deque(maxlen=200)
                for log in (root / (key + '.events.1'), root / (key + '.events')):
                    if log.exists():
                        with log.open() as f:
                            for line in f:
                                try: events.append(json.loads(line))
                                except ValueError: pass
                details['events'] = list(events)
                last = next((e for e in reversed(events) if e.get('event') == 'stopped'), None)
                if last: details['stats'] = {k: last.get(k, 0) for k in ('active','accepted','rejected','failed','tx','rx')}
                if 'stats' in details: details['stats']['active'] = 0
            rows.append({k: v for k, v in meta.items() if k not in ('token', 'control')} |
                        {k: v for k, v in details.items() if k != 'ok'} |
                        dict(running=live, status='running' if live else ('unknown' if state.exists() else 'stopped'), managed=True, telemetry='connections' if 'stats' in details or details.get('events') else 'unavailable'))
        except Exception:
            continue
    return rows


def recv_exact(s, n):
    data = b''
    while len(data) < n:
        part = s.recv(n - len(data))
        if not part: raise ConnectionError('EOF')
        data += part
    return data


def relay(a, b, traffic=None):
    counts = [0, 0]
    errors = []
    def pump(src, dst, direction):
        try:
            while True:
                data = src.recv(65536)
                if not data: break
                dst.sendall(data)
                counts[direction] += len(data)
                if traffic: traffic.add('tx' if direction == 0 else 'rx', len(data))
        except (OSError, TimeoutError) as e: errors.append(str(e))
        finally:
            try: dst.shutdown(socket.SHUT_WR)
            except OSError: pass
    t = threading.Thread(target=pump, args=(a, b, 0), daemon=True)
    t.start()
    pump(b, a, 1)
    t.join(timeout=305)
    return counts, errors


def allowed_networks(value):
    parts = value.split(',')
    if not parts or len(parts) > 128 or any(not p.strip() for p in parts):
        raise ValueError('来源规则为空或过多')
    networks = [ipaddress.ip_network(p.strip()) for p in parts]
    if any(n.version != 4 for n in networks):
        raise ValueError('仅支持 IPv4 来源规则')
    return networks


def source_allowed(peer, value):
    address = ipaddress.ip_address(peer)
    return any(address in net for net in allowed_networks(value))


def handle(client, peer, cfg, traffic=None):
    def event(name, **fields):
        if traffic: traffic.event(name, peer=f'{peer[0]}:{peer[1]}', **fields)
    with client:
        if not source_allowed(peer[0], cfg['allow']):
            if traffic: traffic.add('rejected')
            event('rejected', error='来源不在允许网段')
            return
        if traffic:
            traffic.add('accepted'); traffic.add('active')
        started = time.monotonic()
        target = cfg.get('target', '')
        connected_target = None
        event('accepted')
        client.settimeout(15)
        try:
            if cfg['kind'] == 'socks':
                ver, count = recv_exact(client, 2)
                if ver != 5: raise ValueError('SOCKS 版本错误')
                methods = recv_exact(client, count)
                if 0 not in methods:
                    client.sendall(b'\x05\xff'); raise ValueError('不支持认证方法')
                client.sendall(b'\x05\x00')
                ver, cmd, reserved, atype = recv_exact(client, 4)
                if ver != 5 or cmd != 1 or reserved != 0:
                    client.sendall(b'\x05\x07\x00\x01' + b'\0' * 6)
                    raise ValueError('仅支持 TCP CONNECT')
                if atype == 1: host = socket.inet_ntoa(recv_exact(client, 4))
                elif atype == 3: host = recv_exact(client, recv_exact(client, 1)[0]).decode('ascii')
                elif atype == 4: host = socket.inet_ntop(socket.AF_INET6, recv_exact(client, 16))
                else:
                    client.sendall(b'\x05\x08\x00\x01' + b'\0' * 6)
                    raise ValueError('地址类型不支持')
                port = struct.unpack('!H', recv_exact(client, 2))[0]
            else:
                host, port = cfg['target'].rsplit(':', 1)
                port = int(port)
            target = f'{host}:{port}'
            try: upstream = socket.create_connection((host, port), timeout=10)
            except OSError:
                if cfg['kind'] == 'socks': client.sendall(b'\x05\x05\x00\x01' + b'\0' * 6)
                raise
            with upstream:
                if cfg['kind'] == 'socks': client.sendall(b'\x05\x00\x00\x01' + b'\0' * 6)
                connected_target = target
                if traffic: traffic.target_change(target, 1)
                event('connected', target=target)
                client.settimeout(300); upstream.settimeout(300)
                counts, errors = relay(client, upstream, traffic)
                if errors and traffic: traffic.add('failed')
                event('closed', target=target, tx=counts[0], rx=counts[1], duration_ms=round((time.monotonic()-started)*1000), error='; '.join(errors))
        except (OSError, ValueError, UnicodeError) as e:
            if traffic: traffic.add('failed')
            event('failed', target=target, error=str(e), duration_ms=round((time.monotonic()-started)*1000))
        finally:
            if traffic:
                if connected_target: traffic.target_change(connected_target, -1)
                traffic.add('active', -1)


def serve(cfg):
    state, control_path = paths(cfg)
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((cfg['ip'], cfg['port']))  # Reject non-local or occupied addresses.
    listener.listen(64)
    admin = socket.socket(socket.AF_UNIX)
    admin.bind(str(control_path)); os.chmod(control_path, 0o600); admin.listen(4)
    meta = {k: v for k, v in cfg.items() if k != 'action'}
    meta.update(started=datetime.now(timezone.utc).isoformat(timespec='seconds'), pid=os.getpid(), token=secrets.token_hex(24), control=str(control_path))
    state.write_text(json.dumps(meta)); os.chmod(state, 0o600)
    record = state.with_suffix('.record')
    record.write_text(json.dumps({k: v for k, v in meta.items() if k not in ('token', 'control')})); os.chmod(record, 0o600)
    traffic = Traffic(cfg)
    traffic.event('started', listen=f"{cfg['ip']}:{cfg['port']}", target=cfg.get('target', ''))
    sel = selectors.DefaultSelector(); sel.register(listener, 1, 'data'); sel.register(admin, 1, 'admin')
    slots = threading.BoundedSemaphore(128)
    def worker(c, p):
        try: handle(c, p, cfg, traffic)
        finally: slots.release()
    try:
        stop = False
        while not stop:
            for key, _ in sel.select():
                c, peer = key.fileobj.accept()
                if key.data == 'data':
                    if slots.acquire(blocking=False): threading.Thread(target=worker, args=(c, peer), daemon=True).start()
                    else:
                        traffic.add('rejected'); traffic.event('rejected', peer=f'{peer[0]}:{peer[1]}', error='并发连接达到上限'); c.close()
                else:
                    with c:
                        c.settimeout(2)
                        try:
                            req = json.loads(c.recv(2048))
                            valid = secrets.compare_digest(req.get('token', ''), meta['token'])
                            reply = dict(ok=valid)
                            if valid and req.get('action') == 'status': reply.update(traffic.snapshot())
                            c.sendall(json.dumps(reply).encode())
                            stop = valid and req.get('action') == 'stop'
                        except (ValueError, OSError): pass
    finally:
        traffic.event('stopped', **traffic.snapshot()['stats'])
        sel.close(); listener.close(); admin.close()
        control_path.unlink(missing_ok=True); state.unlink(missing_ok=True)


def service(cfg):
    state, ctl = paths(cfg)
    if cfg['action'] == 'delete':
        record = state.with_suffix('.record')
        if not state.exists() and not record.exists():
            raise RuntimeError('没有本工具管理的规则，拒绝删除')
        if state.exists():
            service(dict(cfg, action='stop'))
        if state.exists() or ctl.exists():
            raise RuntimeError('尚未确认服务退出，未删除规则')
        record.unlink(missing_ok=True)
        # Historical events are retained on disk, but no longer enumerate as a rule.
        return '规则已删除'
    if cfg['action'] in ('remove', 'stop'):
        if not state.exists():
            if state.with_suffix('.record').exists(): return '服务已停止'
            raise RuntimeError('没有本工具管理的服务，拒绝停止其他进程')
        meta = json.loads(state.read_text())
        if not control(meta, 'stop').get('ok'): raise RuntimeError('控制令牌验证失败')
        for _ in range(30):
            if not state.exists(): return '服务已停止'
            time.sleep(.1)
        raise RuntimeError('停止请求已发送，但尚未确认退出')
    if state.exists() or ctl.exists(): raise RuntimeError('配置已存在；请先查看或停止。不自动覆盖残留状态')
    ip = ipaddress.ip_address(cfg['ip'])
    allowed_networks(cfg['allow'])
    if ip.version != 4 or ip.is_unspecified or not (0 < cfg['port'] < 65536): raise RuntimeError('无效监听地址')
    # First bind before spawning, to return a useful error without adding firewall rules.
    with socket.socket() as check:
        check.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        check.bind((cfg['ip'], cfg['port']))
    payload = dict(cfg); payload['action'] = '_serve'
    loader = 'import base64;exec(base64.b64decode(' + repr(SOURCE_B64) + '))'
    log = rootdir() / 'last-start.log'
    with log.open('w') as out:
        os.chmod(log, 0o600)
        p = subprocess.Popen([sys.executable, '-c', loader, json.dumps(payload)], stdin=subprocess.DEVNULL,
                             stdout=out, stderr=out, start_new_session=True)
    for _ in range(40):
        if p.poll() is not None: raise RuntimeError('代理启动失败：' + log.read_text()[-1200:])
        if state.exists():
            try:
                if control(json.loads(state.read_text()), 'status')['ok']: return '已启动 ' + cfg['kind'] + ' ' + cfg['ip'] + ':' + str(cfg['port'])
            except (OSError, ValueError): pass
        time.sleep(.1)
    p.terminate()
    raise RuntimeError('代理启动超时，已停止启动进程')


def main(cfg):
    kind, action = cfg['kind'], cfg['action']
    if action == '_serve': return serve(cfg)
    if kind == 'addresses':
        rows = []
        if platform.system() == 'Darwin':
            iface = ''
            for line in run(['ifconfig']).splitlines():
                if line and not line[0].isspace(): iface = line.split(':')[0]
                words = line.split()
                if len(words) > 1 and words[0] == 'inet':
                    rows.append(dict(ip=words[1], interface=iface, state='assigned'))
        else:
            for iface in json.loads(run(['ip', '-j', '-4', 'addr', 'show'])):
                for addr in iface.get('addr_info', []):
                    rows.append(dict(ip=addr['local'], interface=iface['ifname'], prefix=addr.get('prefixlen'), state=iface.get('operstate', '')))
        return rows
    if kind == 'containers': return containers()
    if kind == 'services': return dict(services=list_services(), note='连接日志仅记录地址、时间、字节数与错误，不记录通信内容；旧进程需手动重建才支持日志。')
    if kind == 'status':
        cmds = [['ifconfig'], ['netstat', '-rn', '-f', 'inet']] if platform.system() == 'Darwin' else [['ip', '-brief', 'address'], ['ip', 'route'], ['ss', '-lnt']]
        parts = []
        for argv in cmds:
            try: parts.append('$ ' + ' '.join(argv) + '\n' + run(argv))
            except Exception as e: parts.append(str(e))
        parts.append('本工具代理/转发：\n' + json.dumps(list_services(), ensure_ascii=False, indent=2))
        return '\n\n'.join(parts)
    if kind == 'vpn':
        name = cfg.get('name', '')
        if platform.system() == 'Darwin':
            argv = ['scutil', '--nc', 'list'] if action == 'list' else ['scutil', '--nc', 'start' if action == 'connect' else 'stop', name]
        else:
            if action == 'list': argv = ['nmcli', '-f', 'NAME,TYPE,DEVICE', 'connection', 'show']
            else:
                typ = run(['nmcli', '-g', 'connection.type', 'connection', 'show', 'id', name])
                if typ not in ('vpn', 'wireguard'): raise RuntimeError('该连接不是 VPN/WireGuard，拒绝操作普通网卡')
                argv = ['nmcli', '--wait', '15', 'connection', 'up' if action == 'connect' else 'down', 'id', name]
        return run(argv) or '命令已完成，请刷新 VPN 状态确认'
    if action == 'list': return list_services()
    return service(cfg)


if __name__ == '__main__':
    try:
        result = main(json.loads(sys.argv[1]))
        if result is not None: print(result if isinstance(result, str) else json.dumps(result, ensure_ascii=False))
    except Exception as e:
        print('错误：' + str(e), file=sys.stderr)
        sys.exit(1)
