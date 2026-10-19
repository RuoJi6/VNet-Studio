import importlib.util
import pathlib
import socket
import tempfile
import threading
import json
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('agent', pathlib.Path(__file__).with_name('unix_agent.py'))
agent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(agent)


class AgentTests(unittest.TestCase):
    def test_linux_address_rows(self):
        raw = '[{"ifname":"eth0","operstate":"UP","addr_info":[{"local":"10.211.55.19","prefixlen":24}]}]'
        with patch.object(agent.platform, 'system', return_value='Linux'), patch.object(agent, 'run', return_value=raw):
            self.assertEqual(agent.main({'kind':'addresses','action':'list'}), [{'ip':'10.211.55.19','interface':'eth0','prefix':24,'state':'UP'}])

    def test_mac_address_rows(self):
        raw = 'en0: flags=1\n\tinet6 fe80::1\n\tinet 10.0.0.4 netmask 0xffffff00\n'
        with patch.object(agent.platform, 'system', return_value='Darwin'), patch.object(agent, 'run', return_value=raw):
            self.assertEqual(agent.main({'kind':'addresses','action':'list'}), [{'ip':'10.0.0.4','interface':'en0','state':'assigned'}])

    def test_source_acl_rejects_before_protocol(self):
        a, b = socket.socketpair()
        with b:
            agent.handle(a, ('10.0.0.2', 123), {'allow': '10.0.0.1/32'})
            self.assertEqual(b.recv(1), b'')

    def test_multiple_and_all_source_rules(self):
        acl='10.211.55.2/32,10.211.55.18/32'
        self.assertTrue(agent.source_allowed('10.211.55.2',acl))
        self.assertTrue(agent.source_allowed('10.211.55.18',acl))
        self.assertFalse(agent.source_allowed('10.211.55.19',acl))
        self.assertTrue(agent.source_allowed('203.0.113.9','0.0.0.0/0'))
        for invalid in ['', '10.0.0.1/32,', '10.0.0.1/32,invalid', '::/0']:
            with self.assertRaises(ValueError): agent.allowed_networks(invalid)

    def test_no_udp_or_bind(self):
        a, b = socket.socketpair()
        with b:
            b.sendall(b'\x05\x01\x00\x05\x03\x00\x01')
            agent.handle(a, ('10.0.0.1', 123), {'kind': 'socks', 'allow': '10.0.0.1/32'})
            self.assertEqual(b.recv(20), b'\x05\x00\x05\x07\x00\x01'+b'\0'*6)

    def test_mac_vpn_dispatch(self):
        with patch.object(agent.platform, 'system', return_value='Darwin'), patch.object(agent, 'run', return_value='Connected') as run:
            agent.main({'kind': 'vpn', 'action': 'connect', 'name': 'Test VPN'})
            run.assert_called_once_with(['scutil', '--nc', 'start', 'Test VPN'])

    def test_linux_refuses_non_vpn_connection(self):
        with patch.object(agent.platform, 'system', return_value='Linux'), patch.object(agent, 'run', return_value='ethernet'):
            with self.assertRaisesRegex(RuntimeError, '不是 VPN'):
                agent.main({'kind': 'vpn', 'action': 'disconnect', 'name': 'LAN'})

    def test_container_engine_failure_reported(self):
        with patch.object(agent.shutil, 'which', side_effect=lambda x: '/bin/docker' if x == 'docker' else None), patch.object(agent, 'run', side_effect=RuntimeError('daemon offline')):
            r = agent.containers()
            self.assertEqual(r['containers'], [])
            self.assertEqual(len(r['errors']), 2)


class TrafficTests(unittest.TestCase):
    def telemetry(self, directory):
        with patch.object(agent, 'paths', return_value=(pathlib.Path(directory)/'test.json', pathlib.Path(directory)/'test.sock')):
            return agent.Traffic({})

    def test_relay_logs_bytes_without_payload(self):
        with tempfile.TemporaryDirectory() as d:
            telemetry = self.telemetry(d)
            client, request = socket.socketpair()
            upstream, server = socket.socketpair()
            secret = b'private HTTP body not for logs'
            def echo():
                with server:
                    self.assertEqual(server.recv(1024), secret)
                    server.sendall(b'OK')
            thread = threading.Thread(target=echo); thread.start()
            with request, patch.object(agent.socket, 'create_connection', return_value=upstream):
                request.sendall(secret);request.shutdown(socket.SHUT_WR)
                agent.handle(client, ('10.0.0.1', 777), {'kind':'forward','target':'127.0.0.1:1234','allow':'10.0.0.1/32'}, telemetry)
                self.assertEqual(request.recv(100), b'OK')
            thread.join(2)
            result=telemetry.snapshot()
            self.assertEqual(result['stats'], dict(active=0,accepted=1,rejected=0,failed=0,tx=len(secret),rx=2))
            self.assertEqual([e['event'] for e in result['events']], ['accepted','connected','closed'])
            self.assertNotIn(secret.decode(), telemetry.log.read_text())
            self.assertEqual(result['events'][-1]['target'], '127.0.0.1:1234')

    def test_live_target_lifetime_including_relay_failure(self):
        with tempfile.TemporaryDirectory() as d:
            telemetry=self.telemetry(d)
            cfg={'kind':'forward','target':'127.0.0.1:1234','allow':'10.0.0.1/32'}
            def relay(a,b,t):
                self.assertEqual(t.snapshot()['active_targets'],[dict(target=cfg['target'],connections=1)])
                raise OSError('injected relay failure')
            client,request=socket.socketpair(); upstream,server=socket.socketpair()
            with request,server,patch.object(agent.socket,'create_connection',return_value=upstream),patch.object(agent,'relay',side_effect=relay):
                agent.handle(client,('10.0.0.1',99),cfg,telemetry)
            self.assertEqual(telemetry.snapshot()['active_targets'],[])
            self.assertEqual(telemetry.stats['active'],0)

    def test_target_reference_counts_survive_other_connection_close(self):
        with tempfile.TemporaryDirectory() as d:
            t=self.telemetry(d)
            t.target_change('one:80',1);t.target_change('one:80',1);t.target_change('two:443',1)
            t.target_change('one:80',-1)
            self.assertEqual(t.snapshot()['active_targets'],[dict(target='one:80',connections=1),dict(target='two:443',connections=1)])
            t.target_change('one:80',-1)
            self.assertEqual(t.snapshot()['active_targets'],[dict(target='two:443',connections=1)])

    def test_failure_and_acl_are_distinct(self):
        with tempfile.TemporaryDirectory() as d:
            t=self.telemetry(d)
            for peer in ('10.0.0.2','10.0.0.1'):
                a,b=socket.socketpair()
                with b, patch.object(agent.socket,'create_connection',side_effect=ConnectionRefusedError('connection refused')):
                    agent.handle(a,(peer,22),{'kind':'forward','allow':'10.0.0.1/32','target':'127.0.0.1:4'},t)
            self.assertEqual(t.stats['rejected'],1)
            self.assertEqual(t.stats['failed'],1)
            self.assertEqual(t.stats['active'],0)
            self.assertIn('connection refused',t.snapshot()['events'][-1]['error'])

    def test_log_rotation_and_memory_bound(self):
        with tempfile.TemporaryDirectory() as d:
            t=self.telemetry(d)
            for i in range(300): t.event('test',error='x'*5000)
            self.assertEqual(len(t.events),200)
            self.assertTrue(t.log.with_suffix('.events.1').exists())
            self.assertLess(t.log.stat().st_size,1_060_000)

    def test_stopped_services_and_no_tokens(self):
        with tempfile.TemporaryDirectory() as d, patch.object(agent.Path,'home',return_value=pathlib.Path(d)):
            root=agent.rootdir(); f=root/'forward-10_0_0_1-8.record'
            f.write_text(json.dumps(dict(kind='forward',ip='10.0.0.1',port=8)))
            f.with_suffix('.events').write_text(json.dumps(dict(event='stopped',accepted=1,tx=123,rx=45))+'\n')
            row=agent.list_services()[0]
            self.assertFalse(row['running'])
            self.assertNotIn('token',row)
            self.assertEqual(row['events'][0]['event'],'stopped')


class LifecycleTests(unittest.TestCase):
    def test_delete_stopped_rule_retains_logs_and_other_rules(self):
        with tempfile.TemporaryDirectory() as d, patch.object(agent.Path,'home',return_value=pathlib.Path(d)):
            cfg=dict(kind='forward',ip='10.0.0.1',port=1234,action='delete')
            state,ctl=agent.paths(cfg);record=state.with_suffix('.record')
            record.write_text(json.dumps(cfg));log=state.with_suffix('.events');log.write_text('history')
            other=state.parent/'socks-10_0_0_1-8888.record';other.write_text('other')
            self.assertEqual(agent.service(cfg),'规则已删除')
            self.assertFalse(record.exists());self.assertTrue(other.exists());self.assertEqual(log.read_text(),'history')
            with self.assertRaises(RuntimeError):agent.service(cfg)

    def test_failed_stop_preserves_record_for_running_rule(self):
        with tempfile.TemporaryDirectory() as d, patch.object(agent.Path,'home',return_value=pathlib.Path(d)):
            cfg=dict(kind='forward',ip='10.0.0.1',port=1234,action='delete')
            state,ctl=agent.paths(cfg);state.write_text('{}');record=state.with_suffix('.record');record.write_text('{}')
            with patch.object(agent,'control',side_effect=OSError('unreachable')):
                with self.assertRaises(OSError):agent.service(cfg)
            self.assertTrue(state.exists());self.assertTrue(record.exists())

    def test_stop_is_idempotent_only_for_owned_rule(self):
        with tempfile.TemporaryDirectory() as d, patch.object(agent.Path,'home',return_value=pathlib.Path(d)):
            cfg=dict(kind='socks',ip='10.0.0.1',port=1234,action='stop')
            state,_=agent.paths(cfg)
            with self.assertRaises(RuntimeError):agent.service(cfg)
            state.with_suffix('.record').write_text('{}')
            self.assertEqual(agent.service(cfg),'服务已停止')


if __name__ == '__main__': unittest.main()
